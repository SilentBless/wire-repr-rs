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
    let builder_declaration_where = &builder_declaration_generics.where_clause;
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
    let choice_groups = render_choice_groups(schema, runtime);

    let fixed_size = if schema.has_explicit_geometry() {
        quote!(None)
    } else {
        builder_optional_size(schema, runtime)
    };
    Ok(quote! {
        #choice_groups
        const _: () = {
            #(#validator_references)*
        };

        #[doc(hidden)]
        #vis struct #builder #builder_declaration_generics #builder_declaration_where {
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
    Array(Box<syn::Type>),
    Choice(syn::Ident),
    Nested(Box<syn::Type>),
}

pub(super) fn slots(schema: &Schema) -> Vec<Slot> {
    let mut used = schema.generics.clone();
    let mut slots = Vec::new();
    for field in &schema.fields {
        if field.kind.constant().is_some()
            || field.kind.computed().is_some()
            || schema.is_length_controller(&field.name)
            || schema.is_count_controller(&field.name)
            || schema.is_presence_controller(&field.name)
            || schema.is_bit_controller(&field.name)
            || field.layout.condition.is_some()
        {
            continue;
        }
        let state = fresh_type_ident(&used, &format!("{}State", pascal(&field.name)));
        used.params
            .push(GenericParam::Type(TypeParam::from(state.clone())));
        let kind = match &field.kind {
            FieldKind::Scalar(scalar) => SlotKind::Value(value_type_tokens(&scalar.value_type)),
            FieldKind::Bytes(_) => {
                let ty = &field.ty;
                SlotKind::Value(quote!(#ty))
            }
            FieldKind::ScalarArray(_) => {
                let ty = &field.ty;
                SlotKind::Value(quote!(#ty))
            }
            FieldKind::RawBytes(_) => SlotKind::RawBytes,
            FieldKind::Array(array) => SlotKind::Array(Box::new(array.item.clone())),
            FieldKind::Flag(_) => SlotKind::Choice(field.name.clone()),
            FieldKind::BitProjection(_) => {
                let ty = &field.ty;
                SlotKind::Value(quote!(#ty))
            }
            FieldKind::Nested(nested) => SlotKind::Nested(Box::new(nested.ty.clone())),
            FieldKind::Recursive(_) => {
                unreachable!("recursive builder is rejected before rendering")
            }
        };
        slots.push(Slot {
            field: field.name.clone(),
            state,
            kind,
        });
    }
    slots
}
pub(super) fn choice_trait_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = schema
        .fields
        .iter()
        .position(|field| field.name == *flag)
        .expect("flag belongs to schema");
    format_ident!("__WireRepr{}Choice{index}Value", schema.name.unraw())
}

pub(super) fn choice_start_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = schema
        .fields
        .iter()
        .position(|field| field.name == *flag)
        .expect("flag belongs to schema");
    format_ident!("__WireRepr{}Choice{index}", schema.name.unraw())
}

fn choice_state_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = schema
        .fields
        .iter()
        .position(|field| field.name == *flag)
        .expect("flag belongs to schema");
    format_ident!("__WireRepr{}Choice{index}State", schema.name.unraw())
}

pub(super) fn choice_final_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = schema
        .fields
        .iter()
        .position(|field| field.name == *flag)
        .expect("flag belongs to schema");
    format_ident!("__WireRepr{}Choice{index}Final", schema.name.unraw())
}

fn render_choice_groups(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    let vis = &schema.vis;
    let groups = schema.flag_fields().map(|flag| {
        let name = &flag.name;
        let start = choice_start_ident(schema, name);
        let state = choice_state_ident(schema, name);
        let final_value = choice_final_ident(schema, name);
        let choice_trait = choice_trait_ident(schema, name);
        let present_field = private_ident(schema, &format!("choice_{}_present", name.unraw()));
        let dependents = schema.condition_dependents(name).collect::<Vec<_>>();
        let parameters = dependents
            .iter()
            .enumerate()
            .map(|(index, _)| format_ident!("__WireReprField{index}"))
            .collect::<Vec<_>>();
        let initial_states = dependents
            .iter()
            .map(|_| quote!(#runtime::__private::Unset))
            .collect::<Vec<_>>();
        let complete_states = dependents
            .iter()
            .map(|field| {
                let FieldKind::Scalar(scalar) = &field.kind else {
                    unreachable!("validated conditional field is scalar")
                };
                let ty = value_type_tokens(&scalar.value_type);
                quote!(#runtime::__private::Set<#ty>)
            })
            .collect::<Vec<_>>();
        let state_fields = dependents
            .iter()
            .zip(&parameters)
            .map(|(field, parameter)| {
                let field = &field.name;
                quote!(#field: #parameter,)
            });
        let final_fields = dependents.iter().map(|field| {
            let field_name = &field.name;
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("validated conditional field is scalar")
            };
            let ty = value_type_tokens(&scalar.value_type);
            quote!(#field_name: Option<#ty>,)
        });
        let initial_values = dependents.iter().map(|field| {
            let field = &field.name;
            quote!(#field: #runtime::__private::Unset,)
        });
        let absent_values = dependents.iter().map(|field| {
            let field = &field.name;
            quote!(#field: None,)
        });
        let present_values = dependents.iter().map(|field| {
            let field = &field.name;
            quote!(#field: Some(value.#field.0),)
        });
        let setters = dependents.iter().enumerate().map(|(target_index, target)| {
            let field = &target.name;
            let FieldKind::Scalar(scalar) = &target.kind else {
                unreachable!("validated conditional field is scalar")
            };
            let ty = value_type_tokens(&scalar.value_type);
            let impl_parameters = parameters
                .iter()
                .enumerate()
                .filter_map(|(index, parameter)| (index != target_index).then_some(parameter))
                .collect::<Vec<_>>();
            let impl_generics = if impl_parameters.is_empty() {
                TokenStream::new()
            } else {
                quote!(<#(#impl_parameters),*>)
            };
            let current = parameters.iter().enumerate().map(|(index, parameter)| {
                if index == target_index {
                    quote!(#runtime::__private::Unset)
                } else {
                    quote!(#parameter)
                }
            });
            let returned = parameters.iter().enumerate().map(|(index, parameter)| {
                if index == target_index {
                    quote!(#runtime::__private::Set<#ty>)
                } else {
                    quote!(#parameter)
                }
            });
            let assignments = dependents.iter().enumerate().map(|(index, dependent)| {
                let dependent = &dependent.name;
                if index == target_index {
                    quote!(#dependent: #runtime::__private::Set(value),)
                } else {
                    quote!(#dependent: self.#dependent,)
                }
            });
            quote! {
                impl #impl_generics #state<#(#current),*> {
                    #[doc = concat!("Sets conditional field `", stringify!(#field), "`.")]
                    #[inline(always)]
                    #vis fn #field(self, value: #ty) -> #state<#(#returned),*> {
                        #state {
                            #(#assignments)*
                        }
                    }
                }
            }
        });
        let writes = dependents.iter().map(|field| {
            let field_name = &field.name;
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("validated conditional field is scalar")
            };
            let wire_ty = scalar_type_tokens(scalar.wire_type);
            let encode = to_bytes_method(scalar.endian);
            quote! {
                let value: #wire_ty =
                    self.#field_name.expect("present choice has every field");
                writer.write(&value.#encode())?;
            }
        });
        quote! {
            #[doc(hidden)]
            #vis struct #start;

            #[doc(hidden)]
            #vis struct #state<#(#parameters),*> {
                #(#state_fields)*
            }

            #[doc(hidden)]
            #vis struct #final_value {
                #present_field: bool,
                #(#final_fields)*
            }

            #[doc(hidden)]
            #vis trait #choice_trait {
                fn is_present(&self) -> bool;
                fn write<O: #runtime::Output>(
                    self,
                    writer: &mut #runtime::ChildWriter<'_, O>,
                ) -> Result<(), #runtime::OutputError<O::GrowError>>;
            }

            impl #start {
                #[inline(always)]
                #vis fn absent(self) -> #final_value {
                    #final_value {
                        #present_field: false,
                        #(#absent_values)*
                    }
                }

                #[inline(always)]
                #vis fn present<__WireReprBuild>(self, build: __WireReprBuild) -> #final_value
                where
                    __WireReprBuild: FnOnce(
                        #state<#(#initial_states),*>
                    ) -> #state<#(#complete_states),*>,
                {
                    let value = build(#state {
                        #(#initial_values)*
                    });
                    #final_value {
                        #present_field: true,
                        #(#present_values)*
                    }
                }
            }

            #(#setters)*

            impl #choice_trait for #final_value {
                #[inline(always)]
                fn is_present(&self) -> bool {
                    self.#present_field
                }

                #[inline]
                fn write<O: #runtime::Output>(
                    self,
                    writer: &mut #runtime::ChildWriter<'_, O>,
                ) -> Result<(), #runtime::OutputError<O::GrowError>> {
                    if self.#present_field {
                        #(#writes)*
                    }
                    Ok(())
                }
            }
        }
    });
    quote!(#(#groups)*)
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
                    SlotKind::Array(_) => quote!(#runtime::__private::Set<()>),
                    SlotKind::Choice(flag) => {
                        let final_value = choice_final_ident(schema, flag);
                        quote!(#runtime::__private::Set<#final_value>)
                    }
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
            SlotKind::Array(_) => {}
            SlotKind::Choice(flag) => {
                let start = choice_start_ident(schema, flag);
                let final_value = choice_final_ident(schema, flag);
                rendered.push(quote! {
                    impl #impl_params #current_builder #impl_where {
                        #[doc = concat!("Chooses conditional group `", stringify!(#field), "`.")]
                        #[inline(always)]
                        #vis fn #field<#build_fn>(
                            self,
                            choose: #build_fn,
                        ) -> #returned_builder
                        where
                            #build_fn: FnOnce(#start) -> #final_value,
                        {
                            let value = choose(#start);
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
    let mut choice_values = Vec::new();
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
            SlotKind::Array(_) => {
                complete_states.push(quote!(#runtime::__private::Set<()>));
            }
            SlotKind::Choice(flag) => {
                let final_value = choice_final_ident(schema, flag);
                let choice_trait = choice_trait_ident(schema, flag);
                complete_states.push(quote!(#runtime::__private::Set<#final_value>));
                choice_values.push((flag.clone(), final_value, choice_trait));
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
    let (_, schema_types, _) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    if schema
        .fields
        .iter()
        .any(|field| field.kind.computed().is_some())
    {
        impl_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#self_type: #runtime::__private::WireSelect));
    }
    let (impl_params, _, impl_where) = impl_generics.split_for_impl();
    let build_value = private_ident(schema, "write_value");

    let mut used_error_names = BTreeSet::from(["Layout".to_owned(), "LengthConflict".to_owned()]);
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
            | FieldKind::ScalarArray(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let nested_names = schema
        .fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_)).then(|| {
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
            if let Some(condition) = &field.layout.condition {
                let first = schema
                    .condition_dependents(condition)
                    .next()
                    .expect("validated conditional group has fields");
                if first.name != field.name {
                    return TokenStream::new();
                }
                let (_, parameter, choice_trait) = choice_values
                    .iter()
                    .find(|(flag, _, _)| flag == condition)
                    .expect("conditional group has one choice value");
                return quote! {
                    <#parameter as #choice_trait>::write(
                        #build_value.#condition.0,
                        writer,
                    )?;
                };
            }
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
                    let value_ty = value_type_tokens(&scalar.value_type);
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    let source = if let Some(constant) = &scalar.constant {
                        quote!(#constant)
                    } else if scalar.computed.is_some()
                        || schema.is_count_controller(&field.name)
                    {
                        quote!(0 as #value_ty)
                    } else if schema.is_presence_controller(&field.name) {
                        let flag = schema
                            .flag_fields()
                            .find(|flag| {
                                matches!(
                                    &flag.kind,
                                    FieldKind::Flag(flag) if flag.controller == field.name
                                )
                            })
                            .expect("presence controller has one flag");
                        let (_, parameter, choice_trait) = choice_values
                            .iter()
                            .find(|(name, _, _)| name == &flag.name)
                            .expect("flag has one choice value");
                        let flag_name = &flag.name;
                        quote!(
                            <#parameter as #choice_trait>::is_present(
                                &#build_value.#flag_name.0
                            )
                        )
                    } else if schema.is_bit_controller(&field.name) {
                        render_bit_controller_source(
                            schema,
                            &field.name,
                            &build_value,
                            &value_ty,
                            error,
                            runtime,
                        )
                    } else {
                        let dependents = schema.length_dependents(&field.name).collect::<Vec<_>>();
                        if dependents.is_empty() {
                            let field = &field.name;
                            quote!(#build_value.#field.0)
                        } else if matches!(dependents[0].kind, FieldKind::RawBytes(_)) {
                            let controller_name = field.name.unraw().to_string();
                            let first = &dependents[0].name;
                            let checks = dependents.iter().skip(1).filter_map(|dependent| {
                                if !matches!(dependent.kind, FieldKind::RawBytes(_)) {
                                    return None;
                                }
                                let dependent_name = &dependent.name;
                                Some(quote! {
                                    let actual = #build_value.#dependent_name.0.as_ref().len();
                                    if actual != expected {
                                        return Err(#runtime::WriteError::Schema(
                                            #error::LengthConflict {
                                                controller: #controller_name,
                                                expected,
                                                actual,
                                            },
                                        ));
                                    }
                                })
                            });
                            quote! {{
                                let expected = #build_value.#first.0.as_ref().len();
                                #(#checks)*
                                #value_ty::try_from(expected).map_err(|_| {
                                    #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    )
                                })?
                            }}
                        } else {
                            quote!(0 as #value_ty)
                        }
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
                FieldKind::ScalarArray(array) => {
                    let ty = &field.ty;
                    let field = &field.name;
                    let element = scalar_type_tokens(array.element);
                    let encode = to_bytes_method(array.endian);
                    quote! {
                        let value: #ty = #build_value.#field.0;
                        #geometry
                        for element in value {
                            let element: #element = element;
                            writer.write(&element.#encode())?;
                        }
                    }
                }
                FieldKind::RawBytes(raw) => {
                    let field_name = &field.name;
                    let consistency = match &raw.extent {
                        super::model::DynamicExtent::Bounded(controller)
                            if schema
                                .length_dependents(controller)
                                .next()
                                .is_none_or(|dependent| dependent.name != field.name) =>
                        {
                            let controller_field = schema
                                .fields
                                .iter()
                                .find(|candidate| candidate.name == *controller)
                                .expect("validated length controller");
                            let FieldKind::Scalar(controller_scalar) = &controller_field.kind else {
                                unreachable!("validated length controller is scalar")
                            };
                            let relative =
                                super::builder_offset(&controller_field.offset, runtime);
                            let controller_name = controller.unraw().to_string();
                            let wire_ty = scalar_type_tokens(controller_scalar.wire_type);
                            let decode = super::from_bytes_method(controller_scalar.endian);
                            let width = controller_scalar.width();
                            quote! {
                                let controller_offset = #relative
                                    .and_then(|offset| #schema_start.checked_add(offset))
                                    .ok_or_else(|| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?;
                                let controller_bytes = writer
                                    .read_at::<#width>(controller_offset)
                                    .ok_or_else(|| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?;
                                let expected = usize::try_from(
                                    #wire_ty::#decode(controller_bytes)
                                )
                                .map_err(|_| #runtime::WriteError::Schema(
                                    #error::Layout(#runtime::LayoutError {
                                        field: #controller_name,
                                    }),
                                ))?;
                                let actual = #build_value.#field_name.0.as_ref().len();
                                if actual != expected {
                                    return Err(#runtime::WriteError::Schema(
                                        #error::LengthConflict {
                                            controller: #controller_name,
                                            expected,
                                            actual,
                                        },
                                    ));
                                }
                            }
                        }
                        super::model::DynamicExtent::Bounded(_)
                        | super::model::DynamicExtent::Rest => TokenStream::new(),
                    };
                    quote! {
                        #geometry
                        #consistency
                        writer.write(#build_value.#field_name.0.as_ref())?;
                    }
                }
                FieldKind::Array(_) => TokenStream::new(),
                FieldKind::BitProjection(_) => TokenStream::new(),
                FieldKind::Flag(_) => TokenStream::new(),
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
                            let decode = super::from_bytes_method(controller_scalar.endian);
                            let width = controller_scalar.width();
                            let is_first = schema
                                .length_dependents(controller)
                                .next()
                                .is_some_and(|dependent| dependent.name == field.name);
                            let apply = if is_first {
                                quote! {
                                    let controller_value =
                                        #wire_ty::try_from(child_length).map_err(|_| {
                                            #runtime::WriteError::Schema(
                                                #error::Layout(#runtime::LayoutError {
                                                    field: #controller_name,
                                                }),
                                            )
                                        })?;
                                    writer.patch_at(
                                        controller_offset,
                                        &controller_value.#encode(),
                                    )?;
                                }
                            } else {
                                quote! {
                                    let controller_bytes = writer
                                        .read_at::<#width>(controller_offset)
                                        .ok_or_else(|| #runtime::WriteError::Schema(
                                            #error::Layout(#runtime::LayoutError {
                                                field: #controller_name,
                                            }),
                                        ))?;
                                    let expected = usize::try_from(
                                        #wire_ty::#decode(controller_bytes)
                                    )
                                    .map_err(|_| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?;
                                    if child_length != expected {
                                        return Err(#runtime::WriteError::Schema(
                                            #error::LengthConflict {
                                                controller: #controller_name,
                                                expected,
                                                actual: child_length,
                                            },
                                        ));
                                    }
                                }
                            };
                            quote! {
                                let child_length = #actual_end.checked_sub(#start).ok_or_else(|| {
                                    #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError { field: #field_label }),
                                    )
                                })?;
                                let controller_offset = #relative
                                    .and_then(|offset| #schema_start.checked_add(offset))
                                    .ok_or_else(|| #runtime::WriteError::Schema(
                                        #error::Layout(#runtime::LayoutError {
                                            field: #controller_name,
                                        }),
                                    ))?;
                                #apply
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
                FieldKind::Recursive(_) => {
                    unreachable!("recursive builder is rejected before rendering")
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
    let error_fields = schema
        .fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_)))
        .collect::<Vec<_>>();
    let nested_error_parameters = error_fields
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
                #[doc = "The nested schema or counted array failed writing."]
                #[error(#message)]
                #variant(#[source] #parameter),
            })
        })
        .collect::<Vec<_>>();
    let computed_error_variants = schema
        .fields
        .iter()
        .filter_map(|field| {
            let computed = field.kind.computed()?;
            let source = computed.error.as_ref()?;
            let variant = computed_error_ident(&field.name);
            let field_name = field.name.to_string();
            let message = format!("computed field `{field_name}` failed: {{0}}");
            Some(quote! {
                #[doc = "A fallible computed callback rejected the representation."]
                #[error(#message)]
                #variant(#[source] #source),
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
    let has_shared_length = schema
        .fields
        .iter()
        .any(|field| schema.length_dependents(&field.name).nth(1).is_some());
    let length_conflict_variant = has_shared_length.then(|| {
        quote! {
            #[doc = "Payloads governed by one length controller disagree."]
            #[error(
                "controller `{controller}` expected length {expected}, got {actual}"
            )]
            LengthConflict {
                controller: &'static str,
                expected: usize,
                actual: usize,
            },
        }
    });
    let has_errors = schema.layout_can_fail()
        || has_shared_length
        || !conversion_error_variants.is_empty()
        || !computed_error_variants.is_empty()
        || !error_fields.is_empty();
    let error_declaration = if !has_errors {
        TokenStream::new()
    } else if error_fields.is_empty() {
        quote! {
            #[doc = "Typed write failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error {
                #(#conversion_error_variants)*
                #(#computed_error_variants)*
                #layout_error_variant
                #length_conflict_variant
            }
        }
    } else {
        quote! {
            #[doc = "Typed write failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<#(#nested_error_parameters: ::core::error::Error + 'static),*> {
                #(#conversion_error_variants)*
                #(#computed_error_variants)*
                #layout_error_variant
                #length_conflict_variant
                #(#nested_error_variants)*
            }
        }
    };
    let nested_error_types = error_fields.iter().map(|field| match &field.kind {
        FieldKind::Nested(_) => {
            let (_, ty, parameter) = nested_builders
                .iter()
                .find(|(name, _, _)| *name == field.name)
                .expect("nested field has one complete builder");
            quote!(<#ty as #runtime::WireWrite<#parameter>>::Error)
        }
        FieldKind::Array(_) => quote!(::core::convert::Infallible),
        _ => unreachable!("error fields are nested schemas or arrays"),
    });
    let error_type = if error_fields.is_empty() {
        if has_errors {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#nested_error_types),*>)
    };

    let computed_patches = schema
        .computed_fields()
        .filter_map(|field| {
            let computed = field.kind.computed()?;
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("computed destination is scalar")
            };
            let name = &field.name;
            let field_name = name.unraw().to_string();
            let relative = super::builder_offset(&field.offset, runtime);
            let view = private_ident(schema, &format!("{}_computed_view", name.unraw()));
            let semantic = private_ident(schema, &format!("{}_computed_value", name.unraw()));
            let value_ty = value_type_tokens(&scalar.value_type);
            let wire_ty = scalar_type_tokens(scalar.wire_type);
            let encode = to_bytes_method(scalar.endian);
            let call = super::computed::render_call(computed, &view, name, runtime)
                .expect("validated computed callback expression");
            let calculate = if computed.error.is_some() {
                let variant = computed_error_ident(name);
                quote!(#call.map_err(|error| #runtime::WriteError::Schema(
                #error::#variant(error)
            ))?)
            } else {
                call
            };
            let encoded = if scalar.value_type.is_converted() {
                let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                quote!(#conversion.ok_or_else(|| #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #field_name }),
            ))?)
            } else {
                quote!(#semantic)
            };
            Some(quote! {
                let #semantic: #value_ty = {
                    let #view = <#self_type as #runtime::__private::WireSelect>::select_view(
                        writer.as_bytes(),
                    )
                    .map_err(|_| #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    ))?;
                    #calculate
                };
                let encoded: #wire_ty = #encoded;
                let relative = #relative.ok_or_else(|| #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                ))?;
                let offset = #schema_start.checked_add(relative).ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    )
                })?;
                writer.patch_at(offset, &encoded.#encode())?;
            })
        })
        .collect::<Vec<_>>();
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
                #(#computed_patches)*
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
    match &scalar.value_type {
        ValueType::Scalar(_) => quote!(Some(#value)),
        ValueType::Usize | ValueType::Isize | ValueType::Custom(_) => {
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

fn render_bit_controller_source(
    schema: &Schema,
    controller: &syn::Ident,
    build_value: &syn::Ident,
    value_ty: &TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let parts = schema
        .bit_projection_fields()
        .filter_map(|field| {
            let FieldKind::BitProjection(projection) = &field.kind else {
                return None;
            };
            if projection.controller != *controller {
                return None;
            }
            let name = &field.name;
            let start = projection.start;
            let width = projection.end - projection.start + 1;
            let mask = if width == 128 {
                u128::MAX
            } else {
                (1u128 << width) - 1
            };
            let is_bool = matches!(
                &field.ty,
                syn::Type::Path(path) if path.qself.is_none() && path.path.is_ident("bool")
            );
            let raw = if is_bool {
                quote!(if #build_value.#name.0 { 1u128 } else { 0u128 })
            } else {
                quote!(#build_value.#name.0 as u128)
            };
            Some(quote! {
                let part = #raw;
                if part > #mask {
                    return Err(#runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError {
                            field: stringify!(#name),
                        }),
                    ));
                }
                raw |= ((part as #value_ty) << #start)
                    & ((#mask as #value_ty) << #start);
            })
        })
        .collect::<Vec<_>>();
    quote! {{
        let mut raw: #value_ty = 0;
        #(#parts)*
        raw
    }}
}

pub(super) fn computed_error_ident(field: &syn::Ident) -> syn::Ident {
    format_ident!("{}Computed", pascal(field))
}
