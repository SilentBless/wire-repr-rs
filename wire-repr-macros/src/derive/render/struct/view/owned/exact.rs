//! Exact-frame owned struct parsing.

use crate::derive::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) enabled: bool,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) initializers: &'a [TokenStream],
    pub(super) decode_error: &'a Ident,
    pub(super) view_error_type: &'a TokenStream,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) error_helper: TokenStream,
    pub(super) parse_body: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Option<Output> {
    let Input {
        enabled,
        fields,
        labels,
        initializers,
        decode_error,
        view_error_type,
        runtime,
    } = input;
    enabled.then(|| {
        let widths: Vec<_> = fields
            .iter()
            .map(|field| match &field.kind {
                FieldKind::Fixed(codec) => {
                    let codec = super::super::super::codec_tokens(codec, runtime);
                    quote!(<#codec as #runtime::FixedCodec>::WIDTH)
                }
                _ => unreachable!("fixed sequences contain only fixed fields"),
            })
            .collect();
        let ranges = widths.iter().enumerate().map(|(index, width)| {
            let range = format_ident!("range_{index}");
            quote! {
                let end = cursor + #width;
                let #range = cursor..end;
                cursor = end;
            }
        });

        let errors = fields
            .iter()
            .zip(labels)
            .zip(&widths)
            .map(|((_, label), width)| {
                quote! {
                    let available = input_len - cursor;
                    if available < #width {
                        return #decode_error::InputTooShort {
                            field: #label,
                            required: #width,
                            available,
                        };
                    }
                    cursor += #width;
                }
            });
        let total = quote!(0usize #(+ #widths)*);
        Output {
            error_helper: quote! {
                #[cold]
                #[inline(never)]
                fn __wire_repr_complete_error(
                    input_len: usize,
                ) -> #view_error_type {
                    let mut cursor = 0usize;
                    #(#errors)*
                    #decode_error::TrailingBytes {
                        expected: cursor,
                        actual: input_len,
                    }
                }
            },
            parse_body: quote! {
                if input.len() != #total {
                    return Err(Self::__wire_repr_complete_error(input.len()));
                }
                let mut cursor = 0usize;
                #(#ranges)*
                Ok(Self {
                    bytes: input,
                    #(#initializers,)*
                })
            },
        }
    })
}
