//! Bitfield VIEW declaration rendering.

use super::super::super::model::BitfieldField;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) mode: Mode<'a>,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) name: &'a Ident,
    pub(super) fields: &'a [BitfieldField],
}

pub(super) struct Types<'a> {
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
    pub(super) codec: &'a Ident,
    pub(super) storage_type: &'a Ident,
}

pub(super) struct Mode<'a> {
    pub(super) runtime: &'a TokenStream,
    pub(super) owned_mode: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema: Schema { vis, name, fields },
        types:
            Types {
                view,
                decode_error,
                codec,
                storage_type,
            },
        mode: Mode {
            runtime,
            owned_mode,
        },
    } = input;
    let getters: Vec<_> = fields
        .iter()
        .map(|field| {
            let field_name = &field.name;
            let field_type = &field.ty;
            let start = field.start;
            let width = field.end - field.start + 1;
            let value = if width == 1 {
                quote!(((raw >> #start) & 1) != 0)
            } else {
                let mask = if width == 128 {
                    quote!(u128::MAX)
                } else {
                    quote!(((1 as #storage_type) << #width) - 1)
                };
                quote!(((raw >> #start) & #mask) as #field_type)
            };
            quote! {
                #[doc = concat!("Returns the decoded `", stringify!(#field_name), "` projection.")]
                #[must_use]
                #vis fn #field_name(&self) -> #field_type {
                    let raw = <#runtime::#codec as #runtime::FixedCodec>::decode(self.bytes.as_ref());
                    #value
                }
            }
        })
        .collect();

    if owned_mode {
        quote! {
            /// A bytes-backed validated view of this nominal bitfield.
            #[derive(Clone, Debug)]
            #vis struct #view {
                bytes: #runtime::__private::Bytes
            }

            impl #view {
                /// Returns the exact represented storage bytes.
                #[must_use]
                #vis fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }

                fn from_sequence(bytes: #runtime::__private::Bytes) -> Self {
                    Self { bytes }
                }

                #(#getters)*
            }

            impl #runtime::WireView<'static> for #view {
                type DecodeError = #decode_error;

                fn parse_view(
                    mut input: #runtime::__private::Bytes
                ) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    if available < width {
                        return Err(#decode_error::InputTooShort {
                            required: width,
                            available
                        });
                    }
                    let bytes = input.split_to(width);
                    Ok((Self { bytes }, input))
                }

                fn trailing_bytes_error(
                    represented: usize,
                    input: usize
                ) -> Self::DecodeError {
                    #decode_error::TrailingBytes {
                        expected: represented,
                        actual: input
                    }
                }

                fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }
            }

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
                type Segments<'__wire_repr_source> = ::core::iter::Once<
                    #runtime::ByteSegment<'__wire_repr_source>
                >
                where
                    Self: '__wire_repr_source;

                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
                }
            }

            impl #runtime::WireViewValidation<'static> for #view {
                type ValidationError = #decode_error;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    Ok(())
                }
            }

            impl #runtime::WireViewType for #name {
                type DecodeError<'__wire_repr_wire> = #decode_error;
                type View<'__wire_repr_wire> = #view;
            }
        }
    } else {
        quote! {
            /// A bytes-backed validated view of this nominal bitfield.
            #[derive(Clone, Copy, Debug)]
            #vis struct #view<'__wire_repr_wire> {
                bytes: &'__wire_repr_wire [u8]
            }

            impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
                /// Returns the exact represented storage bytes.
                #[must_use]
                #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }

                fn from_sequence(bytes: &'__wire_repr_wire [u8]) -> Self {
                    Self { bytes }
                }

                #(#getters)*
            }

            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire>
                for #view<'__wire_repr_wire>
            {
                type DecodeError = #decode_error;

                fn parse_view(
                    input: &'__wire_repr_wire [u8]
                ) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    let Some((bytes, suffix)) = input.split_at_checked(width) else {
                        return Err(#decode_error::InputTooShort {
                            required: width,
                            available
                        });
                    };
                    Ok((Self { bytes }, suffix))
                }

                fn trailing_bytes_error(
                    represented: usize,
                    input: usize
                ) -> Self::DecodeError {
                    #decode_error::TrailingBytes {
                        expected: represented,
                        actual: input
                    }
                }

                fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }
            }

            impl<'__wire_repr_wire> #runtime::ByteSource for #view<'__wire_repr_wire> {
                #[inline(always)]
                fn byte_len(&self) -> usize {
                    self.as_bytes().len()
                }

                #[inline(always)]
                fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                    sink.write(self.as_bytes());
                }
            }

            impl<'__wire_repr_wire> #runtime::ByteSourceCursor for #view<'__wire_repr_wire> {
                type Segments<'__wire_repr_source> = ::core::iter::Once<
                    #runtime::ByteSegment<'__wire_repr_source>
                >
                where
                    Self: '__wire_repr_source;

                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
                }
            }

            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire>
                for #view<'__wire_repr_wire>
            {
                type ValidationError = #decode_error;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    Ok(())
                }
            }

            impl #runtime::WireViewType for #name {
                type DecodeError<'__wire_repr_wire> = #decode_error;
                type View<'__wire_repr_wire> = #view<'__wire_repr_wire>;
            }
        }
    }
}
