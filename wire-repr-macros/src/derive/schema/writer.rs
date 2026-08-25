use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, TypeParam, parse_quote};

use super::builder::{Slot, SlotKind, convert_to_wire, slots, unique_build_variant};
use super::model::{FieldKind, Schema};
use super::{
    fresh_field_ident, fresh_type_ident, pascal, scalar_type_tokens, to_bytes_method,
    value_type_tokens,
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
    let mut used_error_names = BTreeSet::new();
    let conversion_names = schema
        .fields
        .iter()
        .map(|field| {
            let FieldKind::Scalar(scalar) = &field.kind;
            scalar.value_type.is_converted().then(|| {
                unique_build_variant(
                    &mut used_error_names,
                    &format!("{}Value", pascal(&field.name)),
                )
            })
        })
        .collect::<Vec<_>>();
    let nested_variant = schema.nested.as_ref().map(|nested| {
        unique_build_variant(&mut used_error_names, &pascal(&nested.name).to_string())
    });
    let has_conversions = conversion_names.iter().any(Option::is_some);
    let mut rendered = Vec::new();
    for (target_index, target) in slots.iter().enumerate() {
        let mut impl_generics = schema.generics.clone();
        let mut output_parameter = TypeParam::from(output.clone());
        output_parameter.bounds.push(parse_quote!(#runtime::Output));
        impl_generics
            .params
            .push(GenericParam::Type(output_parameter));
        let build_fn = fresh_type_ident(&impl_generics, "BuildFn");
        let child_builder = fresh_type_ident(&impl_generics, "ChildBuilder");
        let mut current_states = Vec::with_capacity(slots.len());
        let mut returned_states = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            if index == target_index {
                current_states.push(quote!(#runtime::__private::Unset));
                returned_states.push(match &target.kind {
                    SlotKind::Scalar(ty) => {
                        let ty = value_type_tokens(*ty);
                        quote!(#runtime::__private::Set<#ty>)
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
        let current_writer = writer_type(writer_name, original_arguments, output, &current_states);
        let returned_writer =
            writer_type(writer_name, original_arguments, output, &returned_states);
        let field = &target.field;

        match &target.kind {
            SlotKind::Scalar(value_type) => {
                let scalar = schema
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|field| {
                        let FieldKind::Scalar(scalar) = &field.kind;
                        scalar
                    })
                    .expect("writer slot must refer to one schema scalar");
                let offset = schema
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|field| field.offset)
                    .expect("writer slot must have one physical offset");
                let prefix_width = schema.prefix_width;
                let value_ty = value_type_tokens(*value_type);
                let wire_ty = scalar_type_tokens(scalar.wire_type);
                let encode = to_bytes_method(scalar.endian);
                let width = scalar.wire_type.width();
                let field_index = schema
                    .fields
                    .iter()
                    .position(|candidate| candidate.name == *field)
                    .expect("writer slot must have one schema field");
                let schema_error = writer_error_type(
                    schema,
                    &error_name,
                    has_conversions,
                    quote!(::core::convert::Infallible),
                );
                if value_type.is_converted() {
                    let conversion_variant = conversion_names[field_index]
                        .as_ref()
                        .expect("converted writer field must have one error variant");
                    let semantic =
                        fresh_field_ident(schema, &format!("{}_semantic", field.unraw()));
                    let encoded = fresh_field_ident(schema, &format!("{}_encoded", field.unraw()));
                    let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                    rendered.push(quote! {
                        impl #impl_params #current_writer #impl_where {
                            #[doc = concat!("Writes field `", stringify!(#field), "`.")]
                            #[inline]
                            #vis fn #field(
                                mut self,
                                value: #value_ty,
                            ) -> Result<
                                #returned_writer,
                                #runtime::WriteError<
                                    #schema_error,
                                    <#output as #runtime::Output>::GrowError,
                                >,
                            > {
                                let #semantic: #value_ty = value;
                                let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                                    #runtime::WriteError::Schema(
                                        #error_name::#conversion_variant(
                                            #runtime::ScalarBuildConversionError {
                                                from: stringify!(#value_ty),
                                                to: stringify!(#wire_ty),
                                            },
                                        ),
                                    )
                                })?;
                                self.writer.ensure(#offset + #width, #prefix_width)?;
                                self.writer.write_at(#offset, &#encoded.#encode())?;
                                Ok(#writer_name {
                                    writer: self.writer,
                                    #marker: ::core::marker::PhantomData,
                                })
                            }
                        }
                    });
                } else {
                    let width = scalar.wire_type.width();
                    rendered.push(quote! {
                        impl #impl_params #current_writer #impl_where {
                            #[doc = concat!("Writes field `", stringify!(#field), "`.")]
                            #[inline]
                            #vis fn #field(
                                mut self,
                                value: #value_ty,
                            ) -> Result<
                                #returned_writer,
                                #runtime::WriteError<
                                    #schema_error,
                                    <#output as #runtime::Output>::GrowError,
                                >,
                            > {
                                let encoded: #wire_ty = value;
                                self.writer.ensure(#offset + #width, #prefix_width)?;
                                self.writer.write_at(#offset, &encoded.#encode())?;
                                Ok(#writer_name {
                                    writer: self.writer,
                                    #marker: ::core::marker::PhantomData,
                                })
                            }
                        }
                    });
                }
            }
            SlotKind::Nested(ty) => {
                let offset = schema
                    .nested
                    .as_ref()
                    .map(|nested| nested.offset)
                    .expect("nested writer slot must have one physical offset");
                let nested_variant = nested_variant
                    .as_ref()
                    .expect("nested writer field must have one error variant");
                let schema_error = writer_error_type(
                    schema,
                    &error_name,
                    has_conversions,
                    quote!(<#ty as #runtime::WireWrite<#child_builder>>::Error),
                );
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
                            {
                                let mut child_writer = self.writer.child_at(#offset)?;
                                let value = build(<#ty as #runtime::WireBuilder>::builder());
                                <#ty as #runtime::WireWrite<#child_builder>>::write(
                                    value,
                                    &mut child_writer,
                                )
                                .map_err(|error| match error {
                                    #runtime::WriteError::Schema(error) => {
                                        #runtime::WriteError::Schema(
                                            #error_name::#nested_variant(error),
                                        )
                                    }
                                    #runtime::WriteError::Output(error) => {
                                        #runtime::WriteError::Output(error)
                                    }
                                })?;
                            }
                            Ok(#writer_name {
                                writer: self.writer,
                                #marker: ::core::marker::PhantomData,
                            })
                        }
                    }
                });
            }
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
    let mut used_error_names = BTreeSet::new();
    let conversion_names = schema
        .fields
        .iter()
        .map(|field| {
            let FieldKind::Scalar(scalar) = &field.kind;
            scalar.value_type.is_converted().then(|| {
                unique_build_variant(
                    &mut used_error_names,
                    &format!("{}Value", pascal(&field.name)),
                )
            })
        })
        .collect::<Vec<_>>();
    let has_conversions = conversion_names.iter().any(Option::is_some);

    let mut impl_generics = schema.generics.clone();
    let mut output_parameter = TypeParam::from(output.clone());
    output_parameter.bounds.push(parse_quote!(#runtime::Output));
    impl_generics
        .params
        .push(GenericParam::Type(output_parameter));
    let mut complete_states = Vec::with_capacity(slots.len());
    let mut child_error = quote!(::core::convert::Infallible);
    for slot in slots {
        match &slot.kind {
            SlotKind::Scalar(ty) => {
                let ty = value_type_tokens(*ty);
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
                child_error = quote!(<#ty as #runtime::WireWrite<#child>>::Error);
                complete_states.push(quote!(#runtime::__private::Set<#child>));
            }
        }
    }
    let schema_error = writer_error_type(schema, &error_name, has_conversions, child_error);
    let complete_writer = writer_type(writer_name, original_arguments, output, &complete_states);
    let (impl_params, _, impl_where) = impl_generics.split_for_impl();
    let prefix_width = schema.prefix_width;
    let constant_writes = schema
        .fields
        .iter()
        .zip(&conversion_names)
        .enumerate()
        .filter_map(|(index, (field, conversion_variant))| {
            let FieldKind::Scalar(scalar) = &field.kind;
            let constant = scalar.constant.as_ref()?;
            let value_ty = value_type_tokens(scalar.value_type);
            let wire_ty = scalar_type_tokens(scalar.wire_type);
            let encode = to_bytes_method(scalar.endian);
            let offset = field.offset;
            if let Some(variant) = conversion_variant {
                let semantic = fresh_field_ident(schema, &format!("constant_{index}_semantic"));
                let encoded = fresh_field_ident(schema, &format!("constant_{index}_encoded"));
                let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                Some(quote! {
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
                    self.writer.write_at(#offset, &#encoded.#encode())?;
                })
            } else {
                Some(quote! {
                    let encoded: #wire_ty = #constant;
                    self.writer.write_at(#offset, &encoded.#encode())?;
                })
            }
        });

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
                self.writer.ensure(#prefix_width, #prefix_width)?;
                #(#constant_writes)*
                Ok(self.writer.finish())
            }
        }
    }
}

fn writer_error_type(
    schema: &Schema,
    error: &syn::Ident,
    has_conversions: bool,
    child_error: TokenStream,
) -> TokenStream {
    if schema.nested.is_some() {
        quote!(#error<#child_error>)
    } else if has_conversions {
        quote!(#error)
    } else {
        quote!(::core::convert::Infallible)
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
