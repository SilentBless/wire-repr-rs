use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, TypeParam};

use super::model::{FieldKind, Scalar, Schema, ValueType};
use super::{fresh_type_ident, pascal, value_type_tokens};

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
    let index = choice_index(schema, flag);
    format_ident!("__WireRepr{}Choice{index}Value", schema.name.unraw())
}

pub(super) fn choice_start_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = choice_index(schema, flag);
    format_ident!("__WireRepr{}Choice{index}", schema.name.unraw())
}

pub(super) fn choice_state_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = choice_index(schema, flag);
    format_ident!("__WireRepr{}Choice{index}State", schema.name.unraw())
}

pub(super) fn choice_final_ident(schema: &Schema, flag: &syn::Ident) -> syn::Ident {
    let index = choice_index(schema, flag);
    format_ident!("__WireRepr{}Choice{index}Final", schema.name.unraw())
}

fn choice_index(schema: &Schema, flag: &syn::Ident) -> usize {
    schema
        .fields
        .iter()
        .position(|field| field.name == *flag)
        .expect("flag belongs to schema")
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
