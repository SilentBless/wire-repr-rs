//! Struct derive rendering.

mod builder;
mod error;
mod fixed;
mod plan;
mod preparation;
mod selection;
mod view;

use super::super::model::{Codec, FieldKind, WireStruct};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    if model.operation_input.is_none()
        && model
            .fields
            .iter()
            .all(|field| matches!(field.kind, FieldKind::Fixed(_)) && field.computation.is_none())
    {
        return fixed::render(model, runtime);
    }

    let WireStruct {
        vis,
        name,
        wire_lifetime,
        operation_input,
        validators: model_validators,
        validation_error,
        fields,
        preparation,
    } = model;
    let computation_order = preparation.computation_order;
    let controlled_by = preparation.controlled_by;
    let position_sources = preparation.position_sources;
    let plan = format_ident!("{name}Plan");
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let operation_input_ty = operation_input.as_ref().map(|input| &input.ty);
    let operation_name = operation_input.as_ref().map(|input| &input.name);
    let operation_parse = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_parse_with_{}", input.name));
    let operation_prepare = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_prepare_with_{}", input.name));
    let operation_value = format_ident!("__wire_repr_operation");
    let view_input_request = format_ident!("{name}ViewInputRequest");
    let direct_view_request = format_ident!("{name}ViewRequest");
    let unchecked_view_request = format_ident!("{name}UncheckedViewRequest");
    let cursor_input_request = format_ident!("{name}CursorInputRequest");
    let direct_cursor = format_ident!("{name}Cursor");
    let unchecked_cursor = format_ident!("{name}UncheckedCursor");
    let encode_request = format_ident!("{name}EncodeRequest");
    let labels: Vec<_> = fields.iter().map(|field| field.name.to_string()).collect();
    let variants: Vec<_> = fields
        .iter()
        .map(|field| variant_name(&field.name.to_string()))
        .collect();
    let plans: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}"))
        .collect();
    let gaps: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            (field.position.is_some()
                || field.padding_before != 0
                || field.alignment_before.is_some())
            .then(|| format_ident!("gap_{index}"))
        })
        .collect();
    let gap_names: Vec<_> = gaps.iter().flatten().collect();
    let has_geometry = !gap_names.is_empty();
    let has_positions = fields.iter().any(|field| field.position.is_some());
    let has_computed = fields.iter().any(|field| field.computation.is_some());
    let has_builder = has_computed || controlled_by.iter().any(Option::is_some);
    let prepare_fields: Vec<_> = fields
        .iter()
        .enumerate()
        .filter(|(index, field)| field.computation.is_none() && controlled_by[*index].is_none())
        .map(|(_, field)| field)
        .collect();
    let prepare_field_names: Vec<_> = prepare_fields.iter().map(|field| &field.name).collect();
    let prepare_field_parameters: Vec<_> = prepare_fields
        .iter()
        .map(|field| {
            let field_name = &field.name;
            let ty = &field.ty;
            quote!(#field_name: #ty)
        })
        .collect();
    let prepare_destructure: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let field_name = &field.name;
            if field.computation.is_some() || controlled_by[index].is_some() {
                quote!(#field_name: _)
            } else {
                quote!(#field_name)
            }
        })
        .collect();
    let prepare_helper = format_ident!("__wire_repr_prepare_fields");

    let has_bytes = controlled_by.iter().any(Option::is_some) || has_computed;
    let has_nested = fields
        .iter()
        .any(|field| matches!(field.kind, FieldKind::Nested));
    let nested_view_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested)
                .then(|| super::generated_view_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_decode_error_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_decode_error_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_plan_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_plan_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_fields_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested)
                .then(|| super::generated_fields_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_encode_error_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_encode_error_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let (plan_decl_generics, plan_decl_where, plan_type, plan_impl_generics, plan_impl_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime, '__wire_repr_value>),
                quote!(where #lifetime: '__wire_repr_value),
                quote!(#plan<#lifetime, '__wire_repr_value>),
                quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
                quote!(#plan<#lifetime, '__wire_repr_value>),
            )
        } else {
            (
                quote!(<'__wire_repr_value>),
                quote!(),
                quote!(#plan<'__wire_repr_value>),
                quote!(<'__wire_repr_value>),
                quote!(#plan<'__wire_repr_value>),
            )
        };
    let (selection_impl_generics, selection_plan_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
                quote!(#plan<#lifetime, '__wire_repr_value>),
            )
        } else {
            (
                quote!(<'__wire_repr_value>),
                quote!(#plan<'__wire_repr_value>),
            )
        };
    let (decode_error_decl_generics, error_impl_type, view_error_type, association_error_type) =
        if has_nested {
            (
                quote!(<'__wire_repr_wire>),
                quote!(#decode_error<'_>),
                quote!(#decode_error<'__wire_repr_wire>),
                quote!(#decode_error<'__wire_repr_view>),
            )
        } else {
            (
                quote!(),
                quote!(#decode_error),
                quote!(#decode_error),
                quote!(#decode_error),
            )
        };
    let (encode_error_decl_generics, encode_error_type, encode_error_impl_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime>),
                quote!(#encode_error<#lifetime>),
                quote!(#encode_error<'_>),
            )
        } else {
            (quote!(), quote!(#encode_error), quote!(#encode_error))
        };
    let custom_validation_error = validation_error;
    let generated_validation_error = custom_validation_error.is_none() && has_nested;
    let validation_error = format_ident!("{name}ValidationError");
    let validation_error_type = if let Some(error) = custom_validation_error.as_ref() {
        quote!(#error)
    } else if generated_validation_error {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#view_error_type)
    };
    let validation_impl = {
        let field_validators = fields.iter().enumerate().flat_map(|(index, field)| {
            let name = &field.name;
            let own = field.validators.iter().map(move |validator| quote!(#validator(self.#name())?;));
            if matches!(field.kind, FieldKind::Nested) {
                let child_view = nested_view_paths[index].as_ref().expect("nested fields have generated view paths");
                let variant = &variants[index];
                let child = if generated_validation_error {
                    let nested_variant = format_ident!("Nested{variant}");
                    quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(#validation_error::#nested_variant)?;)
                } else {
                    quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(<Self::ValidationError as From<_>>::from)?;)
                };
                quote!(#child #(#own)*)
            } else { quote!(#(#own)*) }
        });
        quote! {
            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type ValidationError = #validation_error_type;
                fn validate(&self) -> Result<(), Self::ValidationError> {
                    #(#field_validators)*
                    #(#model_validators(self)?;)*
                    Ok(())
                }
            }
        }
    };
    let generated_validation_error_decl = generated_validation_error.then(|| {
        let nested_variants = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let child_view = nested_view_paths[index].as_ref().expect("nested fields have generated view paths");
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(
                    #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                    #variant(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError),
                )
            });
        let nested_arms = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(Self::#variant(error) => write!(formatter, "nested validation failed in field `{}`: {error}", #label),)
            });
        quote! {
            /// Semantic validation failures for this wire representation.
            #[derive(Debug)]
            #vis enum #validation_error<'__wire_repr_wire> {
                /// Structural framing failed.
                Decode(#view_error_type),
                #(#nested_variants)*
            }
            impl ::core::fmt::Display for #validation_error<'_> {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::Decode(error) => error.fmt(formatter),
                        #(#nested_arms)*
                    }
                }
            }
            impl ::core::error::Error for #validation_error<'_> {}
            impl<'__wire_repr_wire> From<#view_error_type> for #validation_error<'__wire_repr_wire> {
                fn from(error: #view_error_type) -> Self { Self::Decode(error) }
            }
        }
    });
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let cursor_method = quote! {
        /// Returns a fail-closed cursor over consecutive representations.
        #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
            #runtime::ValidatedViewCursor::new(input)
        }
    };
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_request<'__wire_repr_view, #view<'__wire_repr_view>>),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>),
        )
    };

    let selection = selection::render(selection::Input {
        name: &name,
        vis: &vis,
        fields: &fields,
        plans: &plans,
        gaps: &gaps,
        nested_fields_paths: &nested_fields_paths,
        nested_view_paths: &nested_view_paths,
        nested_plan_paths: &nested_plan_paths,
        selection_impl_generics: &selection_impl_generics,
        selection_plan_type: &selection_plan_type,
        view: &view,
        runtime,
    });
    let selection::Output {
        field_proxy,
        declaration: selection_declaration,
    } = selection;
    let view_declaration = view::render(view::Input {
        vis: &vis,
        fields: &fields,
        labels: &labels,
        variants: &variants,
        controlled_by: &controlled_by,
        position_sources: &position_sources,
        nested_view_paths: &nested_view_paths,
        view: &view,
        decode_error: &decode_error,
        view_error_type: &view_error_type,
        operation_input_ty,
        operation_parse: operation_parse.as_ref(),
        field_proxy: &field_proxy,
        runtime,
    })
    .declaration;
    let plan_output = plan::render(plan::Input {
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        fields: &fields,
        plans: &plans,
        gaps: &gaps,
        gap_names: &gap_names,
        nested_plan_paths: &nested_plan_paths,
        plan: &plan,
        plan_decl_generics: &plan_decl_generics,
        plan_decl_where: &plan_decl_where,
        plan_impl_generics: &plan_impl_generics,
        plan_impl_type: &plan_impl_type,
        field_proxy: &field_proxy,
        runtime,
    });
    let plan_declaration = plan_output.declaration;
    let plan_lifetime_init = plan_output.lifetime_init;
    let preparation_body = preparation::render(preparation::Input {
        fields: &fields,
        plans: &plans,
        gaps: &gaps,
        gap_names: &gap_names,
        variants: &variants,
        controlled_by: &controlled_by,
        computation_order: &computation_order,
        operation_prepare: operation_prepare.as_ref(),
        operation_value: &operation_value,
        encode_error: &encode_error,
        plan: &plan,
        plan_lifetime_init: &plan_lifetime_init,
        runtime,
    });

    let error_declaration = error::render(error::Input {
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        fields: &fields,
        labels: &labels,
        variants: &variants,
        nested_view_paths: &nested_view_paths,
        nested_decode_error_paths: &nested_decode_error_paths,
        nested_encode_error_paths: &nested_encode_error_paths,
        has_positions,
        has_geometry,
        has_bytes,
        has_builder,
        has_computed,
        decode_error: &decode_error,
        encode_error: &encode_error,
        decode_error_decl_generics: &decode_error_decl_generics,
        error_impl_type: &error_impl_type,
        encode_error_decl_generics: &encode_error_decl_generics,
        encode_error_impl_type: &encode_error_impl_type,
        runtime,
    });

    let builder = builder::render(builder::Input {
        has_builder,
        name: &name,
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        operation_input_ty,
        operation_name,
        prepare_fields: &prepare_fields,
        prepare_field_names: &prepare_field_names,
        plan: &plan,
        encode_error: &encode_error,
        self_type: &self_type,
        prepare_helper: &prepare_helper,
        runtime,
    });
    let builder_declaration = builder.declaration;
    let builder_method = builder.method;

    let inherent_impl = if operation_input_ty.is_some() {
        let operation_name = operation_name.expect("operation input");
        let operation_encode_generics = if let Some(lifetime) = &wire_lifetime {
            quote!(<#lifetime, '__wire_repr_operation>)
        } else {
            quote!(<'__wire_repr_operation>)
        };
        let operation_encode_request_type = if let Some(lifetime) = &wire_lifetime {
            quote!(#encode_request<#lifetime, '_>)
        } else {
            quote!(#encode_request<'_>)
        };
        quote! {
            #[doc(hidden)] #vis struct #view_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)] #vis struct #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #cursor_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)] #vis struct #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #encode_request #operation_encode_generics { value: #self_type, operation: &'__wire_repr_operation #operation_input_ty }

            #[allow(missing_docs)] impl<'__wire_repr_wire> #view_input_request<'__wire_repr_wire> {
                #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #direct_view_request<'__wire_repr_wire, '_> { #direct_view_request { input: self.input, operation } }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis fn unchecked(self) -> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_view_request { input: self.input, operation: self.operation } }
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #validation_error_type> { let (view, remainder) = #view::#operation_parse(self.input, self.operation)?; #runtime::WireViewValidation::validate(&view)?; Ok((view, remainder)) }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #validation_error_type> { let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?; if suffix.is_empty() { Ok(view) } else { Err(<#validation_error_type as From<#view_error_type>>::from(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len })) } }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #view_error_type> { #view::#operation_parse(self.input, self.operation) }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #view_error_type> { let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?; if suffix.is_empty() { Ok(view) } else { Err(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len }) } }
            }
            #[allow(missing_docs)] impl<'__wire_repr_wire> #cursor_input_request<'__wire_repr_wire> { #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #direct_cursor<'__wire_repr_wire, '_> { #direct_cursor { remaining: self.input, operation } } }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #[must_use] #vis fn unchecked(self) -> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_cursor { remaining: self.remaining, operation: self.operation } }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#validation_error_type>> { if self.remaining.is_empty() { return Ok(None); } let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(|error| #runtime::ViewCursorError::Item(error.into()))?; if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); } #runtime::WireViewValidation::validate(&view).map_err(#runtime::ViewCursorError::Item)?; self.remaining = suffix; Ok(Some(view)) }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#view_error_type>> { if self.remaining.is_empty() { return Ok(None); } let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(#runtime::ViewCursorError::Item)?; if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); } self.remaining = suffix; Ok(Some(view)) }
            }
            #[allow(missing_docs)] impl #operation_encode_generics #encode_request #operation_encode_generics { #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where #self_type: '__wire_repr_value { <#self_type>::#operation_prepare(self.value, self.operation) } #vis fn build_into<'__wire_repr_value, '__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> where #self_type: '__wire_repr_value { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) } }
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #builder_method
                #vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_input_request<'__wire_repr_view> { #view_input_request { input } }
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor_input_request<'__wire_repr_view> { #cursor_input_request { input } }
                #vis fn #operation_name(self, operation: &#operation_input_ty) -> #operation_encode_request_type { #encode_request { value: self, operation } }
                #[doc(hidden)]
                #vis fn #operation_prepare<'__wire_repr_value>(self, #operation_value: &#operation_input_ty) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    let Self { #(#prepare_destructure,)* } = self;
                    Self::#prepare_helper(#(#prepare_field_names,)* #operation_value)
                }
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters,)* #operation_value: &#operation_input_ty) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    #preparation_body
                }
            }
        }
    } else {
        quote! {
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #view_signature { #view_request::new(input) }
                #cursor_method
                #builder_method
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters),*) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    #preparation_body
                }
                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where Self: '__wire_repr_value { <Self as #runtime::WireEncode>::prepare(self) }
                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) }
            }
        }
    };

    let encode_impl = if operation_input_ty.is_some() {
        quote!()
    } else {
        quote! {
            impl #impl_generics #runtime::WireEncode for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;
                fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value {
                    let Self { #(#prepare_destructure,)* } = self;
                    Self::#prepare_helper(#(#prepare_field_names),*)
                }
            }
        }
    };

    Ok(quote! {
        #error_declaration

        #view_declaration
        #generated_validation_error_decl
        #validation_impl

        #selection_declaration
        #plan_declaration

        #builder_declaration
        #inherent_impl
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
        #encode_impl

    })
}

fn codec_tokens(codec: &Codec, runtime: &TokenStream) -> TokenStream {
    match codec {
        Codec::Builtin(name) => {
            let name = format_ident!("{name}");
            quote!(#runtime::#name)
        }
        Codec::OwnedBytes(length) => quote!(#runtime::__private::OwnedBytes<#length>),
        Codec::Custom(path) => quote!(#path),
    }
}
fn variant_name(name: &str) -> proc_macro2::Ident {
    let value: String = name
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    format_ident!("{value}")
}
