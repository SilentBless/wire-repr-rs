//! Fixed-struct borrowed view rendering.

use super::geometry::{decode_geometry, getter_cursor_step, getter_geometry};
use crate::derive::model::{Codec, Field};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) validation: Validation<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) position_sources: &'a [bool],
}

pub(super) struct Types<'a> {
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
    pub(super) field_proxy: &'a Ident,
    pub(super) self_type: &'a TokenStream,
    pub(super) impl_generics: &'a TokenStream,
}

pub(super) struct Validation<'a> {
    pub(super) model_validators: &'a [syn::Path],
    pub(super) error_type: &'a TokenStream,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema:
            Schema {
                vis,
                fields,
                labels,
                position_sources,
            },
        types:
            Types {
                view,
                decode_error,
                field_proxy,
                self_type,
                impl_generics,
            },
        validation: Validation {
            model_validators,
            error_type,
        },
        runtime,
    } = input;
    let plain_fixed_sequence = fields.iter().all(|field| {
        field.position.is_none() && field.padding_before == 0 && field.alignment_before.is_none()
    });
    let constructor = plain_fixed_sequence.then(|| {
        quote! {
            fn from_sequence(bytes: &'__wire_repr_wire [u8]) -> Self {
                Self { bytes }
            }
        }
    });
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = fixed_codec(field, runtime);
        let prior = fields
            .iter()
            .take(index)
            .map(|prior| getter_cursor_step(prior, runtime));
        let geometry = getter_geometry(field, runtime);
        let return_type = match &field.kind {
            crate::derive::model::FieldKind::Fixed(Codec::OwnedBytes(length)) => {
                quote!(&'__wire_repr_wire [u8; #length])
            }
            crate::derive::model::FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::Value<'__wire_repr_wire>)
            }
            _ => unreachable!(),
        };
        let value = match &field.kind {
            crate::derive::model::FieldKind::Fixed(Codec::OwnedBytes(length)) => quote! {
                match <&'__wire_repr_wire [u8; #length]>::try_from(bytes) {
                    Ok(bytes) => bytes,
                    Err(_) => unreachable!("validated fixed byte array has its declared width"),
                }
            },
            crate::derive::model::FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::decode(bytes))
            }
            _ => unreachable!(),
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            #vis fn #field_name(&self) -> #return_type {
                let mut cursor = 0usize;
                #(#prior)*
                #geometry
                let width = <#codec as #runtime::FixedCodec>::WIDTH;
                let bytes = &self.bytes[cursor..cursor + width];
                #value
            }
        }
    });
    let decode_steps = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = fixed_codec(field, runtime);
        let geometry = decode_geometry(field, label, decode_error);
        let decode_source = position_sources[index].then(|| {
            quote! {
                let #field_name = <#codec as #runtime::FixedCodec>::decode(bytes);
            }
        });
        quote! {
            #geometry
            let width = <#codec as #runtime::FixedCodec>::WIDTH;
            let available = remaining.len();
            let Some((bytes, suffix)) = remaining.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort {
                    field: #label,
                    required: width,
                    available,
                });
            };
            #decode_source
            remaining = suffix;
        }
    });
    let field_validators = fields.iter().flat_map(|field| {
        let name = &field.name;
        field
            .validators
            .iter()
            .map(move |validator| quote!(#validator(self.#name())?;))
    });

    quote! {
        /// A bytes-backed validated read view for this wire representation.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> {
            bytes: &'__wire_repr_wire [u8],
        }

        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            #[doc = "Returns this view's exact represented bytes."]
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }

            #[doc = "Returns a byte-selection root for this exact source representation."]
            #[must_use]
            #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }

            #constructor
            #(#getters)*
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
            type Segments<'__wire_repr_source> = ::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>>
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
            }
        }

        impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type DecodeError = #decode_error;

            fn parse_view(
                input: &'__wire_repr_wire [u8],
            ) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                let mut remaining = input;
                #(#decode_steps)*
                let represented = &input[..input.len() - remaining.len()];
                Ok((Self { bytes: represented }, remaining))
            }

            fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                #decode_error::TrailingBytes {
                    expected: represented,
                    actual: input,
                }
            }

            fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }
        }

        impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type ValidationError = #error_type;

            fn validate(&self) -> Result<(), Self::ValidationError> {
                #(#field_validators)*
                #(#model_validators(self)?;)*
                Ok(())
            }
        }

        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #decode_error;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
    }
}

fn fixed_codec(field: &Field, runtime: &TokenStream) -> TokenStream {
    match &field.kind {
        crate::derive::model::FieldKind::Fixed(codec) => super::super::codec_tokens(codec, runtime),
        _ => unreachable!(),
    }
}
