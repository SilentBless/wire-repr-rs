//! Struct derive rendering.

mod builder;
mod error;
mod fixed;
mod interface;
mod plan;
mod preparation;
mod selection;
mod validation;
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
    let validation_error = format_ident!("{name}ValidationError");
    let self_type = if let Some(lifetime) = &wire_lifetime {
        quote!(#name<#lifetime>)
    } else {
        quote!(#name)
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
    let validation::Output {
        error_type: validation_error_type,
        declaration: validation_declaration,
    } = validation::render(validation::Input {
        vis: &vis,
        fields: &fields,
        labels: &labels,
        variants: &variants,
        nested_view_paths: &nested_view_paths,
        model_validators: &model_validators,
        custom_validation_error: custom_validation_error.as_ref(),
        has_nested,
        view: &view,
        validation_error: &validation_error,
        view_error_type: &view_error_type,
        runtime,
    });
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

    let interface_declaration = interface::render(interface::Input {
        name: &name,
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        operation_input_ty,
        operation_name,
        operation_parse: operation_parse.as_ref(),
        operation_prepare: operation_prepare.as_ref(),
        operation_value: &operation_value,
        view_input_request: &view_input_request,
        direct_view_request: &direct_view_request,
        unchecked_view_request: &unchecked_view_request,
        cursor_input_request: &cursor_input_request,
        direct_cursor: &direct_cursor,
        unchecked_cursor: &unchecked_cursor,
        encode_request: &encode_request,
        view: &view,
        decode_error: &decode_error,
        view_error_type: &view_error_type,
        validation_error_type: &validation_error_type,
        association_error_type: &association_error_type,
        plan_type: &plan_type,
        encode_error_type: &encode_error_type,
        prepare_helper: &prepare_helper,
        preparation_body: &preparation_body,
        prepare_field_parameters: &prepare_field_parameters,
        prepare_destructure: &prepare_destructure,
        prepare_field_names: &prepare_field_names,
        builder_method: &builder_method,
        runtime,
    });

    Ok(quote! {
        #error_declaration

        #view_declaration
        #validation_declaration

        #selection_declaration
        #plan_declaration

        #builder_declaration
        #interface_declaration

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
