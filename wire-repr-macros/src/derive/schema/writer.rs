use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, TypeParam, parse_quote};

use super::builder::{Slot, SlotKind, convert_to_wire, slots, unique_build_variant};
use super::model::{FieldKind, LayoutOffset, Schema, SizeTerm};
use super::{
    builder_offset, builder_optional_size, fresh_field_ident, fresh_type_ident, pascal,
    private_ident, scalar_type_tokens, to_bytes_method, value_type_tokens,
};

pub(super) fn render(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    let vis = &schema.vis;
    let name = &schema.name;
    let writer_name = format_ident!("{}Writer", name.unraw());
    let slots = slots(schema);
    let marker = fresh_field_ident(schema, "writer_schema");
    let output = fresh_type_ident(&schema.generics, "Output");
    let original_arguments = generic_arguments(&schema.generics);
    let state_markers = slots.iter().map(|slot| &slot.state).collect::<Vec<_>>();

    let mut writer_generics = schema.generics.clone();
    for parameter in &mut writer_generics.params {
        match parameter {
            GenericParam::Type(parameter) => parameter.default = None,
            GenericParam::Const(parameter) => parameter.default = None,
            GenericParam::Lifetime(_) => {}
        }
    }
    let mut output_parameter = TypeParam::from(output.clone());
    output_parameter.bounds.push(parse_quote!(#runtime::Output));
    writer_generics
        .params
        .push(GenericParam::Type(output_parameter));
    for slot in &slots {
        let mut parameter = TypeParam::from(slot.state.clone());
        parameter.default = Some(parse_quote!(#runtime::__private::Unset));
        writer_generics.params.push(GenericParam::Type(parameter));
    }
    let writer_declaration_generics = writer_generics.clone();
    let initial_states = slots
        .iter()
        .map(|_| quote!(#runtime::__private::Unset))
        .collect::<Vec<_>>();
    let initial_writer = writer_type(&writer_name, &original_arguments, &output, &initial_states);

    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let setters = render_setters(
        schema,
        &writer_name,
        &marker,
        &output,
        &original_arguments,
        &slots,
        runtime,
    );
    let finish = render_finish(
        schema,
        &writer_name,
        &output,
        &original_arguments,
        &slots,
        runtime,
    );

    quote! {
        #[doc(hidden)]
        #vis struct #writer_name #writer_declaration_generics {
            writer: #runtime::Writer<#output>,
            #marker: ::core::marker::PhantomData<
                fn() -> (#self_type, #(#state_markers,)*)
            >,
        }

        impl #schema_impl #self_type #schema_where {
            /// Creates a progressive typestate writer over `output`.
            #[inline(always)]
            #vis fn builder<#output: #runtime::Output>(output: #output) -> #initial_writer {
                #writer_name {
                    writer: #runtime::Writer::new(output),
                    #marker: ::core::marker::PhantomData,
                }
            }
        }

        #(#setters)*
        #finish
    }
}

fn render_setters(
    schema: &Schema,
    writer_name: &syn::Ident,
    marker: &syn::Ident,
    output: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    let vis = &schema.vis;
    let error_name = format_ident!("{}WriteError", schema.name.unraw());
    let (conversion_names, nested_names) = writer_error_names(schema);
    let has_conversions = conversion_names.iter().any(Option::is_some);
    let mut rendered = Vec::new();

    for (target_index, target) in slots.iter().enumerate() {
        let physical_index = schema
            .fields
            .iter()
            .position(|field| field.name == target.field)
            .expect("writer slot has one physical field");
        let physical = &schema.fields[physical_index];
        let mut impl_generics = schema.generics.clone();
        let mut output_parameter = TypeParam::from(output.clone());
        output_parameter.bounds.push(parse_quote!(#runtime::Output));
        impl_generics
            .params
            .push(GenericParam::Type(output_parameter));
        add_offset_bounds(&mut impl_generics, &physical.offset, runtime);

        let build_fn = fresh_type_ident(&impl_generics, "BuildFn");
        let child_builder = fresh_type_ident(&impl_generics, "ChildBuilder");
        let mut current_states = Vec::with_capacity(slots.len());
        let mut returned_states = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            if index == target_index {
                current_states.push(quote!(#runtime::__private::Unset));
                returned_states.push(match &target.kind {
                    SlotKind::Value(ty) => quote!(#runtime::__private::Set<#ty>),
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
        let current_writer = writer_type(writer_name, original_arguments, output, &current_states);
        let returned_writer =
            writer_type(writer_name, original_arguments, output, &returned_states);
        let field = &target.field;
        let offset = builder_offset(&physical.offset, runtime);
        let offset_local = private_ident(schema, &format!("{}_offset", field.unraw()));
        let value = private_ident(schema, &format!("{}_value", field.unraw()));
        let field_name = field.unraw().to_string();
        let layout_failure = quote! {
            #runtime::WriteError::Schema(
                #error_name::Layout(#runtime::LayoutError { field: #field_name }),
            )
        };
        let offset_binding = if schema.layout_can_fail() {
            quote!(let #offset_local = #offset.ok_or_else(|| #layout_failure)?;)
        } else {
            quote!(
                let #offset_local = #offset.expect("validated fixed layout offset");
            )
        };
        let assignments = quote! {
            #writer_name {
                writer: self.writer,
                #marker: ::core::marker::PhantomData,
            }
        };

        match (&target.kind, &physical.kind) {
            (SlotKind::Value(ty), FieldKind::Scalar(scalar)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let wire_ty = scalar_type_tokens(scalar.wire_type);
                let encode = to_bytes_method(scalar.endian);
                let encoded = private_ident(schema, &format!("{}_encoded", field.unraw()));
                let write = if scalar.value_type.is_converted() {
                    let conversion = convert_to_wire(scalar, &value, &wire_ty);
                    let variant = conversion_names[physical_index]
                        .as_ref()
                        .expect("converted field has one error variant");
                    quote! {
                        let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                            #runtime::WriteError::Schema(
                                #error_name::#variant(#runtime::ScalarBuildConversionError {
                                    from: stringify!(#ty),
                                    to: stringify!(#wire_ty),
                                }),
                            )
                        })?;
                        self.writer.write_at(#offset_local, &#encoded.#encode())?;
                    }
                } else {
                    quote! {
                        let #encoded: #wire_ty = #value;
                        self.writer.write_at(#offset_local, &#encoded.#encode())?;
                    }
                };
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field(
                            mut self,
                            #value: #ty,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        > {
                            #offset_binding
                            #write
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Value(ty), FieldKind::Bytes(_)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes fixed byte-array field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field(
                            mut self,
                            #value: #ty,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        > {
                            #offset_binding
                            self.writer.write_at(#offset_local, &#value)?;
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Nested(ty), FieldKind::Nested(nested)) => {
                let child_error = quote!(<#ty as #runtime::WireWrite<#child_builder>>::Error);
                let schema_error = writer_error_type(
                    schema,
                    &error_name,
                    has_conversions,
                    Some((field, child_error)),
                    runtime,
                );
                let variant = nested_names[physical_index]
                    .as_ref()
                    .expect("nested field has one error variant");
                let child_writer = private_ident(schema, &format!("{}_writer", field.unraw()));
                let create_child = if nested.terminal {
                    quote! {
                        match <#ty as #runtime::WireBuilder>::FIXED_SIZE {
                            Some(size) => self.writer.fixed_child_at(#offset_local, size)?,
                            None => self.writer.child_at(#offset_local)?,
                        }
                    }
                } else {
                    quote! {{
                        let size = <#ty as #runtime::WireBuilder>::FIXED_SIZE
                            .ok_or_else(|| #layout_failure)?;
                        self.writer.fixed_child_at(#offset_local, size)?
                    }}
                };
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes nested field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field<#build_fn, #child_builder>(
                            mut self,
                            build: #build_fn,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        >
                        where
                            #ty: #runtime::WireBuilder + #runtime::WireWrite<#child_builder>,
                            #build_fn: FnOnce(
                                <#ty as #runtime::WireBuilder>::Builder,
                            ) -> #child_builder,
                        {
                            #offset_binding
                            {
                                let mut #child_writer = #create_child;
                                let value = build(<#ty as #runtime::WireBuilder>::builder());
                                <#ty as #runtime::WireWrite<#child_builder>>::write(
                                    value,
                                    &mut #child_writer,
                                )
                                .map_err(|error| match error {
                                    #runtime::WriteError::Schema(error) => {
                                        #runtime::WriteError::Schema(
                                            #error_name::#variant(error),
                                        )
                                    }
                                    #runtime::WriteError::Output(error) => {
                                        #runtime::WriteError::Output(error)
                                    }
                                })?;
                                #child_writer.finish()?;
                            }
                            Ok(#assignments)
                        }
                    }
                });
            }
            _ => unreachable!("slot kind must match its physical field"),
        }
    }
    rendered
}

fn render_finish(
    schema: &Schema,
    writer_name: &syn::Ident,
    output: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let error_name = format_ident!("{}WriteError", schema.name.unraw());
    let (conversion_names, _) = writer_error_names(schema);
    let has_conversions = conversion_names.iter().any(Option::is_some);
    let mut impl_generics = schema.generics.clone();
    let mut output_parameter = TypeParam::from(output.clone());
    output_parameter.bounds.push(parse_quote!(#runtime::Output));
    impl_generics
        .params
        .push(GenericParam::Type(output_parameter));

    let mut complete_states = Vec::with_capacity(slots.len());
    let mut child_errors = Vec::new();
    for slot in slots {
        match &slot.kind {
            SlotKind::Value(ty) => {
                complete_states.push(quote!(#runtime::__private::Set<#ty>));
            }
            SlotKind::Nested(ty) => {
                let child =
                    fresh_type_ident(&impl_generics, &format!("{}Builder", pascal(&slot.field)));
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(child.clone())));
                impl_generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: #runtime::WireWrite<#child>));
                complete_states.push(quote!(#runtime::__private::Set<#child>));
                child_errors.push((
                    slot.field.clone(),
                    quote!(<#ty as #runtime::WireWrite<#child>>::Error),
                ));
            }
        }
    }
    for field in &schema.fields {
        add_offset_bounds(&mut impl_generics, &field.offset, runtime);
    }
    let complete_writer = writer_type(writer_name, original_arguments, output, &complete_states);
    let schema_error =
        writer_error_type_from_all(schema, &error_name, has_conversions, &child_errors, runtime);

    let constant_writes = schema
        .fields
        .iter()
        .zip(&conversion_names)
        .enumerate()
        .filter_map(|(index, (field, conversion_variant))| {
            let constant = field.kind.constant()?;
            let offset = builder_offset(&field.offset, runtime);
            let offset_local = private_ident(schema, &format!("constant_{index}_offset"));
            let field_name = field.name.unraw().to_string();
            let layout_failure = quote! {
                #runtime::WriteError::Schema(
                    #error_name::Layout(#runtime::LayoutError { field: #field_name }),
                )
            };
            let offset_binding = if schema.layout_can_fail() {
                quote!(let #offset_local = #offset.ok_or_else(|| #layout_failure)?;)
            } else {
                quote!(
                    let #offset_local = #offset.expect("validated fixed layout offset");
                )
            };
            Some(match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let value_ty = value_type_tokens(scalar.value_type);
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    if let Some(variant) = conversion_variant {
                        let semantic = private_ident(schema, &format!("constant_{index}_semantic"));
                        let encoded = private_ident(schema, &format!("constant_{index}_encoded"));
                        let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                        quote! {
                            #offset_binding
                            let #semantic: #value_ty = #constant;
                            let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error_name::#variant(
                                        #runtime::ScalarBuildConversionError {
                                            from: stringify!(#value_ty),
                                            to: stringify!(#wire_ty),
                                        },
                                    ),
                                )
                            })?;
                            self.writer.write_at(#offset_local, &#encoded.#encode())?;
                        }
                    } else {
                        quote! {
                            #offset_binding
                            let encoded: #wire_ty = #constant;
                            self.writer.write_at(#offset_local, &encoded.#encode())?;
                        }
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    let value = private_ident(schema, &format!("constant_{index}_bytes"));
                    quote! {
                        #offset_binding
                        let #value: #ty = #constant;
                        self.writer.write_at(#offset_local, &#value)?;
                    }
                }
                FieldKind::Nested(_) => unreachable!(),
            })
        });
    let ensure_total = if schema
        .fields
        .last()
        .is_some_and(|field| !matches!(field.kind, FieldKind::Nested(_)))
    {
        let total = builder_optional_size(schema, runtime);
        if schema.layout_can_fail() {
            quote! {
                let total = #total.ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error_name::Layout(#runtime::LayoutError { field: "<finish>" }),
                    )
                })?;
                self.writer.ensure(total, total)?;
            }
        } else {
            quote! {
                let total = #total.expect("validated fixed layout width");
                self.writer.ensure(total, total)?;
            }
        }
    } else {
        TokenStream::new()
    };
    let (impl_params, _, impl_where) = impl_generics.split_for_impl();

    quote! {
        impl #impl_params #complete_writer #impl_where {
            /// Finishes the representation and returns its exact output range.
            #[inline]
            #vis fn finish(
                mut self,
            ) -> Result<
                #runtime::Written<#output>,
                #runtime::WriteError<
                    #schema_error,
                    <#output as #runtime::Output>::GrowError,
                >,
            > {
                #ensure_total
                #(#constant_writes)*
                Ok(self.writer.finish())
            }
        }
    }
}

fn writer_error_names(schema: &Schema) -> (Vec<Option<syn::Ident>>, Vec<Option<syn::Ident>>) {
    let mut used = BTreeSet::from(["Layout".to_owned()]);
    let conversions = schema
        .fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Scalar(scalar) if scalar.value_type.is_converted() => Some(
                unique_build_variant(&mut used, &format!("{}Value", pascal(&field.name))),
            ),
            FieldKind::Scalar(_) | FieldKind::Bytes(_) | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let nested = schema
        .fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested(_))
                .then(|| unique_build_variant(&mut used, &pascal(&field.name).to_string()))
        })
        .collect();
    (conversions, nested)
}

fn writer_error_type(
    schema: &Schema,
    error: &syn::Ident,
    has_conversions: bool,
    target: Option<(&syn::Ident, TokenStream)>,
    _runtime: &TokenStream,
) -> TokenStream {
    let child_errors = schema
        .nested_fields()
        .map(|field| {
            if target
                .as_ref()
                .is_some_and(|(name, _)| **name == field.name)
            {
                target.as_ref().expect("checked target").1.clone()
            } else {
                quote!(::core::convert::Infallible)
            }
        })
        .collect::<Vec<_>>();
    if child_errors.is_empty() {
        if has_conversions || schema.layout_can_fail() {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#child_errors),*>)
    }
}

fn writer_error_type_from_all(
    schema: &Schema,
    error: &syn::Ident,
    has_conversions: bool,
    child_errors: &[(syn::Ident, TokenStream)],
    _runtime: &TokenStream,
) -> TokenStream {
    let errors = schema
        .nested_fields()
        .map(|field| {
            child_errors
                .iter()
                .find(|(name, _)| *name == field.name)
                .expect("complete writer has every nested error")
                .1
                .clone()
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        if has_conversions || schema.layout_can_fail() {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#errors),*>)
    }
}

fn add_offset_bounds(generics: &mut syn::Generics, offset: &LayoutOffset, runtime: &TokenStream) {
    for term in &offset.terms {
        if let SizeTerm::Nested(ty) = term {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #runtime::WireBuilder));
        }
    }
}
fn writer_type(
    writer: &syn::Ident,
    original_arguments: &[TokenStream],
    output: &syn::Ident,
    states: &[TokenStream],
) -> TokenStream {
    let mut arguments = original_arguments.to_vec();
    arguments.push(quote!(#output));
    arguments.extend(states.iter().cloned());
    quote!(#writer<#(#arguments),*>)
}

fn generic_arguments(generics: &syn::Generics) -> Vec<TokenStream> {
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
