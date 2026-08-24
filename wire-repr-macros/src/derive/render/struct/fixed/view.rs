//! Fixed-struct borrowed view rendering.

use super::geometry::{decode_geometry, getter_cursor_step, getter_geometry};
use crate::derive::model::{Codec, Field};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

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
    pub(super) inferred_error: Option<&'a Ident>,
    pub(super) inferred: &'a [super::super::Validator],
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
        validation:
            Validation {
                model_validators,
                error_type,
                inferred_error,
                inferred,
            },
        runtime,
    } = input;
    let state = format_ident!("{view}State");
    let getter_declarations = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = fixed_codec(field, runtime);
        let return_type = match &field.kind {
            crate::derive::model::FieldKind::Fixed(Codec::OwnedBytes(length)) => {
                quote!(&[u8; #length])
            }
            crate::derive::model::FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::Value<'_>)
            }
            _ => unreachable!(),
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            fn #field_name(&self) -> #return_type;
        }
    });
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let codec = fixed_codec(field, runtime);
        let prior = fields
            .iter()
            .take(index)
            .map(|prior| getter_cursor_step(prior, runtime));
        let geometry = getter_geometry(field, runtime);
        let return_type = match &field.kind {
            crate::derive::model::FieldKind::Fixed(Codec::OwnedBytes(length)) => {
                quote!(&[u8; #length])
            }
            crate::derive::model::FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::Value<'_>)
            }
            _ => unreachable!(),
        };
        let value = match &field.kind {
            crate::derive::model::FieldKind::Fixed(Codec::OwnedBytes(length)) => quote! {
                match <&[u8; #length]>::try_from(bytes) {
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
            #[inline(always)]
            fn #field_name(&self) -> #return_type {
                let mut cursor = 0usize;
                #(#prior)*
                #geometry
                let width = <#codec as #runtime::FixedCodec>::WIDTH;
                let bytes = &self.as_bytes()[cursor..cursor + width];
                #value
            }
        }
    });
    let decode_steps = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = fixed_codec(field, runtime);
        let geometry = decode_geometry(field, label, decode_error);
        let decode_source = position_sources[index].then(|| quote!(let #field_name = <#codec as #runtime::FixedCodec>::decode(bytes);));
        quote! {
            #geometry
            let width = <#codec as #runtime::FixedCodec>::WIDTH;
            let available = remaining.len();
            let Some((bytes, suffix)) = remaining.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort { field: #label, required: width, available });
            };
            #decode_source
            remaining = suffix;
        }
    });
    let mut inferred_validators = inferred.iter();
    let mut field_validators = Vec::new();
    for field in fields {
        let name = &field.name;
        for validator in &field.validators {
            if let Some(inferred_error) = inferred_error {
                let metadata = inferred_validators
                    .next()
                    .expect("inferred field validator metadata");
                let callback = &metadata.callback;
                let variant = &metadata.variant;
                field_validators
                    .push(quote!(#callback(self.#name()).map_err(#inferred_error::#variant)?;));
            } else {
                field_validators.push(quote!(#validator(self.#name())?;));
            }
        }
    }
    let mut model_validation = Vec::new();
    for validator in model_validators {
        if let Some(inferred_error) = inferred_error {
            let metadata = inferred_validators
                .next()
                .expect("inferred model validator metadata");
            let callback = &metadata.callback;
            let variant = &metadata.variant;
            model_validation.push(quote!(#callback(self).map_err(#inferred_error::#variant)?;));
        } else {
            model_validation.push(quote!(#validator(self)?;));
        }
    }
    debug_assert!(inferred_validators.next().is_none());

    let compatibility = if cfg!(feature = "bytes") {
        quote! {
            impl #runtime::WireView<'static> for #state<#runtime::__private::Bytes> {
                type DecodeError = #decode_error;
                #[inline(always)]
                fn parse_view(input: #runtime::__private::Bytes) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    let represented = Self::__wire_repr_frame(input.as_ref())?;
                    let suffix = input.slice(represented..);
                    let input = input.slice(..represented);
                    Ok((Self { input }, suffix))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }
                #[inline(always)]
                fn as_bytes(&self) -> &[u8] { self.input.as_ref() }
            }

            impl #runtime::WireViewValidation<'static> for #state<#runtime::__private::Bytes> {
                type ValidationError = #error_type;
                fn validate(&self) -> Result<(), Self::ValidationError> { self.__wire_repr_validate() }
            }

            impl #impl_generics #runtime::WireViewType for #self_type {
                type DecodeError<'__wire_repr_view> = #decode_error;
                type View<'__wire_repr_view> = #state<#runtime::__private::Bytes>;
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #state<&'__wire_repr_wire [u8]> {
                type DecodeError = #decode_error;
                #[inline(always)]
                fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let represented = Self::__wire_repr_frame(input)?;
                    Ok((Self { input: &input[..represented] }, &input[represented..]))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }
                #[inline(always)]
                fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.input }
            }

            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #state<&'__wire_repr_wire [u8]> {
                type ValidationError = #error_type;
                fn validate(&self) -> Result<(), Self::ValidationError> { self.__wire_repr_validate() }
            }

            impl #impl_generics #runtime::WireViewType for #self_type {
                type DecodeError<'__wire_repr_view> = #decode_error;
                type View<'__wire_repr_view> = #state<&'__wire_repr_view [u8]>;
            }
        }
    };

    quote! {
        /// A bytes-backed validated read view for this wire representation.
        #vis trait #view: #runtime::ByteSource + #runtime::ByteSourceCursor {
            #[doc = "Returns this view's exact represented bytes."]
            #[must_use]
            fn as_bytes(&self) -> &[u8];

            #[doc = "Returns a byte-selection root for this exact source representation."]
            #[must_use]
            fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>>
            where
                Self: Sized,
            {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }

            #(#getter_declarations)*
        }

        /// Concrete backing state for [`#view`].
        #[doc(hidden)]
        #[derive(Debug)]
        #vis struct #state<T: ::core::convert::AsRef<[u8]>> {
            input: T,
        }

        impl<T: ::core::convert::AsRef<[u8]> + Clone> Clone for #state<T> {
            fn clone(&self) -> Self { Self { input: self.input.clone() } }
        }
        impl<T: ::core::convert::AsRef<[u8]> + Copy> Copy for #state<T> {}

        impl<T: ::core::convert::AsRef<[u8]>> #state<T> {
            #[inline(always)]
            fn __wire_repr_frame(input_bytes: &[u8]) -> Result<usize, #decode_error> {
                let mut remaining = input_bytes;
                #(#decode_steps)*
                Ok(input_bytes.len() - remaining.len())
            }

            #[inline(always)]
            fn __wire_repr_validate(&self) -> Result<(), #error_type> {
                #(#field_validators)*
                #(#model_validation)*
                Ok(())
            }

            #[inline(always)]
            fn from_sequence(input: T) -> Self {
                Self { input }
            }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #view for #state<T> {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] { self.input.as_ref() }
            #(#getters)*
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSource for #state<T> {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.as_bytes().len() }
            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { sink.write(self.as_bytes()); }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSourceCursor for #state<T> {
            type Segments<'__wire_repr_source> = ::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>> where Self: '__wire_repr_source;
            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> { ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes())) }
            type Bytes<'__wire_repr_source> = ::core::iter::Copied<::core::slice::Iter<'__wire_repr_source, u8>> where Self: '__wire_repr_source;
            #[inline(always)]
            fn bytes(&self) -> Self::Bytes<'_> { self.as_bytes().iter().copied() }
        }

        #compatibility
    }
}

fn fixed_codec(field: &Field, runtime: &TokenStream) -> TokenStream {
    match &field.kind {
        crate::derive::model::FieldKind::Fixed(codec) => super::super::codec_tokens(codec, runtime),
        _ => unreachable!(),
    }
}
