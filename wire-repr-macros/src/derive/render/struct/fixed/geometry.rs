//! Fixed-struct geometry token rendering.

use super::super::super::super::model::{Field, FieldKind, FieldPosition};
use super::super::codec_tokens;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) fn decode_geometry(field: &Field, label: &str, decode_error: &Ident) -> TokenStream {
    if let Some(position) = &field.position {
        let (position, conversion) = match position {
            FieldPosition::Static(position) => (quote!(#position), quote!()),
            FieldPosition::Source(source) => (
                quote!(position),
                quote!(
                    let position = usize::try_from(#source).map_err(|_| #decode_error::PositionNotRepresentable {
                        field: #label,
                        value: #source as u128
                    })?;
                ),
            ),
        };
        quote! {
            #conversion
            let represented = input.len() - remaining.len();
            if #position < represented {
                return Err(#decode_error::PositionBeforeCursor {
                    field: #label,
                    position: #position,
                    cursor: represented
                });
            }
            let gap = #position - represented;
            let available = remaining.len();
            let Some((_, suffix)) = remaining.split_at_checked(gap) else {
                return Err(#decode_error::InputTooShort {
                    field: #label,
                    required: gap,
                    available
                });
            };
            remaining = suffix;
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! {
            let represented = input.len() - remaining.len();
            let padded = represented.checked_add(#padding).ok_or(
                #decode_error::GeometryOverflow { field: #label }
            )?;
            let alignment_padding = match #alignment {
                Some(boundary) => {
                    let remainder = padded % boundary;
                    if remainder == 0 {
                        0
                    } else {
                        boundary - remainder
                    }
                },
                None => 0
            };
            let gap = #padding.checked_add(alignment_padding).ok_or(
                #decode_error::GeometryOverflow { field: #label }
            )?;
            let available = remaining.len();
            let Some((_, suffix)) = remaining.split_at_checked(gap) else {
                return Err(#decode_error::InputTooShort {
                    field: #label,
                    required: gap,
                    available
                });
            };
            remaining = suffix;
        }
    }
}

pub(super) fn view_getter_cursor_step(field: &Field, runtime: &TokenStream) -> TokenStream {
    let codec = match &field.kind {
        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
        _ => unreachable!(),
    };
    let geometry = view_getter_geometry(field, runtime);
    quote! {
        #geometry
        cursor = cursor.checked_add(<#codec as #runtime::FixedCodec>::WIDTH)
            .expect("view field geometry overflow");
    }
}

pub(super) fn view_getter_geometry(field: &Field, _runtime: &TokenStream) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => quote! {
                cursor = usize::try_from(target.#source())
                    .expect("validated position source fits usize");
            },
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! {
            let padded = cursor
                .checked_add(#padding)
                .expect("view field geometry overflow");
            let alignment_padding = match #alignment {
                Some(boundary) => {
                    let remainder = padded % boundary;
                    if remainder == 0 {
                        0
                    } else {
                        boundary - remainder
                    }
                },
                None => 0
            };
            cursor = padded
                .checked_add(alignment_padding)
                .expect("view field geometry overflow");
        }
    }
}

pub(super) fn getter_cursor_step(field: &Field, runtime: &TokenStream) -> TokenStream {
    let codec = match &field.kind {
        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
        _ => unreachable!(),
    };
    let geometry = getter_geometry(field, runtime);
    quote! {
        #geometry
        cursor += <#codec as #runtime::FixedCodec>::WIDTH;
    }
}

pub(super) fn getter_geometry(field: &Field, _runtime: &TokenStream) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => {
                quote! {
                    cursor = usize::try_from(self.#source())
                        .expect("validated position source fits usize");
                }
            }
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! {
            let padded = cursor + #padding;
            let alignment_padding = match #alignment {
                Some(boundary) => {
                    let remainder = padded % boundary;
                    if remainder == 0 {
                        0
                    } else {
                        boundary - remainder
                    }
                },
                None => 0
            };
            cursor = padded + alignment_padding;
        }
    }
}
