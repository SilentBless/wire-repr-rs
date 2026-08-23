//! Borrowed VIEW storage rendering.

use crate::derive::model::{Field, FieldKind};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn declarations(
    fields: &[Field],
    nested_view_paths: &[Option<TokenStream>],
) -> Vec<TokenStream> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            match &field.kind {
                FieldKind::Nested => {
                    let child_view = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    quote!(#stored: #child_view<'__wire_repr_wire>)
                }
                FieldKind::Fixed(codec) => match codec.static_width() {
                    Some(width) => quote!(#stored: &'__wire_repr_wire [u8; #width]),
                    None => quote!(#stored: &'__wire_repr_wire [u8]),
                },
                FieldKind::Prefix(_) | FieldKind::Bytes { .. } | FieldKind::Rest => {
                    quote!(#stored: &'__wire_repr_wire [u8])
                }
            }
        })
        .collect()
}

pub(super) fn initializers(fields: &[Field]) -> Vec<TokenStream> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            let name = &field.name;
            match field.kind {
                FieldKind::Fixed(_) | FieldKind::Prefix(_) => {
                    let raw = format_ident!("raw_{index}");
                    quote!(#stored: #raw)
                }
                FieldKind::Nested | FieldKind::Bytes { .. } | FieldKind::Rest => {
                    quote!(#stored: #name)
                }
            }
        })
        .collect()
}
