use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, Generics, TypeParam, parse_quote};

use super::model::{FieldKind, Position, Scalar, Schema, ValueType};
use super::{
    builder_optional_size, fresh_field_ident, fresh_type_ident, pascal, private_ident,
    scalar_type_tokens, to_bytes_method, value_type_tokens,
};

pub(super) fn render(schema: &Schema, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let vis = &schema.vis;
    let name = &schema.name;
    let builder = format_ident!("{}Builder", name.unraw());
    let error = format_ident!("{}WriteError", name.unraw());
    let validator_references = schema
        .validators
        .iter()
        .map(|validator| quote!(let _ = #validator;));
    let original_arguments = generic_arguments(&schema.generics);
    let slots = slots(schema);
    let marker = fresh_field_ident(schema, "schema");

    let mut builder_generics = schema.generics.clone();
    for slot in &slots {
        let state = &slot.state;
        let mut parameter = TypeParam::from(state.clone());
        parameter.default = Some(parse_quote!(#runtime::__private::Unset));
        builder_generics.params.push(GenericParam::Type(parameter));
    }
    let builder_declaration_generics = builder_generics.clone();
    let initial_states = slots
        .iter()
        .map(|_| quote!(#runtime::__private::Unset))
        .collect::<Vec<_>>();
    let initial_builder = builder_type(&builder, &original_arguments, &initial_states);

    let builder_fields = slots.iter().map(|slot| {
        let field = &slot.field;
        let state = &slot.state;
        quote!(#field: #state,)
    });
    let builder_initializers = slots.iter().map(|slot| {
        let field = &slot.field;
        quote!(#field: #runtime::__private::Unset,)
    });

    let (_, schema_types, _) = schema.generics.split_for_impl();
    let mut capability_generics = schema.generics.clone();
    for field in schema.nested_fields() {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        capability_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: #runtime::WireBuilder));
    }
    let (schema_impl, _, schema_where) = capability_generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let setters = render_setters(
        schema,
        &builder,
        &marker,
        &original_arguments,
        &slots,
        runtime,
    );
    let complete = render_complete(
        schema,
        &builder,
        &error,
        &original_arguments,
        &slots,
        runtime,
    );

    let fixed_size = if schema.has_explicit_geometry() {
        quote!(None)
    } else {
        builder_optional_size(schema, runtime)
    };
    Ok(quote! {
        const _: () = {
            #(#validator_references)*
        };

        #[doc(hidden)]
        #vis struct #builder #builder_declaration_generics {
            #(#builder_fields)*
            #marker: ::core::marker::PhantomData<fn() -> #self_type>,
        }

        impl #schema_impl #runtime::WireBuilder for #self_type #schema_where {
            const FIXED_SIZE: Option<usize> = #fixed_size;
            type Builder = #initial_builder;

            #[inline(always)]
            fn builder() -> Self::Builder {
                #builder {
                    #(#builder_initializers)*
                    #marker: ::core::marker::PhantomData,
                }
            }
        }


        #(#setters)*
        #complete
    })
}

pub(super) struct Slot {
    pub(super) field: syn::Ident,
    pub(super) state: syn::Ident,
    pub(super) kind: SlotKind,
}

pub(super) enum SlotKind {
    Value(TokenStream),
    RawBytes,
    Nested(Box<syn::Type>),
}

pub(super) fn slots(schema: &Schema) -> Vec<Slot> {
    let mut used = schema.generics.clone();
    let mut slots = Vec::new();
    for field in &schema.fields {
        if field.kind.constant().is_some() || schema.is_length_controller(&field.name) {
            continue;
        }
        let state = fresh_type_ident(&used, &format!("{}State", pascal(&field.name)));
        used.params
            .push(GenericParam::Type(TypeParam::from(state.clone())));
        let kind = match &field.kind {
            FieldKind::Scalar(scalar) => SlotKind::Value(value_type_tokens(scalar.value_type)),
            FieldKind::Bytes(_) => {
                let ty = &field.ty;
                SlotKind::Value(quote!(#ty))
            }
            FieldKind::RawBytes(_) => SlotKind::RawBytes,
            FieldKind::Nested(nested) => SlotKind::Nested(Box::new(nested.ty.clone())),
        };
        slots.push(Slot {
            field: field.name.clone(),
            state,
            kind,
        });
    }
    slots
}

fn render_setters(
    schema: &Schema,
    builder: &syn::Ident,
    marker: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    let vis = &schema.vis;
    let mut rendered = Vec::new();
    for (target_index, target) in slots.iter().enumerate() {
        let mut impl_generics = schema.generics.clone();
        let build_fn = fresh_type_ident(&impl_generics, "BuildFn");
        let child_builder = fresh_type_ident(&impl_generics, "ChildBuilder");
        let raw_bytes = fresh_type_ident(&impl_generics, "RawBytes");
        let mut current_states = Vec::with_capacity(slots.len());
        let mut returned_states = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            if index == target_index {
                current_states.push(quote!(#runtime::__private::Unset));
                returned_states.push(match &target.kind {
                    SlotKind::Value(ty) => quote!(#runtime::__private::Set<#ty>),
                    SlotKind::RawBytes => quote!(#runtime::__private::Set<#raw_bytes>),
                    SlotKind::Nested(_) => quote!(#runtime::__private::Set<#child_builder>),
                });
            } else {
                let state = &slot.state;
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(state.clone())));
                current_states.push(quote!(#state));
                returned_states.push(quote!(#state));
            }
        }
        let (impl_params, _, impl_where) = impl_generics.split_for_impl();
        let current_builder = builder_type(builder, original_arguments, &current_states);
        let returned_builder = builder_type(builder, original_arguments, &returned_states);
        let field = &target.field;
        let assignments = slots.iter().enumerate().map(|(index, slot)| {
            let name = &slot.field;
            if index == target_index {
                quote!(#name: #runtime::__private::Set(value),)
            } else {
                quote!(#name: self.#name,)
            }
        });

        match &target.kind {
            SlotKind::Value(ty) => {
                rendered.push(quote! {
                    impl #impl_params #current_builder #impl_where {
                        #[doc = concat!("Sets field `", stringify!(#field), "`.")]
                        #[inline(always)]
                        #vis fn #field(self, value: #ty) -> #returned_builder {
                            #builder {
                                #(#assignments)*
                                #marker: ::core::marker::PhantomData,
                            }
                        }
                    }
                });
            }
            SlotKind::RawBytes => {
                rendered.push(quote! {
                    impl #impl_params #current_builder #impl_where {
                        #[doc = concat!("Sets raw byte field `", stringify!(#field), "`.")]
                        #[inline(always)]
                        #vis fn #field<#raw_bytes>(self, value: #raw_bytes) -> #returned_builder
                        where
                            #raw_bytes: AsRef<[u8]>,
                        {
                            #builder {
                                #(#assignments)*
                                #marker: ::core::marker::PhantomData,
                            }
                        }
                    }
                });
            }
            SlotKind::Nested(ty) => {
                rendered.push(quote! {
                    impl #impl_params #current_builder #impl_where {
                        #[doc = concat!("Builds nested field `", stringify!(#field), "`.")]
                        #[inline(always)]
                        #vis fn #field<#build_fn, #child_builder>(
                            self,
                            build: #build_fn,
                        ) -> #returned_builder
                        where
                            #ty: #runtime::WireBuilder,
                            #build_fn: FnOnce(
                                <#ty as #runtime::WireBuilder>::Builder,
                            ) -> #child_builder,
                        {
                            let value = build(<#ty as #runtime::WireBuilder>::builder());
                            #builder {
                                #(#assignments)*
                                #marker: ::core::marker::PhantomData,
                            }
                        }
                    }
                });
            }
        }
    }
    rendered
}

fn render_complete(
    schema: &Schema,
    builder: &syn::Ident,
    error: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let name = &schema.name;
    let mut impl_generics = schema.generics.clone();
    let mut complete_states = Vec::with_capacity(slots.len());
    let mut nested_builders = Vec::new();
    for slot in slots {
        match &slot.kind {
            SlotKind::Value(ty) => {
                complete_states.push(quote!(#runtime::__private::Set<#ty>));
            }
            SlotKind::RawBytes => {
                let parameter =
                    fresh_type_ident(&impl_generics, &format!("{}Bytes", pascal(&slot.field)));
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(parameter.clone())));
                impl_generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#parameter: AsRef<[u8]>));
                complete_states.push(quote!(#runtime::__private::Set<#parameter>));
            }
            SlotKind::Nested(ty) => {
                let parameter =
                    fresh_type_ident(&impl_generics, &format!("{}Builder", pascal(&slot.field)));
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(parameter.clone())));
                impl_generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(
                        #ty: #runtime::WireBuilder + #runtime::WireWrite<#parameter>
                    ));
                complete_states.push(quote!(#runtime::__private::Set<#parameter>));
                nested_builders.push((slot.field.clone(), ty.clone(), parameter));
            }
        }
    }
    let complete_builder = builder_type(builder, original_arguments, &complete_states);
    let write_output = fresh_type_ident(&impl_generics, "Output");
    let (impl_params, _, impl_where) = impl_generics.split_for_impl();
    let (_, schema_types, _) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let build_value = private_ident(schema, "write_value");

    let mut used_error_names = BTreeSet::from(["Layout".to_owned()]);
    let conversion_names = schema
        .fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Scalar(scalar) if scalar.value_type.is_converted() => {
                Some(unique_build_variant(
                    &mut used_error_names,
                    &format!("{}Value", pascal(&field.name)),
                ))
            }
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let nested_names = schema
        .fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested(_)).then(|| {
                unique_build_variant(&mut used_error_names, &pascal(&field.name).to_string())
            })
        })
        .collect::<Vec<_>>();

    let schema_start = private_ident(schema, "schema_start");
    let writes = schema
        .fields
        .iter()
        .zip(&conversion_names)
        .zip(&nested_names)
        .enumerate()
        .map(|(index, ((field, conversion_variant), nested_variant))| {
            let geometry = if schema.has_explicit_geometry() {
                render_detached_geometry(
                    schema,
                    field,
                    &build_value,
                    &schema_start,
                    error,
                    runtime,
                )
            } else {
                TokenStream::new()
            };
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let value_ty = value_type_tokens(scalar.value_type);
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    let source = if let Some(constant) = &scalar.constant {
                        quote!(#constant)
                    } else if let Some(dependent) = schema.length_dependent(&field.name) {
                        match &dependent.kind {
                            FieldKind::RawBytes(_) => {
                                let dependent_name = &dependent.name;
                                let controller_name = field.name.unraw().to_string();
                                quote! {
                                    #value_ty::try_from(
                                        #build_value.#dependent_name.0.as_ref().len()
                                    )
                                    .map_err(|_| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?
                                }
                            }
                            FieldKind::Nested(_) => quote!(0 as #value_ty),
                            FieldKind::Scalar(_) | FieldKind::Bytes(_) => unreachable!(),
                        }
                    } else {
                        let field = &field.name;
                        quote!(#build_value.#field.0)
                    };
                    if let Some(variant) = conversion_variant {
                        let semantic = private_ident(schema, &format!("scalar_{index}_value"));
                        let encoded = private_ident(schema, &format!("scalar_{index}_encoded"));
                        let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                        quote! {
                            #geometry
                            let #semantic: #value_ty = #source;
                            let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                                #runtime::WriteError::Schema(#error::#variant(
                                    #runtime::ScalarBuildConversionError {
                                        from: stringify!(#value_ty),
                                        to: stringify!(#wire_ty),
                                    },
                                ))
                            })?;
                            writer.write(&#encoded.#encode())?;
                        }
                    } else {
                        let encoded = private_ident(schema, &format!("scalar_{index}_encoded"));
                        quote! {
                            let #encoded: #wire_ty = #source;
                            #geometry
                            writer.write(&#encoded.#encode())?;
                        }
                    }
                }
                FieldKind::Bytes(bytes) => {
                    let source = if let Some(constant) = &bytes.constant {
                        quote!(#constant)
                    } else {
                        let field = &field.name;
                        quote!(#build_value.#field.0)
                    };
                    let value = private_ident(schema, &format!("bytes_{index}_value"));
                    let ty = &field.ty;
                    quote! {
                        let #value: #ty = #source;
                        #geometry
                        writer.write(&#value)?;
                    }
                }
                FieldKind::RawBytes(_) => {
                    let field_name = &field.name;
                    quote! {
                        #geometry
                        writer.write(#build_value.#field_name.0.as_ref())?;
                    }
                }
                FieldKind::Nested(nested) => {
                    let field_name = &field.name;
                    let field_label = field.name.unraw().to_string();
                    let ty = &nested.ty;
                    let (_, _, parameter) = nested_builders
                        .iter()
                        .find(|(candidate, _, _)| candidate == field_name)
                        .expect("nested field has a complete builder parameter");
                    let variant = nested_variant.as_ref().expect("nested error variant");
                    let fixed_size = private_ident(schema, &format!("nested_{index}_fixed_size"));
                    let start = private_ident(schema, &format!("nested_{index}_start"));
                    let actual_end = private_ident(schema, &format!("nested_{index}_actual_end"));
                    let expected_end =
                        private_ident(schema, &format!("nested_{index}_expected_end"));
                    let size_value = if nested.extent.is_some() {
                        quote!(None)
                    } else if nested.terminal {
                        quote!(<#ty as #runtime::WireBuilder>::FIXED_SIZE)
                    } else {
                        quote!(Some(
                            <#ty as #runtime::WireBuilder>::FIXED_SIZE.ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error::Layout(#runtime::LayoutError { field: #field_label }),
                                )
                            })?
                        ))
                    };
                    let patch = nested
                        .extent
                        .as_ref()
                        .map(|controller| {
                            let controller_field = schema
                                .fields
                                .iter()
                                .find(|candidate| candidate.name == *controller)
                                .expect("validated length controller");
                            let FieldKind::Scalar(controller_scalar) = &controller_field.kind else {
                                unreachable!("validated controller is scalar")
                            };
                            let relative = super::builder_offset(&controller_field.offset, runtime);
                            let controller_name = controller.unraw().to_string();
                            let wire_ty = scalar_type_tokens(controller_scalar.wire_type);
                            let encode = to_bytes_method(controller_scalar.endian);
                            quote! {
                                let child_length = #actual_end.checked_sub(#start).ok_or_else(|| {
                                    #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError { field: #field_label }),
                                    )
                                })?;
                                let controller_value = #wire_ty::try_from(child_length).map_err(|_| {
                                    #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    )
                                })?;
                                let controller_offset = #relative
                                    .and_then(|offset| #schema_start.checked_add(offset))
                                    .ok_or_else(|| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?;
                                writer.patch_at(controller_offset, &controller_value.#encode())?;
                            }
                        })
                        .unwrap_or_default();
                    quote! {
                        let #fixed_size = #size_value;
                        #geometry
                        let #start = writer.position();
                        <#ty as #runtime::WireWrite<#parameter>>::write(
                            #build_value.#field_name.0,
                            writer,
                        )
                        .map_err(|error| match error {
                            #runtime::WriteError::Schema(error) => {
                                #runtime::WriteError::Schema(#error::#variant(error))
                            }
                            #runtime::WriteError::Output(error) => {
                                #runtime::WriteError::Output(error)
                            }
                        })?;
                        let #actual_end = writer.position();
                        if let Some(size) = #fixed_size {
                            let #expected_end = #start.checked_add(size).ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error::Layout(#runtime::LayoutError { field: #field_label }),
                                )
                            })?;
                            if #actual_end > #expected_end {
                                return Err(#runtime::WriteError::Output(
                                    #runtime::OutputError::ChildOverflow {
                                        end: #actual_end,
                                        limit: #expected_end,
                                    },
                                ));
                            }
                            if #actual_end < #expected_end {
                                return Err(#runtime::WriteError::Output(
                                    #runtime::OutputError::ChildIncomplete {
                                        end: #actual_end,
                                        limit: #expected_end,
                                    },
                                ));
                            }
                        }
                        #patch
                    }
                }
            }
        });

    let conversion_error_variants = schema
        .fields
        .iter()
        .zip(&conversion_names)
        .filter_map(|(field, variant)| {
            let variant = variant.as_ref()?;
            let field_name = field.name.to_string();
            let message = format!("cannot write scalar field `{field_name}`: {{0}}");
            Some(quote! {
                #[doc = "The Rust scalar cannot be represented by its declared wire type."]
                #[error(#message)]
                #variant(#[source] #runtime::ScalarBuildConversionError),
            })
        })
        .collect::<Vec<_>>();
    let nested_error_parameters = nested_builders
        .iter()
        .enumerate()
        .map(|(index, _)| format_ident!("__WireReprError{index}"))
        .collect::<Vec<_>>();
    let mut nested_index = 0usize;
    let nested_error_variants = schema
        .fields
        .iter()
        .zip(&nested_names)
        .filter_map(|(field, variant)| {
            let variant = variant.as_ref()?;
            let parameter = &nested_error_parameters[nested_index];
            nested_index += 1;
            let field_name = field.name.to_string();
            let message = format!("failed to write nested field `{field_name}`: {{0}}");
            Some(quote! {
                #[doc = "The nested schema failed writing."]
                #[error(#message)]
                #variant(#[source] #parameter),
            })
        })
        .collect::<Vec<_>>();
    let layout_error_variant = schema.layout_can_fail().then(|| {
        quote! {
            #[doc = "A static field offset or fixed width could not be established."]
            #[error("cannot write schema layout: {0}")]
            Layout(#[source] #runtime::LayoutError),
        }
    });
    let has_errors = schema.layout_can_fail()
        || !conversion_error_variants.is_empty()
        || !nested_builders.is_empty();
    let error_declaration = if !has_errors {
        TokenStream::new()
    } else if nested_builders.is_empty() {
        quote! {
            #[doc = "Typed write failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error {
                #(#conversion_error_variants)*
                #layout_error_variant
            }
        }
    } else {
        quote! {
            #[doc = "Typed write failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<#(#nested_error_parameters: ::core::error::Error + 'static),*> {
                #(#conversion_error_variants)*
                #layout_error_variant
                #(#nested_error_variants)*
            }
        }
    };
    let nested_error_types = nested_builders
        .iter()
        .map(|(_, ty, parameter)| quote!(<#ty as #runtime::WireWrite<#parameter>>::Error));
    let error_type = if nested_builders.is_empty() {
        if has_errors {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#nested_error_types),*>)
    };

    quote! {
        #error_declaration

        impl #impl_params #runtime::WireWrite<#complete_builder> for #self_type #impl_where {
            type Error = #error_type;

            #[inline]
            fn write<#write_output: #runtime::Output>(
                #build_value: #complete_builder,
                writer: &mut #runtime::ChildWriter<'_, #write_output>,
            ) -> Result<
                (),
                #runtime::WriteError<Self::Error, #write_output::GrowError>,
            > {
                let #schema_start = writer.position();
                #(#writes)*
                Ok(())
            }
        }
    }
}
fn render_detached_geometry(
    _schema: &Schema,
    field: &super::model::Field,
    build_value: &syn::Ident,
    schema_start: &syn::Ident,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let field_name = field.name.unraw().to_string();
    let position = match &field.layout.position {
        Some(Position::Static(position)) => quote! {
            let relative_position: usize = #position;
            let field_position = #schema_start.checked_add(relative_position).ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                )
            })?;
        },
        Some(Position::Field(controller)) => quote! {
            let relative_position = usize::try_from(#build_value.#controller.0).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                )
            })?;
            let field_position = #schema_start.checked_add(relative_position).ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                )
            })?;
        },
        None => {
            let pad = field
                .layout
                .pad_before
                .as_ref()
                .map(|pad| {
                    quote! {
                        relative_position =
                            relative_position.checked_add(#pad).ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                                )
                            })?;
                    }
                })
                .unwrap_or_default();
            let align = field
                .layout
                .align_before
                .as_ref()
                .map(|align| {
                    quote! {
                        relative_position = #runtime::__private::checked_align(
                            relative_position,
                            #align,
                        )
                        .ok_or_else(|| #runtime::WriteError::Schema(
                            #error::Layout(#runtime::LayoutError { field: #field_name }),
                        ))?;
                    }
                })
                .unwrap_or_default();
            quote! {
                let mut relative_position =
                    writer.position().checked_sub(#schema_start).ok_or_else(|| {
                        #runtime::WriteError::Schema(
                            #error::Layout(#runtime::LayoutError { field: #field_name }),
                        )
                    })?;
                #pad
                #align
                let field_position = #schema_start.checked_add(relative_position).ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    )
                })?;
            }
        }
    };
    quote! {
        #position
        writer.fill_to(field_position)?;
    }
}

fn generic_arguments(generics: &Generics) -> Vec<TokenStream> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => {
                let lifetime = &parameter.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
            GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
        })
        .collect()
}

fn builder_type(
    builder: &syn::Ident,
    original_arguments: &[TokenStream],
    states: &[TokenStream],
) -> TokenStream {
    let arguments = original_arguments
        .iter()
        .chain(states.iter())
        .collect::<Vec<_>>();
    if arguments.is_empty() {
        quote!(#builder)
    } else {
        quote!(#builder<#(#arguments),*>)
    }
}
pub(super) fn unique_build_variant(used: &mut BTreeSet<String>, base: &str) -> syn::Ident {
    if used.insert(base.to_owned()) {
        return format_ident!("{base}");
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}{suffix}");
        if used.insert(candidate.clone()) {
            return format_ident!("{candidate}");
        }
    }
    unreachable!("usize suffix space cannot be exhausted by generated variants")
}

pub(super) fn convert_to_wire(
    scalar: &Scalar,
    value: &syn::Ident,
    wire_type: &TokenStream,
) -> TokenStream {
    match scalar.value_type {
        ValueType::Scalar(_) => quote!(Some(#value)),
        ValueType::Usize | ValueType::Isize => {
            quote!(<#wire_type>::try_from(#value).ok())
        }
        ValueType::Bool => quote!(Some(if #value {
            1 as #wire_type
        } else {
            0 as #wire_type
        })),
        ValueType::Char => quote!(<#wire_type>::try_from(u32::from(#value)).ok()),
    }
}
