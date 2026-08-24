//! Bitfield VIEW declaration rendering.

use super::super::super::model::BitfieldField;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) owned_mode: bool,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) name: &'a Ident,
    pub(super) fields: &'a [BitfieldField],
}

pub(super) struct Types<'a> {
    pub(super) view: &'a Ident,
    pub(super) error: &'a Ident,
    pub(super) codec: &'a Ident,
    pub(super) storage_type: &'a Ident,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema: Schema { vis, name, fields },
        types:
            Types {
                view,
                error,
                codec,
                storage_type,
            },
        owned_mode,
        runtime,
    } = input;
    let state = format_ident!("{view}State");
    let getter_declarations: Vec<_> = fields
        .iter()
        .map(|field| {
            let field_name = &field.name;
            let field_type = &field.ty;
            quote! {
                #[doc = concat!("Returns the decoded `", stringify!(#field_name), "` projection.")]
                #[must_use]
                fn #field_name(&self) -> #field_type;
            }
        })
        .collect();
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
                #[inline(always)]
                fn #field_name(&self) -> #field_type {
                    let raw = <#runtime::#codec as #runtime::FixedCodec>::decode(
                        self.input.as_ref(),
                    );
                    #value
                }
            }
        })
        .collect();

    let wire_view_implementations = if owned_mode {
        quote! {
            impl #runtime::WireView<'static> for #state<#runtime::__private::Bytes> {
                type DecodeError = #error;

                fn parse_view(mut input: #runtime::__private::Bytes) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    if available < width {
                        return Err(#error::InputTooShort { required: width, available });
                    }
                    let bytes = input.split_to(width);
                    Ok((Self { input: bytes }, input))
                }

                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #error::TrailingBytes { expected: represented, actual: input }
                }

                fn as_bytes(&self) -> &[u8] {
                    self.input.as_ref()
                }
            }

            impl #runtime::WireViewValidation<'static> for #state<#runtime::__private::Bytes> {
                type ValidationError = #error;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    Ok(())
                }
            }

            impl #runtime::WireViewType for #name {
                type DecodeError<'__wire_repr_wire> = #error;
                type View<'__wire_repr_wire> = #state<#runtime::__private::Bytes>;
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire>
                for #state<&'__wire_repr_wire [u8]>
            {
                type DecodeError = #error;

                fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    let Some((bytes, suffix)) = input.split_at_checked(width) else {
                        return Err(#error::InputTooShort { required: width, available });
                    };
                    Ok((Self { input: bytes }, suffix))
                }

                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #error::TrailingBytes { expected: represented, actual: input }
                }

                fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.input
                }
            }

            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire>
                for #state<&'__wire_repr_wire [u8]>
            {
                type ValidationError = #error;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    Ok(())
                }
            }

            impl #runtime::WireViewType for #name {
                type DecodeError<'__wire_repr_wire> = #error;
                type View<'__wire_repr_wire> = #state<&'__wire_repr_wire [u8]>;
            }
        }
    };

    quote! {
        /// A validated view of this nominal bitfield.
        #vis trait #view: #runtime::ByteSource + #runtime::ByteSourceCursor {
            /// Returns the exact represented storage bytes.
            #[must_use]
            fn as_bytes(&self) -> &[u8];

            #(#getter_declarations)*
        }

        /// Concrete backing state for [`#view`].
        #[doc(hidden)]
        #[derive(Debug)]
        #vis struct #state<T: ::core::convert::AsRef<[u8]>> {
            input: T,
        }

        impl<T: ::core::convert::AsRef<[u8]> + Clone> Clone for #state<T> {
            fn clone(&self) -> Self {
                Self { input: self.input.clone() }
            }
        }

        impl<T: ::core::convert::AsRef<[u8]> + Copy> Copy for #state<T> {}

        impl<T: ::core::convert::AsRef<[u8]>> #state<T> {
            #[inline(always)]
            fn from_sequence(input: T) -> Self {
                Self { input }
            }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #view for #state<T> {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                self.input.as_ref()
            }

            #(#getters)*
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSource for #state<T> {
            #[inline(always)]
            fn byte_len(&self) -> usize {
                self.as_bytes().len()
            }

            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                sink.write(self.as_bytes());
            }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSourceCursor for #state<T> {
            type Segments<'__wire_repr_source> = ::core::iter::Once<
                #runtime::ByteSegment<'__wire_repr_source>
            >
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
            }

            type Bytes<'__wire_repr_source> = ::core::iter::Copied<::core::slice::Iter<'__wire_repr_source, u8>>
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn bytes(&self) -> Self::Bytes<'_> {
                self.as_bytes().iter().copied()
            }
        }

        #wire_view_implementations
    }
}
