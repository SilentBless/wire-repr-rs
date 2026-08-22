//! Enum derive rendering.

use super::super::model::{EnumTag, UnknownPolicy, Variant, VariantSelector, WireEnum};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireEnum, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireEnum {
        vis,
        name,
        wire_lifetime,
        tag,
        unknown,
        operation_input,
        variants,
    } = model;
    let operation = operation_input.as_ref().map(|input| &input.ty);
    let operation_error = operation_input
        .as_ref()
        .and_then(|input| input.error.as_ref());
    let plan = format_ident!("{name}Plan");
    let field_proxy = format_ident!("{name}Fields");
    let view = format_ident!("{name}View");
    let view_variant = format_ident!("__{name}ViewVariant");
    let view_input_request = format_ident!("{name}ViewInputRequest");
    let view_request = format_ident!("{name}ViewRequest");
    let unchecked_view_request = format_ident!("{name}UncheckedViewRequest");
    let cursor_input_request = format_ident!("{name}CursorInputRequest");
    let cursor = format_ident!("{name}Cursor");
    let unchecked_cursor = format_ident!("{name}UncheckedCursor");
    let encode_request = format_ident!("{name}EncodeRequest");
    let operation_parse = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_parse_with_{}", input.name));
    let operation_prepare = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_prepare_with_{}", input.name));
    let decode_error = format_ident!("{name}DecodeError");
    let validation_error = format_ident!("{name}ValidationError");
    let encode_error = format_ident!("{name}EncodeError");
    let (tag_codec, tag_type, byte_tag_width) = match &tag {
        EnumTag::Integer(tag) => {
            let codec = format_ident!("{}", tag.codec);
            let ty = format_ident!("{}", tag.ty);
            let codec = if tag.builtin {
                quote!(#runtime::#codec)
            } else {
                quote!(#codec)
            };
            (codec, quote!(#ty), None)
        }
        EnumTag::Bytes { width } => (
            quote!(#runtime::__private::OwnedBytes<#width>),
            quote!([u8; #width]),
            Some(*width),
        ),
    };
    let preserves_unknown = matches!(unknown, UnknownPolicy::Preserve);
    let unknown_variant = variants
        .iter()
        .find(|variant| matches!(variant.selector, VariantSelector::Unknown));
    let operation_input_ty = operation;
    let uses_operation_input = operation_input_ty.is_some();
    let has_body = variants.iter().any(|variant| {
        variant.body.is_some() && !matches!(variant.selector, VariantSelector::Unknown)
    });
    let static_view_request = if has_body {
        quote!(#runtime::ValidatedViewRequest)
    } else {
        quote!(#runtime::ViewRequest)
    };
    let view_variant_has_lifetime = has_body || (preserves_unknown && byte_tag_width.is_some());
    let view_variant_decl_generics = view_variant_has_lifetime.then(|| quote!(<'__wire_repr_wire>));
    let view_variant_type = if view_variant_has_lifetime {
        quote!(#view_variant<'__wire_repr_wire>)
    } else {
        quote!(#view_variant)
    };
    let body_view_paths: Vec<_> = variants
        .iter()
        .map(|variant| {
            if matches!(variant.selector, VariantSelector::Unknown) {
                Ok(None)
            } else {
                variant
                    .body
                    .as_ref()
                    .map(super::generated_view_path)
                    .transpose()
            }
        })
        .collect::<syn::Result<_>>()?;
    let (
        impl_generics,
        self_type,
        encode_error_type,
        encode_error_impl_type,
        plan_decl_generics,
        plan_decl_where,
        plan_type,
        plan_impl_generics,
        plan_impl_type,
        view_signature,
    ) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#encode_error<#lifetime>),
            quote!(#encode_error<'_>),
            quote!(<#lifetime, '__wire_repr_value>),
            quote!(where #lifetime: '__wire_repr_value),
            quote!(#plan<#lifetime, '__wire_repr_value>),
            quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
            quote!(#plan<#lifetime, '__wire_repr_value>),
            quote!(
                #vis fn view<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #static_view_request<'__wire_repr_view, #view<'__wire_repr_view>>
            ),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#encode_error),
            quote!(#encode_error),
            quote!(<'__wire_repr_value>),
            quote!(),
            quote!(#plan<'__wire_repr_value>),
            quote!(<'__wire_repr_value>),
            quote!(#plan<'__wire_repr_value>),
            quote!(
                #vis fn view<'__wire_repr_wire>(
                    input: &'__wire_repr_wire [u8],
                ) -> #static_view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>
            ),
        )
    };
    let (
        decode_error_decl_generics,
        view_error_type,
        association_error_type,
        decode_error_impl_type,
    ) = if has_body || byte_tag_width.is_some() || operation_input.is_some() {
        (
            quote!(<'__wire_repr_wire>),
            quote!(#decode_error<'__wire_repr_wire>),
            quote!(#decode_error<'__wire_repr_view>),
            quote!(#decode_error<'_>),
        )
    } else {
        (
            quote!(),
            quote!(#decode_error),
            quote!(#decode_error),
            quote!(#decode_error),
        )
    };
    let validation_error_type = if has_body {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#view_error_type)
    };
    let validation_error_impl_type = if has_body {
        quote!(#validation_error<'_>)
    } else {
        quote!(#view_error_type)
    };
    let static_cursor = if has_body {
        quote!(#runtime::ValidatedViewCursor)
    } else {
        quote!(#runtime::ViewCursor)
    };
    let encode_error_decl_generics = wire_lifetime
        .as_ref()
        .map_or_else(|| quote!(), |lifetime| quote!(<#lifetime>));
    let operation_view_signature = quote!(
        #vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_input_request<'__wire_repr_view>
    );
    let (operation_encode_decl_generics, operation_encode_value_type) =
        if let Some(lifetime) = &wire_lifetime {
            (
                quote!(<#lifetime, '__wire_repr_operation>),
                quote!(#name<#lifetime>),
            )
        } else {
            (quote!(<'__wire_repr_operation>), quote!(#name))
        };
    let operation_encode_request_type = if let Some(lifetime) = &wire_lifetime {
        quote!(#encode_request<#lifetime, '_>)
    } else {
        quote!(#encode_request<'_>)
    };

    let plan_variants = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) {
            quote! {
                #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
                #variant_name {
                    /// Prepared encoded tag.
                    tag: <#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                    /// Exact total encoded length.
                    encoded_len: usize,
                },
            }
        } else {
            match &variant.body {
                Some(body) => quote! {
                    #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
                    #variant_name {
                        /// Prepared encoded tag.
                        tag: <#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                        /// Prepared variant body.
                        body: <#body as #runtime::WireEncode>::Plan<'__wire_repr_value>,
                        /// Exact total encoded length.
                        encoded_len: usize,
                    },
                },
                None => quote! {
                    #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
                    #variant_name {
                        /// Prepared encoded tag.
                        tag: <#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                        /// Exact total encoded length.
                        encoded_len: usize,
                    },
                },
            }
        }
    });
    let plan_lengths = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        quote!(Self::#variant_name { encoded_len, .. } => *encoded_len,)
    });
    let view_variants = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) {
            if byte_tag_width.is_some() {
                quote!(#variant_name(&'__wire_repr_wire #tag_type),)
            } else {
                quote!(#variant_name(#tag_type),)
            }
        } else {
            match &variant.body {
                Some(_) => {
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    quote!(#variant_name(#body_view<'__wire_repr_wire>),)
                }
                None => quote!(#variant_name,),
            }
        }
    });
    let view_getters = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        let method = snake_case(variant_name);
        if matches!(variant.selector, VariantSelector::Unknown) {
            let return_type = if byte_tag_width.is_some() {
                quote!(&'__wire_repr_wire #tag_type)
            } else {
                quote!(#tag_type)
            };
            quote! {
                #[doc = concat!("Returns the raw tag captured by `", stringify!(#variant_name), "`.")]
                #[must_use]
                #vis fn #method(&self) -> Option<#return_type> {
                    match self.variant {
                        #view_variant::#variant_name(tag) => Some(tag),
                        _ => None,
                    }
                }
            }
        } else {
            match &variant.body {
                Some(_) => {
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    quote! {
                        #[doc = concat!("Returns the `", stringify!(#variant_name), "` body view when selected.")]
                        #[must_use]
                        #vis fn #method(&self) -> Option<#body_view<'__wire_repr_wire>> {
                            match self.variant {
                                #view_variant::#variant_name(body) => Some(body),
                                _ => None,
                            }
                        }
                    }
                }
                None => {
                    let method = format_ident!("is_{}", method);
                    quote! {
                        #[doc = concat!("Returns whether the `", stringify!(#variant_name), "` variant is selected.")]
                        #[must_use]
                        #vis fn #method(&self) -> bool {
                            matches!(self.variant, #view_variant::#variant_name)
                        }
                    }
                }
            }
        }
    });
    let decode_variants = variants.iter().enumerate().filter_map(|(index, variant)| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                let body_view = body_view_paths[index]
                    .as_ref()
                    .expect("body variants have generated view paths");
                let error = quote!(<#body_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::DecodeError);
                quote! {
                    #[doc = concat!("Nested decode error for variant `", stringify!(#variant_name), "`.")]
                    #variant_name(#error),
                }
            })
        }
    });
    let encode_variants = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|body| {
                let variant_name = &variant.name;
                quote! {
                    #[doc = concat!("Nested preparation error for variant `", stringify!(#variant_name), "`.")]
                    #variant_name(<#body as #runtime::WireEncode>::EncodeError),
                }
            })
        }
    });
    let decode_display_arms = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                quote!(Self::#variant_name(error) => write!(formatter, "wire decode failed in variant `{}`: {error}", stringify!(#variant_name)),)
            })
        }
    });
    let validation_variants = variants.iter().enumerate().filter_map(|(index, variant)| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                let body_view = body_view_paths[index]
                    .as_ref()
                    .expect("body variants have generated view paths");
                quote! {
                    /// Nested semantic validation failed in this variant body.
                    #variant_name(<#body_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError),
                }
            })
        }
    });
    let validation_display_arms = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                quote!(Self::#variant_name(error) => write!(formatter, "nested validation failed in variant `{}`: {error}", stringify!(#variant_name)),)
            })
        }
    });
    let validation_arms = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                quote!(#view_variant::#variant_name(body) => <_ as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&body).map_err(#validation_error::#variant_name),)
            })
        }
    });
    let validation_error_decl = has_body.then(|| quote! {
        /// Semantic validation failures for this tagged wire enum.
        #[derive(Debug)]
        #vis enum #validation_error<'__wire_repr_wire> {
            /// Structural framing failed.
            Decode(#view_error_type),
            #(#validation_variants)*
        }

        impl ::core::fmt::Display for #validation_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::Decode(error) => error.fmt(formatter),
                    #(#validation_display_arms)*
                }
            }
        }

        impl ::core::error::Error for #validation_error_impl_type {}

        impl<'__wire_repr_wire> From<#view_error_type> for #validation_error<'__wire_repr_wire> {
            fn from(error: #view_error_type) -> Self { Self::Decode(error) }
        }
    });
    let encode_display_arms = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            None
        } else {
            variant.body.as_ref().map(|_| {
                let variant_name = &variant.name;
                quote!(Self::#variant_name(error) => write!(formatter, "wire preparation failed for variant `{}`: {error}", stringify!(#variant_name)),)
            })
        }
    });
    let decode_arms: Vec<_> = variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| !matches!(variant.selector, VariantSelector::Unknown))
        .map(|(index, variant)| {
            decode_arm(
                variant,
                body_view_paths[index].as_ref(),
                &view_variant,
                &decode_error,
                &tag_type,
                uses_operation_input,
                runtime,
            )
        })
        .collect();
    let prepare_arms: Vec<_> = variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.name;
            if matches!(variant.selector, VariantSelector::Unknown) {
                quote! {
                    Self::#variant_name(raw_tag) => {
                        let tag = match <#tag_codec as #runtime::FixedCodec>::plan(raw_tag) {
                            Ok(plan) => plan,
                            Err(error) => match error {},
                        };
                        Ok(#plan::#variant_name {
                            tag,
                            encoded_len: <#tag_codec as #runtime::FixedCodec>::WIDTH,
                        })
                    }
                }
            } else {
                let prepare_tag = if uses_operation_input {
                    let selector = variant
                        .operation_selector
                        .as_ref()
                        .expect("dynamic operation selector");
                    quote! {
                        let raw_tag = operation
                            .encode(#selector)
                            .map_err(#encode_error::OperationMapping)?
                            .ok_or(#encode_error::SelectorUnavailable {
                                selector: stringify!(#selector),
                            })?;
                        let tag = match <#tag_codec as #runtime::FixedCodec>::plan(raw_tag) {
                            Ok(plan) => plan,
                            Err(error) => match error {},
                        };
                    }
                } else {
                    let tag_value = selector_value(&variant.selector, &tag_type);
                    quote! {
                        let tag = match <#tag_codec as #runtime::FixedCodec>::plan(#tag_value) {
                            Ok(plan) => plan,
                            Err(error) => match error {},
                        };
                    }
                };
                match &variant.body {
                    Some(body) => quote! {
                        Self::#variant_name(body) => {
                            #prepare_tag
                            let body = <#body as #runtime::WireEncode>::prepare(body)
                                .map_err(#encode_error::#variant_name)?;
                            let encoded_len = <#tag_codec as #runtime::FixedCodec>::WIDTH
                                .checked_add(#runtime::PreparedLayout::encoded_len(&body))
                                .ok_or(#encode_error::LengthOverflow)?;
                            Ok(#plan::#variant_name { tag, body, encoded_len })
                        }
                    },
                    None => quote! {
                        Self::#variant_name => {
                            #prepare_tag
                            Ok(#plan::#variant_name {
                                tag,
                                encoded_len: <#tag_codec as #runtime::FixedCodec>::WIDTH,
                            })
                        }
                    },
                }
            }
        })
        .collect();
    let selector_decode_variant = operation_error.map(|error| {
        quote! {
            /// The supplied operation input failed while resolving a raw selector.
            OperationMapping(#error),
        }
    });
    let selector_encode_variants = operation_error.map(|error| {
        quote! {
            /// The supplied operation input failed while resolving a selector.
            OperationMapping(#error),
            /// The supplied operation input has no canonical raw selector.
            SelectorUnavailable {
                /// The declared operation selector.
                selector: &'static str,
            },
        }
    });
    let selector_decode_display_arm = uses_operation_input.then(|| quote! {
        Self::OperationMapping(error) => write!(formatter, "operation input mapping failed while decoding: {error}"),
    });
    let selector_encode_display_arms = uses_operation_input.then(|| quote! {
        Self::OperationMapping(error) => write!(formatter, "operation input mapping failed while encoding: {error}"),
        Self::SelectorUnavailable { selector } => write!(formatter, "operation input has no canonical raw selector for `{selector}`"),
    });

    let emit_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) {
            quote! {
                Self::#variant_name { tag, .. } => {
                    #runtime::ByteSource::emit_to(tag, sink);
                }
            }
        } else {
            match &variant.body {
                Some(_) => quote! {
                    Self::#variant_name { tag, body, .. } => {
                        #runtime::ByteSource::emit_to(tag, sink);
                        #runtime::ByteSource::emit_to(body, sink);
                    }
                },
                None => quote! {
                    Self::#variant_name { tag, .. } => {
                        #runtime::ByteSource::emit_to(tag, sink);
                    }
                },
            }
        }
    });
    let plan_cursor = format_ident!("__{name}PlanCursor");
    let tag_plan = quote!(<#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>);
    let plan_cursor_bounds: Vec<_> = variants
        .iter()
        .filter_map(|variant| {
            (!matches!(variant.selector, VariantSelector::Unknown))
                .then_some(variant.body.as_ref())
                .flatten()
                .map(|body| quote!(<#body as #runtime::WireEncode>::Plan<'__wire_repr_value>: #runtime::ByteSourceCursor))
        })
        .collect();
    let plan_cursor_variants: Vec<_> = variants
        .iter()
        .filter_map(|variant| {
            if matches!(variant.selector, VariantSelector::Unknown) {
                return None;
            }
            variant.body.as_ref().map(|body| {
                let variant_name = &variant.name;
                quote!(
                    #variant_name {
                        tag: <#tag_plan as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>,
                        body: <<#body as #runtime::WireEncode>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>,
                    },
                )
            })
        })
        .collect();
    let plan_cursor_next_arms = variants.iter().filter_map(|variant| {
        if matches!(variant.selector, VariantSelector::Unknown) {
            return None;
        }
        variant.body.as_ref().map(|_| {
            let variant_name = &variant.name;
            quote!(Self::#variant_name { tag, body } => tag.next().or_else(|| body.next()),)
        })
    });
    let plan_segments_from_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) || variant.body.is_none() {
            quote!(
                Self::#variant_name { tag, .. } => #plan_cursor::Tag {
                    tag: #runtime::ByteSourceCursor::segments(tag),
                },
            )
        } else {
            quote!(
                Self::#variant_name { tag, body, .. } => #plan_cursor::#variant_name {
                    tag: #runtime::ByteSourceCursor::segments(tag),
                    body: #runtime::ByteSourceCursor::segments(body),
                },
            )
        }
    });
    let plan_cursor_impl_where = quote!(
        where
            #tag_plan: #runtime::ByteSourceCursor,
            #(#plan_cursor_bounds,)*
    );
    let (plan_cursor_decl_generics, plan_cursor_type, plan_cursor_definition_where) = if has_body {
        if let Some(lifetime) = &wire_lifetime {
            (
                quote!(<#lifetime, '__wire_repr_value, '__wire_repr_source>),
                quote!(#plan_cursor<#lifetime, '__wire_repr_value, '__wire_repr_source>),
                quote!(
                    where
                        #lifetime: '__wire_repr_value,
                        #lifetime: '__wire_repr_source,
                        '__wire_repr_value: '__wire_repr_source,
                        #(#plan_cursor_bounds,)*
                ),
            )
        } else {
            (
                quote!(<'__wire_repr_value, '__wire_repr_source>),
                quote!(#plan_cursor<'__wire_repr_value, '__wire_repr_source>),
                quote!(
                    where
                        '__wire_repr_value: '__wire_repr_source,
                        #(#plan_cursor_bounds,)*
                ),
            )
        }
    } else {
        (
            quote!(<'__wire_repr_value, '__wire_repr_source>),
            quote!(#plan_cursor<'__wire_repr_value, '__wire_repr_source>),
            quote!(where '__wire_repr_value: '__wire_repr_source),
        )
    };

    let static_tag_prelude = if let Some(width) = byte_tag_width {
        quote! {
            let available = input.len();
            let Some((tag, remaining)) = input.split_first_chunk::<#width>() else {
                return Err(#decode_error::InputTooShort {
                    required: #width,
                    available,
                });
            };
            let tag_bytes = &input[..#width];
        }
    } else {
        quote! {
            let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
            let available = input.len();
            let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort {
                    required: width,
                    available,
                });
            };
            let tag = <#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);
        }
    };
    let unknown_fallback = if let Some(variant) = unknown_variant {
        let variant_name = &variant.name;
        quote! {
            Ok((
                Self {
                    bytes: tag_bytes,
                    variant: #view_variant::#variant_name(tag),
                },
                remaining,
            ))
        }
    } else {
        let error_tag = if byte_tag_width.is_some() {
            quote!(*tag)
        } else {
            quote!(tag)
        };
        quote!(Err(#decode_error::UnknownTag { tag: #error_tag }))
    };
    let needs_decode_lifetime_marker =
        byte_tag_width.is_some() || (operation_input.is_some() && !has_body);
    let decode_lifetime_variant = needs_decode_lifetime_marker.then(|| {
        quote! {
            #[doc(hidden)]
            __WireLifetime(
                ::core::convert::Infallible,
                ::core::marker::PhantomData<&'__wire_repr_wire ()>,
            ),
        }
    });
    let decode_lifetime_display_arm = needs_decode_lifetime_marker
        .then(|| quote!(Self::__WireLifetime(never, _) => match *never {},));
    let unknown_decode_variant = matches!(unknown, UnknownPolicy::Reject).then(|| {
        quote! {
            /// The encoded tag does not identify a declared variant.
            UnknownTag {
                /// The decoded raw tag.
                tag: #tag_type,
            },
        }
    });
    let unknown_decode_display_arm = matches!(unknown, UnknownPolicy::Reject).then(
        || quote!(Self::UnknownTag { tag } => write!(formatter, "unknown wire tag {tag:?}"),),
    );

    let operation_view_helper = if let Some(operation_input_ty) = operation_input_ty {
        quote! {
            #[doc(hidden)]
            #vis fn #operation_parse(
                input: &'__wire_repr_wire [u8],
                operation: &#operation_input_ty,
            ) -> Result<(Self, &'__wire_repr_wire [u8]), #view_error_type> {
                let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
                let available = input.len();
                let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                    return Err(#decode_error::InputTooShort { required: width, available });
                };
                let tag = <#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);
                let selector = operation.decode(tag).map_err(#decode_error::OperationMapping)?;
                let Some(selector) = selector else {
                    return Err(#decode_error::UnknownTag { tag });
                };
                match selector {
                    #(#decode_arms)*
                    _ => Err(#decode_error::UnknownTag { tag }),
                }
            }
        }
    } else {
        quote!()
    };

    let view_impl = if operation_input_ty.is_some() {
        quote!()
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type DecodeError = #view_error_type;

                fn parse_view(
                    input: &'__wire_repr_wire [u8],
                ) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    #static_tag_prelude

                    match tag {
                        #(#decode_arms)*
                        _ => #unknown_fallback,
                    }
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
        }
    };

    let inherent_impl = if let Some(operation_input_ty) = operation_input_ty {
        let operation_name = &operation_input.as_ref().expect("operation input").name;
        quote! {
            #[doc(hidden)]
            #vis struct #view_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)]
            #vis struct #view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)]
            #vis struct #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)]
            #vis struct #cursor_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)]
            #vis struct #cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)]
            #vis struct #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)]
            #vis struct #encode_request #operation_encode_decl_generics { value: #operation_encode_value_type, operation: &'__wire_repr_operation #operation_input_ty }

            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #view_input_request<'__wire_repr_wire> {
                #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #view_request<'__wire_repr_wire, '_> { #view_request { input: self.input, operation } }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation> #view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis fn unchecked(self) -> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_view_request { input: self.input, operation: self.operation } }
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #validation_error_type> {
                    let (view, remainder) = #view::#operation_parse(self.input, self.operation).map_err(<#validation_error_type as From<#view_error_type>>::from)?;
                    #runtime::WireViewValidation::validate(&view)?;
                    Ok((view, remainder))
                }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #validation_error_type> {
                    let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() { Ok(view) } else { Err(<#validation_error_type as From<#view_error_type>>::from(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len })) }
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #view_error_type> { #view::#operation_parse(self.input, self.operation) }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #view_error_type> {
                    let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() { Ok(view) } else { Err(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len }) }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #cursor_input_request<'__wire_repr_wire> { #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #cursor<'__wire_repr_wire, '_> { #cursor { remaining: self.input, operation } } }
            impl<'__wire_repr_wire, '__wire_repr_operation> #cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #[must_use] #vis fn unchecked(self) -> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_cursor { remaining: self.remaining, operation: self.operation } }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#validation_error_type>> {
                    if self.remaining.is_empty() { return Ok(None); }
                    let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(|error| #runtime::ViewCursorError::Item(error.into()))?;
                    if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); }
                    #runtime::WireViewValidation::validate(&view).map_err(#runtime::ViewCursorError::Item)?;
                    self.remaining = suffix; Ok(Some(view))
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#view_error_type>> {
                    if self.remaining.is_empty() { return Ok(None); }
                    let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(#runtime::ViewCursorError::Item)?;
                    if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); }
                    self.remaining = suffix; Ok(Some(view))
                }
            }
            #[allow(missing_docs)]
            impl #operation_encode_decl_generics #encode_request #operation_encode_decl_generics {
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where #operation_encode_value_type: '__wire_repr_value { <#operation_encode_value_type>::#operation_prepare(self.value, self.operation) }
                #vis fn build_into<'__wire_repr_value, '__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> where #operation_encode_value_type: '__wire_repr_value { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) }
            }
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #operation_view_signature { #view_input_request { input } }
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor_input_request<'__wire_repr_view> { #cursor_input_request { input } }
                #vis fn #operation_name(self, operation: &#operation_input_ty) -> #operation_encode_request_type { #encode_request { value: self, operation } }
                #[doc(hidden)] #vis fn #operation_prepare<'__wire_repr_value>(self, operation: &#operation_input_ty) -> Result<#plan_type, #encode_error_type> where Self: '__wire_repr_value { match self { #(#prepare_arms)* } }
            }
        }
    } else {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature {
                    #static_view_request::new(input)
                }

                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #static_cursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                    #static_cursor::new(input)
                }

                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type>
                where
                    Self: '__wire_repr_value,
                {
                    <Self as #runtime::WireEncode>::prepare(self)
                }

                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<
                    (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                    #runtime::BuildIntoError<#encode_error_type>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output)
                        .map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    };

    let encode_impl = if operation_input_ty.is_some() {
        quote!()
    } else {
        quote! {
            impl #impl_generics #runtime::WireEncode for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;

                fn prepare<'__wire_repr_value>(
                    self,
                ) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
                where
                    Self: '__wire_repr_value,
                {
                    match self {
                        #(#prepare_arms)*
                    }
                }
            }
        }
    };

    Ok(quote! {
        /// Typed decoding failures for this tagged wire enum.
        #[derive(Debug)]
        #vis enum #decode_error #decode_error_decl_generics {
            /// The tag did not fit in the remaining input.
            InputTooShort {
                /// The required tag width.
                required: usize,
                /// The available byte count.
                available: usize,
            },
            #unknown_decode_variant
            #decode_lifetime_variant
            #selector_decode_variant
            #(#decode_variants)*
            /// Input had bytes after the complete representation.
            TrailingBytes {
                /// The represented byte count.
                expected: usize,
                /// The supplied byte count.
                actual: usize,
            },
        }

        impl ::core::fmt::Display for #decode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { required, available } => {
                        let required_unit = if *required == 1 { "byte" } else { "bytes" };
                        let available_unit = if *available == 1 { "byte" } else { "bytes" };
                        let available_verb = if *available == 1 { "remains" } else { "remain" };
                        write!(formatter, "tag needs {required} {required_unit}, but only {available} {available_unit} {available_verb}")
                    }
                    #unknown_decode_display_arm
                    #decode_lifetime_display_arm
                    #selector_decode_display_arm
                    #(#decode_display_arms)*
                    Self::TrailingBytes { expected, actual } => {
                        let trailing = actual.saturating_sub(*expected);
                        let unit = if trailing == 1 { "byte" } else { "bytes" };
                        write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation")
                    }
                }
            }
        }

        impl ::core::error::Error for #decode_error_impl_type {}

        #validation_error_decl

        #[derive(Clone, Copy, Debug)]
        enum #view_variant #view_variant_decl_generics {
            #(#view_variants)*
        }

        /// A bytes-backed validated read view for this tagged wire enum.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> {
            bytes: &'__wire_repr_wire [u8],
            variant: #view_variant_type,
        }

        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            /// Returns this view's exact represented bytes.
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }

            #(#view_getters)*
            #operation_view_helper
        }

        #view_impl

        impl<'__wire_repr_wire> #runtime::ByteSource for #view<'__wire_repr_wire> {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.as_bytes().len() }

            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { sink.write(self.as_bytes()); }
        }

        impl<'__wire_repr_wire> #runtime::ByteSourceCursor for #view<'__wire_repr_wire> {
            type Segments<'__wire_repr_source> = ::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>> where Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
            }
        }

        impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type ValidationError = #validation_error_type;
            fn validate(&self) -> Result<(), Self::ValidationError> {
                match self.variant {
                    #(#validation_arms)*
                    _ => Ok(()),
                }
            }
        }

        /// Generated field-selection proxy for this tagged wire enum.
        #[doc(hidden)]
        #vis struct #field_proxy<S: #runtime::MarkerScope = #runtime::RootScope>(::core::marker::PhantomData<fn() -> S>);
        impl<S: #runtime::MarkerScope> Copy for #field_proxy<S> {}
        impl<S: #runtime::MarkerScope> Clone for #field_proxy<S> { fn clone(&self) -> Self { *self } }
        #[allow(missing_docs)]
        impl<S: #runtime::MarkerScope> #field_proxy<S> { #[doc(hidden)] #vis fn __wire_repr_new() -> Self { Self(::core::marker::PhantomData) } }

        /// Typed encoding-preparation failures for this tagged wire enum.
        #[derive(Debug)]
        #vis enum #encode_error #encode_error_decl_generics {
            #selector_encode_variants
            #(#encode_variants)*
            /// The encoded length overflowed `usize`.
            LengthOverflow,
        }

        impl ::core::fmt::Display for #encode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #selector_encode_display_arms
                    #(#encode_display_arms)*
                    Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"),
                }
            }
        }

        impl ::core::error::Error for #encode_error_impl_type {}

        /// A prepared encoding for this tagged wire enum.
        #vis enum #plan #plan_decl_generics #plan_decl_where {
            #(#plan_variants)*
        }

        impl #plan_impl_generics #plan_impl_type {
            /// Returns the exact encoded byte count.
            #[must_use]
            #vis fn encoded_len(&self) -> usize {
                match self {
                    #(#plan_lengths)*
                }
            }
        }

        impl #plan_impl_generics #runtime::ByteSource for #plan_impl_type {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.encoded_len() }
            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                match self {
                    #(#emit_arms)*
                }
            }
        }

        #[doc(hidden)]
        #[allow(missing_docs)]
        #vis enum #plan_cursor #plan_cursor_decl_generics #plan_cursor_definition_where {
            Tag { tag: <#tag_plan as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source> },
            #(#plan_cursor_variants)*
        }

        impl #plan_cursor_decl_generics ::core::iter::Iterator for #plan_cursor_type #plan_cursor_definition_where {
            type Item = #runtime::ByteSegment<'__wire_repr_source>;

            #[inline(always)]
            fn next(&mut self) -> Option<Self::Item> {
                match self {
                    Self::Tag { tag } => tag.next(),
                    #(#plan_cursor_next_arms)*
                }
            }
        }

        impl #plan_impl_generics #runtime::ByteSourceCursor for #plan_impl_type
        #plan_cursor_impl_where
        {
            type Segments<'__wire_repr_source> = #plan_cursor_type where Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                match self {
                    #(#plan_segments_from_arms)*
                }
            }
        }

        impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
            type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;

            fn commit_into<'__wire_repr_output>(
                self,
                output: &'__wire_repr_output mut [u8],
            ) -> Result<
                (Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                #runtime::OutputTooShortError,
            > {
                let required = self.encoded_len();
                if output.len() < required {
                    return Err(#runtime::OutputTooShortError {
                        required,
                        available: output.len(),
                    });
                }
                let (bytes, suffix) = output.split_at_mut(required);
                #runtime::ByteSource::write_into(&self, bytes);
                Ok((#runtime::Written::new(bytes), suffix))
            }
        }

        #inherent_impl

        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }

        #encode_impl
    })
}

fn decode_arm(
    variant: &Variant,
    body_view: Option<&TokenStream>,
    view_variant: &proc_macro2::Ident,
    decode_error: &proc_macro2::Ident,
    tag_type: &TokenStream,
    uses_operation_input: bool,
    runtime: &TokenStream,
) -> TokenStream {
    let variant_name = &variant.name;
    let selector = if uses_operation_input {
        let selector = variant
            .operation_selector
            .as_ref()
            .expect("dynamic operation selector");
        quote!(value if value == #selector)
    } else {
        match &variant.selector {
            VariantSelector::Integer(value) => quote!(value if value == (#value as #tag_type)),
            VariantSelector::Bytes(bytes) => {
                let literal = proc_macro2::Literal::byte_string(bytes);
                quote!(value if value == #literal)
            }
            VariantSelector::Unknown | VariantSelector::Dynamic => {
                unreachable!("known static variants have concrete selectors")
            }
        }
    };

    match &variant.body {
        Some(_) => {
            let body_view = body_view.expect("body variants have generated view paths");
            quote! {
                #selector => {
                    let (body, suffix) = <#body_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::parse_view(remaining)
                        .map_err(#decode_error::#variant_name)?;
                    let represented = &input[..input.len() - suffix.len()];
                    Ok((
                        Self {
                            bytes: represented,
                            variant: #view_variant::#variant_name(body),
                        },
                        suffix,
                    ))
                }
            }
        }
        None => quote! {
            #selector => Ok((
                Self {
                    bytes: &input[..input.len() - remaining.len()],
                    variant: #view_variant::#variant_name,
                },
                remaining,
            )),
        },
    }
}

fn selector_value(selector: &VariantSelector, tag_type: &TokenStream) -> TokenStream {
    match selector {
        VariantSelector::Integer(value) => quote!(#value as #tag_type),
        VariantSelector::Bytes(bytes) => {
            let literal = proc_macro2::Literal::byte_string(bytes);
            quote!(*#literal)
        }
        VariantSelector::Unknown | VariantSelector::Dynamic => {
            unreachable!("static selector required")
        }
    }
}

fn snake_case(name: &proc_macro2::Ident) -> proc_macro2::Ident {
    let source = name.to_string();
    let mut value = String::new();
    for (index, character) in source.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                value.push('_');
            }
            value.extend(character.to_lowercase());
        } else {
            value.push(character);
        }
    }
    format_ident!("{value}")
}
