//! Owned `bytes::Bytes` struct VIEW rendering.

#[path = "owned/exact.rs"]
mod exact;

use super::{Geometry, Input, Operation, Output, Schema, Types};
use crate::derive::model::{Codec, FieldKind, FieldPosition};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        schema:
            Schema {
                vis,
                fields,
                labels,
                variants,
            },
        geometry:
            Geometry {
                controlled_by,
                position_sources,
                nested_view_paths,
                fixed_sequence,
            },
        types:
            Types {
                view,
                decode_error,
                view_error_type,
                field_proxy,
            },
        operation:
            Operation {
                operation_input_ty,
                operation_parse,
            },
        runtime,
    } = input;
    let sequence_constructor = fixed_sequence.then(|| {
        quote! {
            #[doc(hidden)]
            fn from_sequence(bytes: #runtime::__private::Bytes) -> Self {
                let (view, suffix) = <Self as #runtime::WireView<'static>>::parse_view(bytes)
                    .expect("fixed sequence item passed structural width validation");
                assert!(suffix.is_empty(), "fixed sequence item did not consume its width");
                view
            }
        }
    });

    let view_fields: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            match field.kind {
                FieldKind::Nested => {
                    let child = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    let range = format_ident!("field_{index}_range");
                    quote!(
                        #stored: <#child as #runtime::WireViewType>::View<'static>,
                        #range: ::core::ops::Range<usize>
                    )
                }
                _ => quote!(#stored: ::core::ops::Range<usize>),
            }
        })
        .collect();

    let initializers: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            if matches!(field.kind, FieldKind::Nested) {
                let name = &field.name;
                let stored_range = format_ident!("field_{index}_range");
                let range = format_ident!("range_{index}");
                quote!(#stored: #name, #stored_range: #range)
            } else {
                let range = format_ident!("range_{index}");
                quote!(#stored: #range)
            }
        })
        .collect();

    let decode_steps: Vec<_> = fields
        .iter()
        .enumerate()
        .zip(labels)
        .zip(variants)
        .map(|(((index, field), label), variant)| {
            let name = &field.name;
            let range = format_ident!("range_{index}");
            let geometry = geometry(field, label, decode_error);
            let decode_source = position_sources[index]
                .then(|| match &field.kind {
                    FieldKind::Fixed(codec) => {
                        let codec = super::super::codec_tokens(codec, runtime);
                        quote!(let #name = <#codec as #runtime::FixedCodec>::decode(&input[#range.clone()]);)
                    }
                    _ => unreachable!(),
                })
                .or_else(|| {
                    controlled_by[index].and_then(|_| match &field.kind {
                        FieldKind::Fixed(codec) => {
                            let codec = super::super::codec_tokens(codec, runtime);
                            Some(quote!(let #name = <#codec as #runtime::FixedCodec>::decode(&input[#range.clone()]);))
                        }
                        FieldKind::Prefix(_) => None,
                        _ => unreachable!(),
                    })
                });
            let decode = match &field.kind {
                FieldKind::Fixed(codec) => {
                    let codec = super::super::codec_tokens(codec, runtime);
                    quote! {
                        let width = <#codec as #runtime::FixedCodec>::WIDTH;
                        let available = input.len() - cursor;
                        if available < width {
                            return Err(#decode_error::InputTooShort {
                                field: #label,
                                required: width,
                                available,
                            });
                        }
                        let end = cursor + width;
                        let #range = cursor..end;
                        #decode_source
                        cursor = end;
                    }
                }
                FieldKind::Prefix(codec) => {
                    let source = controlled_by[index].is_some().then(|| quote! {
                        let #name = <#codec as #runtime::PrefixCodec>::decode(&input[#range.clone()]);
                    });
                    quote! {
                        let remaining = &input[cursor..];
                        let extent = <#codec as #runtime::PrefixCodec>::validate_prefix(
                            remaining,
                        )
                        .map_err(#decode_error::#variant)?;
                        let required = extent.encoded_len().get();
                        let available = remaining.len();
                        if available < required {
                            return Err(#decode_error::InputTooShort {
                                field: #label,
                                required,
                                available,
                            });
                        }
                        let end = cursor + required;
                        let #range = cursor..end;
                        #source
                        cursor = end;
                    }
                }
                FieldKind::Bytes { source, .. } => quote! {
                    let required = usize::try_from(#source).map_err(|_| {
                        #decode_error::LengthNotRepresentable {
                            field: #label,
                        }
                    })?;
                    let available = input.len() - cursor;
                    if available < required {
                        return Err(#decode_error::InputTooShort {
                            field: #label,
                            required,
                            available,
                        });
                    }
                    let end = cursor + required;
                    let #range = cursor..end;
                    cursor = end;
                },
                FieldKind::Rest => quote! {
                    let #range = cursor..input.len();
                    cursor = input.len();
                },
                FieldKind::Nested => {
                    let child = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    let parse = if let Some(operation) = &field.operation_input {
                        let parse = format_ident!("__wire_repr_parse_with_{operation}");
                        quote!(
                            <<#child as #runtime::WireViewType>::View<'static>>
                                ::#parse(nested_input, operation)
                        )
                    } else {
                        quote!(
                            <<#child as #runtime::WireViewType>::View<'static>
                                as #runtime::WireView<'static>>::parse_view(nested_input)
                        )
                    };
                    quote! {
                        let start = cursor;
                        let nested_input = input.slice(cursor..);
                        let nested_len = nested_input.len();
                        let (#name, suffix) = #parse.map_err(#decode_error::#variant)?;
                        cursor += nested_len - suffix.len();
                        let #range = start..cursor;
                    }
                }
            };
            quote!(#geometry #decode)
        })
        .collect();

    let fixed_complete = exact::render(exact::Input {
        enabled: fixed_sequence,
        fields,
        labels,
        initializers: &initializers,
        decode_error,
        view_error_type,
        runtime,
    });

    let getters = fields.iter().enumerate().map(|(index, field)| {
        let name = &field.name;
        let label = &labels[index];
        let stored = format_ident!("field_{index}");
        let (ty, value) = match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec_tokens = super::super::codec_tokens(codec, runtime);
                match codec {
                    Codec::OwnedBytes(length) => (
                        quote!(&[u8; #length]),
                        quote! {
                            match <&[u8; #length]>::try_from(&self.bytes[self.#stored.clone()]) {
                                Ok(bytes) => bytes,
                                Err(_) => unreachable!(
                                    "validated fixed byte array has its declared width",
                                ),
                            }
                        },
                    ),
                    _ if field.computation.is_some() => {
                        let ty = &field.computation.as_ref().expect("checked").value_ty;
                        (
                            quote!(#ty),
                            quote!(<#codec_tokens as #runtime::FixedCodec>::decode(
                                &self.bytes[self.#stored.clone()],
                            )),
                        )
                    }
                    _ => (
                        quote!(<#codec_tokens as #runtime::FixedCodec>::Value<'_>),
                        quote!(<#codec_tokens as #runtime::FixedCodec>::decode(
                            &self.bytes[self.#stored.clone()],
                        )),
                    ),
                }
            }
            FieldKind::Prefix(codec) => (
                quote!(<#codec as #runtime::PrefixCodec>::Value<'_>),
                quote!(<#codec as #runtime::PrefixCodec>::decode(
                    &self.bytes[self.#stored.clone()],
                )),
            ),
            FieldKind::Bytes { .. } | FieldKind::Rest => {
                (quote!(&[u8]), quote!(&self.bytes[self.#stored.clone()]))
            }
            FieldKind::Nested => {
                let child = nested_view_paths[index]
                    .as_ref()
                    .expect("nested fields have generated view paths");
                (
                    quote!(<#child as #runtime::WireViewType>::View<'static>),
                    quote!(self.#stored.clone()),
                )
            }
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            #vis fn #name(&self) -> #ty {
                #value
            }
        }
    });

    let parse_body = quote! {
        let mut input = input;
        let mut cursor = 0usize;
        #(#decode_steps)*
        let represented = input.split_to(cursor);
        let remaining = input;
        Ok((
            Self {
                bytes: represented,
                #(#initializers,)*
            },
            remaining,
        ))
    };
    let (complete_error_helper, parse_complete_body) = fixed_complete.map_or_else(
        || {
            (
                quote!(),
                quote! {
                    let mut cursor = 0usize;
                    #(#decode_steps)*
                    if cursor != input.len() {
                        return Err(#decode_error::TrailingBytes {
                            expected: cursor,
                            actual: input.len(),
                        });
                    }
                    Ok(Self {
                        bytes: input,
                        #(#initializers,)*
                    })
                },
            )
        },
        |output| (output.error_helper, output.parse_body),
    );
    let operation_helper = operation_input_ty.map(|operation| {
        quote! {
            #[doc(hidden)]
            #vis fn #operation_parse(
                input: #runtime::__private::Bytes,
                operation: &#operation,
            ) -> Result<(Self, #runtime::__private::Bytes), #view_error_type> {
                #parse_body
            }
        }
    });
    let view_impl = operation_input_ty.is_none().then(|| {
        quote! {
            impl #runtime::WireView<'static> for #view {
                type DecodeError = #view_error_type;
                fn parse_view(
                    input: #runtime::__private::Bytes,
                ) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    #parse_body
                }
                fn parse_complete(
                    input: #runtime::__private::Bytes,
                ) -> Result<Self, Self::DecodeError> {
                    #parse_complete_body
                }
                fn trailing_bytes_error(
                    represented: usize,
                    input: usize,
                ) -> Self::DecodeError {
                    #decode_error::TrailingBytes {
                        expected: represented,
                        actual: input,
                    }
                }
                fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }
            }
        }
    });

    Output {
        declaration: quote! {
            /// An owned bytes-backed validated read view for this wire representation.
            #[derive(Clone, Debug)]
            #vis struct #view {
                bytes: #runtime::__private::Bytes,
                #(#view_fields,)*
            }
            impl #view {
                #complete_error_helper
                #sequence_constructor
                /// Returns this view's exact represented bytes.
                #[must_use]
                #vis fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }
                /// Returns a byte-selection root for this exact source representation.
                #[must_use]
                #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                    #runtime::ByteSelection::new(
                        self,
                        #field_proxy::__wire_repr_new(),
                    )
                }
                #(#getters)*
                #operation_helper
            }
            #view_impl
            impl #runtime::ByteSource for #view {
                #[inline(always)]
                fn byte_len(&self) -> usize {
                    self.as_bytes().len()
                }
                #[inline(always)]
                fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                    sink.write(self.as_bytes());
                }
            }
            impl #runtime::ByteSourceCursor for #view {
                type Segments<'source>
                    = ::core::iter::Once<#runtime::ByteSegment<'source>>
                where
                    Self: 'source;
                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    ::core::iter::once(#runtime::ByteSegment::Bytes(
                        self.as_bytes(),
                    ))
                }

                type Bytes<'__wire_repr_source> = ::core::iter::Copied<::core::slice::Iter<'__wire_repr_source, u8>>
                where
                    Self: '__wire_repr_source;

                #[inline(always)]
                fn bytes(&self) -> Self::Bytes<'_> {
                    self.as_bytes().iter().copied()
                }
            }
        },
    }
}

fn geometry(
    field: &crate::derive::model::Field,
    label: &str,
    error: &proc_macro2::Ident,
) -> TokenStream {
    if let Some(position) = &field.position {
        let (position, conversion) = match position {
            FieldPosition::Static(position) => (quote!(#position), quote!()),
            FieldPosition::Source(source) => (
                quote!(position),
                quote! {
                    let position = usize::try_from(#source).map_err(|_| {
                        #error::PositionNotRepresentable {
                            field: #label,
                            value: #source as u128,
                        }
                    })?;
                },
            ),
        };
        quote! {
            #conversion
            if #position < cursor {
                return Err(#error::PositionBeforeCursor {
                    field: #label,
                    position: #position,
                    cursor,
                });
            }
            let gap = #position - cursor;
            let available = input.len() - cursor;
            if available < gap {
                return Err(#error::InputTooShort {
                    field: #label,
                    required: gap,
                    available,
                });
            }
            cursor = #position;
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = field
            .alignment_before
            .map_or_else(|| quote!(None::<usize>), |value| quote!(Some(#value)));
        quote! {
            let padded = cursor.checked_add(#padding).ok_or(
                #error::GeometryOverflow { field: #label },
            )?;
            let alignment_padding = match #alignment {
                Some(boundary) => {
                    let remainder = padded % boundary;
                    if remainder == 0 {
                        0
                    } else {
                        boundary - remainder
                    }
                }
                None => 0,
            };
            let gap = #padding.checked_add(alignment_padding).ok_or(
                #error::GeometryOverflow { field: #label },
            )?;
            let available = input.len() - cursor;
            if available < gap {
                return Err(#error::InputTooShort {
                    field: #label,
                    required: gap,
                    available,
                });
            }
            cursor += gap;
        }
    }
}
