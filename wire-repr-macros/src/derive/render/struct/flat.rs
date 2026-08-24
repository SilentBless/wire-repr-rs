//! Descriptor-backed dynamic views for flat byte-slice fields.

use super::super::super::model::{Codec, Field, FieldKind};
use super::{codec_tokens, interface};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct ViewInput<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
    pub(super) field_proxy: &'a Ident,
    pub(super) self_type: &'a TokenStream,
    pub(super) impl_generics: &'a TokenStream,
    pub(super) runtime: &'a TokenStream,
}

pub(super) fn render_view(input: ViewInput<'_>) -> TokenStream {
    let ViewInput {
        vis,
        fields,
        labels,
        view,
        decode_error,
        field_proxy,
        self_type,
        impl_generics,
        runtime,
    } = input;
    let state = format_ident!("{view}State");
    let descriptor = format_ident!("{view}Descriptor");
    let ranges_trait = format_ident!("{view}Ranges");
    let ranges: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}"))
        .collect();
    let range_starts: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}_start"))
        .collect();
    let range_ends: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}_end"))
        .collect();
    let byte_length_sources = fields
        .iter()
        .filter_map(|field| match &field.kind {
            FieldKind::Bytes { source } => Some(source),
            _ => None,
        })
        .collect::<Vec<_>>();
    let scalar_values = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Fixed(Codec::Builtin(codec)) = &field.kind else {
                return None;
            };
            Some((
                format_ident!("field_{index}_value"),
                codec_tokens(&Codec::Builtin(codec), runtime),
            ))
        })
        .collect::<Vec<_>>();
    let scalar_value_names = scalar_values
        .iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    let scalar_value_fields = scalar_values
        .iter()
        .map(|(name, codec)| quote!(#name: <#codec as #runtime::FixedCodec>::Value<'static>));
    let getters = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.name;
            let label = &labels[index];
            let range_start = &range_starts[index];
            let range_end = &range_ends[index];
            match &field.kind {
                FieldKind::Fixed(Codec::Builtin(codec)) => {
                    let codec = codec_tokens(&Codec::Builtin(codec), runtime);
                    let value = format_ident!("field_{index}_value");
                    quote! {
                        #[doc = concat!("Returns the decoded `", #label, "` field.")]
                        #[must_use]
                        #[inline(always)]
                        fn #name(&self) -> <#codec as #runtime::FixedCodec>::Value<'_> {
                            self.descriptor.#value
                        }
                    }
                }
                FieldKind::Fixed(codec) => {
                    let codec = codec_tokens(codec, runtime);
                    let (return_type, value) = match &field.kind {
                        FieldKind::Fixed(Codec::OwnedBytes(length)) => (
                            quote!(&[u8; #length]),
                            quote! {
                                <&[u8; #length]>::try_from(bytes)
                                    .expect("validated fixed byte array has its declared width")
                            },
                        ),
                        FieldKind::Fixed(Codec::Builtin(_) | Codec::Custom(_)) => (
                            quote!(<#codec as #runtime::FixedCodec>::Value<'_>),
                            quote!(<#codec as #runtime::FixedCodec>::decode(bytes)),
                        ),
                        _ => unreachable!(),
                    };
                    quote! {
                        #[doc = concat!("Returns the decoded `", #label, "` field.")]
                        #[must_use]
                        #[inline(always)]
                        fn #name(&self) -> #return_type {
                            let bytes = self.input.as_ref()
                                .get(self.descriptor.#range_start..self.descriptor.#range_end)
                                .unwrap_or_default();
                            #value
                        }
                    }
                }
                FieldKind::Bytes { .. } | FieldKind::Rest => quote! {
                    #[doc = concat!("Returns the decoded `", #label, "` field.")]
                    #[must_use]
                    #[inline(always)]
                    fn #name(&self) -> &[u8] {
                        self.input.as_ref()
                            .get(self.descriptor.#range_start..self.descriptor.#range_end)
                            .unwrap_or_default()
                    }
                },
                _ => unreachable!("flat renderer is selected only for fixed/rest fields"),
            }
        })
        .collect::<Vec<_>>();
    let getter_declarations = fields
        .iter()
        .map(|field| {
            let name = &field.name;
            let return_type = match &field.kind {
                FieldKind::Fixed(Codec::OwnedBytes(length)) => quote!(&[u8; #length]),
                FieldKind::Fixed(codec) => {
                    let codec = codec_tokens(codec, runtime);
                    quote!(<#codec as #runtime::FixedCodec>::Value<'_>)
                }
                FieldKind::Bytes { .. } | FieldKind::Rest => quote!(&[u8]),
                _ => unreachable!("flat renderer is selected only for fixed/rest fields"),
            };
            quote!(fn #name(&self) -> #return_type;)
        })
        .collect::<Vec<_>>();
    let range_methods: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("__wire_repr_field_{index}_range"))
        .collect();
    let range_method_declarations = range_methods
        .iter()
        .map(|method| quote!(fn #method(&self) -> ::core::ops::Range<usize>;));
    let range_method_impls = range_methods
        .iter()
        .zip(&range_starts)
        .zip(&range_ends)
        .map(|((method, start), end)| {
            quote! {
                #[inline(always)]
                fn #method(&self) -> ::core::ops::Range<usize> {
                    self.descriptor.#start..self.descriptor.#end
                }
            }
        });
    let framing = fields.iter().enumerate().map(|(index, field)| {
        let range = &ranges[index];
        let range_start = &range_starts[index];
        let range_end = &range_ends[index];
        let label = &labels[index];
        match &field.kind {
            FieldKind::Fixed(codec) => {
                let static_width = codec.static_width();
                let codec_tokens = codec_tokens(codec, runtime);
                let scalar_value = format_ident!("field_{index}_value");
                let scalar_decode = matches!(codec, Codec::Builtin(_)).then(|| {
                    let source = &field.name;
                    let source_alias = byte_length_sources
                        .contains(&source)
                        .then(|| quote!(let #source = #scalar_value;));
                    quote! {
                        let #scalar_value = <#codec_tokens as #runtime::FixedCodec>::decode(#range);
                        #source_alias
                    }
                });
                if let Some(width) = static_width {
                    quote! {
                        let start = input.len() - remaining.len();
                        let available = remaining.len();
                        let Some((#range, suffix)) = remaining.split_first_chunk::<#width>() else {
                            return Err(#decode_error::InputTooShort {
                                field: #label,
                                required: #width,
                                available,
                            });
                        };
                        let #range_start = start;
                        let #range_end = start + #width;
                        #scalar_decode
                        remaining = suffix;
                    }
                } else {
                    quote! {
                        let width = <#codec_tokens as #runtime::FixedCodec>::WIDTH;
                        let start = input.len() - remaining.len();
                        let available = remaining.len();
                        let Some((#range, suffix)) = remaining.split_at_checked(width) else {
                            return Err(#decode_error::InputTooShort {
                                field: #label,
                                required: width,
                                available,
                            });
                        };
                        let #range_start = start;
                        let #range_end = start + width;
                        remaining = suffix;
                    }
                }
            }
            FieldKind::Bytes { source } => quote! {
                let required = usize::try_from(#source).map_err(|_| {
                    #decode_error::LengthNotRepresentable { field: #label }
                })?;
                let start = input.len() - remaining.len();
                let available = remaining.len();
                let Some((#range, suffix)) = remaining.split_at_checked(required) else {
                    return Err(#decode_error::InputTooShort {
                        field: #label,
                        required,
                        available,
                    });
                };
                let #range_start = start;
                let #range_end = start + required;
                remaining = suffix;
            },
            FieldKind::Rest => quote! {
                let #range_start = input.len() - remaining.len();
                let #range_end = input.len();
                remaining = &[];
            },
            _ => unreachable!("flat renderer is selected only for flat byte-slice fields"),
        }
    });
    let compatibility = if cfg!(feature = "bytes") {
        quote! {
            impl #runtime::WireView<'static> for #state<#runtime::__private::Bytes> {
                type DecodeError = #decode_error;
                fn parse_view(input: #runtime::__private::Bytes) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    let (descriptor, remaining) = Self::__wire_repr_frame(input.as_ref())?;
                    let represented_len = input.len() - remaining.len();
                    let suffix = input.slice(represented_len..);
                    let input = input.slice(..represented_len);
                    Ok((Self { input, descriptor }, suffix))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }
                fn as_bytes(&self) -> &[u8] { self.input.as_ref() }
            }
            impl #runtime::WireViewValidation<'static> for #state<#runtime::__private::Bytes> {
                type ValidationError = #decode_error;
                fn validate(&self) -> Result<(), Self::ValidationError> { Ok(()) }
            }
            impl #impl_generics #runtime::WireViewType for #self_type {
                type DecodeError<'__wire_repr_view> = #decode_error;
                type View<'__wire_repr_view> = #state<#runtime::__private::Bytes>;
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_view> #runtime::WireView<'__wire_repr_view> for #state<&'__wire_repr_view [u8]> {
                type DecodeError = #decode_error;
                fn parse_view(input: &'__wire_repr_view [u8]) -> Result<(Self, &'__wire_repr_view [u8]), Self::DecodeError> {
                    let (descriptor, remaining) = Self::__wire_repr_frame(input)?;
                    Ok((Self { input, descriptor }, remaining))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }
                fn as_bytes(&self) -> &'__wire_repr_view [u8] { self.input }
            }
            impl<'__wire_repr_view> #runtime::WireViewValidation<'__wire_repr_view> for #state<&'__wire_repr_view [u8]> {
                type ValidationError = #decode_error;
                fn validate(&self) -> Result<(), Self::ValidationError> { Ok(()) }
            }
            impl #impl_generics #runtime::WireViewType for #self_type {
                type DecodeError<'__wire_repr_view> = #decode_error;
                type View<'__wire_repr_view> = #state<&'__wire_repr_view [u8]>;
            }
        }
    };

    quote! {
        /// A lifetime-free bytes-backed read view for this wire representation.
        #vis trait #view:
            #ranges_trait + #runtime::ByteSource + #runtime::ByteSourceCursor
        {
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

        #[doc(hidden)]
        #vis trait #ranges_trait {
            #(#range_method_declarations)*
        }

        #[doc(hidden)]
        #[derive(Clone, Copy, Debug)]
        #vis struct #descriptor {
            represented_len: usize,
            #(#range_starts: usize,)*
            #(#range_ends: usize,)*
            #(#scalar_value_fields,)*
        }

        /// Concrete backing state for [`#view`].
        #[doc(hidden)]
        #[derive(Debug)]
        #vis struct #state<T: ::core::convert::AsRef<[u8]>> {
            input: T,
            descriptor: #descriptor,
        }

        impl<T: ::core::convert::AsRef<[u8]> + Clone> Clone for #state<T> {
            fn clone(&self) -> Self {
                Self { input: self.input.clone(), descriptor: self.descriptor.clone() }
            }
        }

        impl<T: ::core::convert::AsRef<[u8]> + Copy> Copy for #state<T> {}

        impl<T: ::core::convert::AsRef<[u8]>> #state<T> {
            #[doc(hidden)]
            #[inline(always)]
            #vis fn as_bytes(&self) -> &[u8] {
                self.input.as_ref()
                    .get(..self.descriptor.represented_len)
                    .unwrap_or_default()
            }

            #[inline(always)]
            fn __wire_repr_frame<'input>(input: &'input [u8]) -> Result<(#descriptor, &'input [u8]), #decode_error> {
                let mut remaining = input;
                #(#framing)*
                Ok((
                    #descriptor {
                        represented_len: input.len() - remaining.len(),
                        #(#range_starts,)*
                        #(#range_ends,)*
                        #(#scalar_value_names,)*
                    },
                    remaining,
                ))
            }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #view for #state<T> {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                #state::as_bytes(self)
            }
            #(#getters)*
        }

        impl<T: ::core::convert::AsRef<[u8]>> #ranges_trait for #state<T> {
            #(#range_method_impls)*
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSource for #state<T> {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.as_bytes().len() }
            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { sink.write(self.as_bytes()); }
        }

        impl<T: ::core::convert::AsRef<[u8]>> #runtime::ByteSourceCursor for #state<T> {
            type Segments<'source> = ::core::iter::Once<#runtime::ByteSegment<'source>> where Self: 'source;
            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> { ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes())) }
            type Bytes<'source> = ::core::iter::Copied<::core::slice::Iter<'source, u8>> where Self: 'source;
            #[inline(always)]
            fn bytes(&self) -> Self::Bytes<'_> { self.as_bytes().iter().copied() }
        }

        #compatibility
    }
}

pub(super) struct ReadInput<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
    pub(super) runtime: &'a TokenStream,
}

pub(super) fn render_read(input: ReadInput<'_>) -> interface::ReadFragments {
    let ReadInput {
        vis,
        view,
        decode_error,
        runtime,
    } = input;
    let state = format_ident!("{view}State");
    let cursor = format_ident!("{view}Cursor");
    let unchecked_cursor = format_ident!("{view}UncheckedCursor");
    interface::ReadFragments {
        request_declarations: quote!(),
        request_impls: quote!(),
        inherent_methods: quote! {
            /// Validates one complete bytes-backed read view.
            #vis fn view<T: ::core::convert::AsRef<[u8]>>(input: T) -> Result<impl #view, #decode_error> {
                let (descriptor, remaining) = #state::<&[u8]>::__wire_repr_frame(input.as_ref())?;
                if !remaining.is_empty() {
                    return Err(#decode_error::TrailingBytes {
                        expected: descriptor.represented_len,
                        actual: input.as_ref().len(),
                    });
                }
                Ok(#state { input, descriptor })
            }

            /// Returns a fail-closed cursor over one contiguous borrowed input.
            #vis const fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor<'__wire_repr_view> {
                #cursor { remaining: input }
            }
        },
        view_type_impl: quote! {
            /// A fail-closed borrowed cursor over consecutive representations.
            #[derive(Clone, Copy, Debug)]
            #vis struct #cursor<'__wire_repr_view> { remaining: &'__wire_repr_view [u8] }

            impl<'__wire_repr_view> #cursor<'__wire_repr_view> {
                #[must_use]
                #vis const fn remaining(&self) -> &'__wire_repr_view [u8] { self.remaining }
                #[must_use]
                #vis const fn unchecked(self) -> #unchecked_cursor<'__wire_repr_view> {
                    #unchecked_cursor { remaining: self.remaining }
                }
                #[allow(clippy::should_implement_trait)]
                #vis fn next(&mut self) -> Result<Option<impl #view + '__wire_repr_view>, #runtime::ViewCursorError<#decode_error>> {
                    if self.remaining.is_empty() { return Ok(None); }
                    let (descriptor, remaining) = #state::<&[u8]>::__wire_repr_frame(self.remaining)
                        .map_err(#runtime::ViewCursorError::Item)?;
                    if descriptor.represented_len == 0 {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    let input = self.remaining;
                    self.remaining = remaining;
                    Ok(Some(#state { input, descriptor }))
                }
            }

            /// A borrowed cursor which performs structural framing only.
            #[derive(Clone, Copy, Debug)]
            #vis struct #unchecked_cursor<'__wire_repr_view> { remaining: &'__wire_repr_view [u8] }

            impl<'__wire_repr_view> #unchecked_cursor<'__wire_repr_view> {
                #[must_use]
                #vis const fn remaining(&self) -> &'__wire_repr_view [u8] { self.remaining }
                #[allow(clippy::should_implement_trait)]
                #vis fn next(&mut self) -> Result<Option<impl #view + '__wire_repr_view>, #runtime::ViewCursorError<#decode_error>> {
                    if self.remaining.is_empty() { return Ok(None); }
                    let (descriptor, remaining) = #state::<&[u8]>::__wire_repr_frame(self.remaining)
                        .map_err(#runtime::ViewCursorError::Item)?;
                    if descriptor.represented_len == 0 {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    let input = self.remaining;
                    self.remaining = remaining;
                    Ok(Some(#state { input, descriptor }))
                }
            }
        },
    }
}
