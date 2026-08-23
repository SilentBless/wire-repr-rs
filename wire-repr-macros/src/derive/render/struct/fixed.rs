//! Fixed-only struct derive rendering.

mod error;
mod geometry;
mod interface;
mod plan;
mod selection;
mod view;

use super::super::super::model::{Field, FieldKind, WireStruct};
use super::{codec_tokens, variant_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn sequence_width(
    fields: &[Field],
    operation_free: bool,
    runtime: &TokenStream,
) -> Option<TokenStream> {
    (operation_free
        && fields.iter().all(|field| {
            matches!(field.kind, FieldKind::Fixed(_))
                && field.computation.is_none()
                && field.position.is_none()
                && field.padding_before == 0
                && field.alignment_before.is_none()
        }))
    .then(|| {
        let widths = fields.iter().map(|field| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            }
            _ => unreachable!(),
        });
        quote!(0usize #(+ #widths)*)
    })
}

pub(super) fn render(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireStruct {
        vis,
        name,
        wire_lifetime,
        operation_input: _,
        validators: model_validators,
        validation_error,
        fields,
        preparation,
    } = model;
    let position_sources = preparation.position_sources;
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let plan = format_ident!("{name}Plan");
    let field_proxy = format_ident!("{name}Fields");
    let markers: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("__WireRepr{name}Field{index}"))
        .collect();
    let labels: Vec<_> = fields.iter().map(|field| field.name.to_string()).collect();
    let variants: Vec<_> = fields
        .iter()
        .map(|field| variant_name(&field.name.to_string()))
        .collect();
    let field_plans: Vec<_> = (0..fields.len())
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
    let validation_error_type = validation_error
        .as_ref()
        .map(|error| quote!(#error))
        .unwrap_or_else(|| quote!(#decode_error));
    let (impl_generics, self_type) = if let Some(lifetime) = &wire_lifetime {
        (quote!(<#lifetime>), quote!(#name<#lifetime>))
    } else {
        (quote!(), quote!(#name))
    };
    let fixed_widths: Vec<_> = fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            }
            _ => unreachable!(),
        })
        .collect();
    let plain_fixed_sequence = fields.iter().all(|field| {
        field.position.is_none() && field.padding_before == 0 && field.alignment_before.is_none()
    });

    let error_declaration = error::render(error::Input {
        schema: error::Schema {
            vis: &vis,
            fields: &fields,
            labels: &labels,
            variants: &variants,
        },
        types: error::Types {
            decode_error: &decode_error,
            encode_error: &encode_error,
        },
        geometry: error::Geometry {
            has_positions,
            has_geometry,
        },
        runtime,
    });
    let view_declaration = view::render(view::Input {
        schema: view::Schema {
            vis: &vis,
            fields: &fields,
            labels: &labels,
            position_sources: &position_sources,
        },
        types: view::Types {
            view: &view,
            decode_error: &decode_error,
            field_proxy: &field_proxy,
            self_type: &self_type,
            impl_generics: &impl_generics,
        },
        validation: view::Validation {
            model_validators: &model_validators,
            error_type: &validation_error_type,
        },
        runtime,
    });
    let selection_declaration = selection::render(selection::Input {
        schema: selection::Schema {
            fields: &fields,
            markers: &markers,
            plans: &field_plans,
            gaps: &gaps,
        },
        types: selection::Types {
            field_proxy: &field_proxy,
            view: &view,
            plan: &plan,
        },
        surface: selection::Surface { runtime, vis: &vis },
    });
    let plan::Output {
        declaration: plan_declaration,
        wire_encode,
    } = plan::render(plan::Input {
        schema: plan::Schema {
            fields: &fields,
            labels: &labels,
            variants: &variants,
        },
        layout: plan::Layout {
            plans: &field_plans,
            gaps: &gaps,
            gap_names: &gap_names,
        },
        types: plan::Types {
            vis: &vis,
            plan: &plan,
            encode_error: &encode_error,
            field_proxy: &field_proxy,
            self_type: &self_type,
            impl_generics: &impl_generics,
        },
        runtime,
    });
    let interface_declaration = interface::render(interface::Input {
        identity: interface::Identity {
            vis: &vis,
            view: &view,
            plan: &plan,
            encode_error: &encode_error,
        },
        types: interface::Types {
            wire_lifetime: wire_lifetime.as_ref(),
            self_type: &self_type,
            impl_generics: &impl_generics,
        },
        sequence: interface::Sequence {
            plain: plain_fixed_sequence,
            has_custom_validation_error: validation_error.is_some(),
            fixed_widths: &fixed_widths,
            validation_error: validation_error.as_ref(),
        },
        runtime,
    });

    Ok(quote! {
        #error_declaration
        #view_declaration
        #selection_declaration
        #plan_declaration
        #interface_declaration
        #wire_encode
    })
}
