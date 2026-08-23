//! Enum prepared-plan rendering.

use crate::derive::model::{Variant, VariantSelector};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::Lifetime;

/// Inputs resolved by the enum renderer before plan generation.
pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) plan: Plan<'a>,
    pub(super) generics: Generics<'a>,
    pub(super) wire_lifetime: Option<&'a Lifetime>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) name: &'a Ident,
    pub(super) variants: &'a [Variant],
    pub(super) tag_codec: &'a TokenStream,
    pub(super) tag_type: &'a TokenStream,
}

pub(super) struct Plan<'a> {
    pub(super) uses_operation_input: bool,
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
}

pub(super) struct Generics<'a> {
    pub(super) plan_decl_generics: &'a TokenStream,
    pub(super) plan_decl_where: &'a TokenStream,
    pub(super) plan_impl_generics: &'a TokenStream,
    pub(super) plan_impl_type: &'a TokenStream,
}

/// Plan declarations plus the preparation arms still consumed by encode rendering.
pub(super) struct Output {
    pub(super) declaration: TokenStream,
    pub(super) prepare_arms: Vec<TokenStream>,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        schema,
        plan: generated_plan,
        generics,
        wire_lifetime,
        runtime,
    } = input;
    let Schema {
        vis,
        name,
        variants,
        tag_codec,
        tag_type,
    } = schema;
    let Plan {
        uses_operation_input,
        plan,
        encode_error,
    } = generated_plan;
    let Generics {
        plan_decl_generics,
        plan_decl_where,
        plan_impl_generics,
        plan_impl_type,
    } = generics;

    let plan_variants = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let body = (!matches!(variant.selector, VariantSelector::Unknown))
            .then_some(variant.body.as_ref())
            .flatten()
            .map(|body| {
                quote! {
                    /// Prepared variant body.
                    body: <#body as #runtime::WireEncode>::Plan<'__wire_repr_value>,
                }
            });
        quote! {
            #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
            #variant_name {
                /// Prepared encoded tag.
                tag: <#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                #body
                /// Exact total encoded length.
                encoded_len: usize,
            },
        }
    });
    let plan_lengths = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        quote! {
            Self::#variant_name { encoded_len, .. } => *encoded_len,
        }
    });
    let prepare_arms = variants
        .iter()
        .map(|variant| {
            prepare_arm(
                variant,
                tag_codec,
                tag_type,
                uses_operation_input,
                plan,
                encode_error,
                runtime,
            )
        })
        .collect();
    let emit_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        if !matches!(variant.selector, VariantSelector::Unknown) && variant.body.is_some() {
            quote! {
                Self::#variant_name { tag, body, .. } => {
                    #runtime::ByteSource::emit_to(tag, sink);
                    #runtime::ByteSource::emit_to(body, sink);
                }
            }
        } else {
            quote! {
                Self::#variant_name { tag, .. } => {
                    #runtime::ByteSource::emit_to(tag, sink);
                }
            }
        }
    });

    let plan_cursor = format_ident!("__{name}PlanCursor");
    let tag_plan = quote!(<#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>);
    let plan_cursor_bounds: Vec<_> = variants
        .iter()
        .filter(|variant| !matches!(variant.selector, VariantSelector::Unknown))
        .filter_map(|variant| variant.body.as_ref())
        .map(|body| {
            quote! {
                <#body as #runtime::WireEncode>::Plan<'__wire_repr_value>:
                    #runtime::ByteSourceCursor
            }
        })
        .collect();
    let plan_cursor_variants: Vec<_> = variants
        .iter()
        .filter(|variant| !matches!(variant.selector, VariantSelector::Unknown))
        .filter_map(|variant| variant.body.as_ref().map(|body| (variant, body)))
        .map(|(variant, body)| {
            let variant_name = &variant.name;
            quote! {
                #variant_name {
                    tag: <#tag_plan as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>,
                    body: <<#body as #runtime::WireEncode>::Plan<'__wire_repr_value>
                        as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>,
                },
            }
        })
        .collect();
    let plan_cursor_next_arms = variants
        .iter()
        .filter(|variant| {
            !matches!(variant.selector, VariantSelector::Unknown) && variant.body.is_some()
        })
        .map(|variant| {
            let variant_name = &variant.name;
            quote! {
                Self::#variant_name { tag, body } => tag.next().or_else(|| body.next()),
            }
        });
    let plan_segments_from_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) || variant.body.is_none() {
            quote! {
                Self::#variant_name { tag, .. } => #plan_cursor::Tag {
                    tag: #runtime::ByteSourceCursor::segments(tag),
                },
            }
        } else {
            quote! {
                Self::#variant_name { tag, body, .. } => #plan_cursor::#variant_name {
                    tag: #runtime::ByteSourceCursor::segments(tag),
                    body: #runtime::ByteSourceCursor::segments(body),
                },
            }
        }
    });
    let plan_cursor_impl_where = quote! {
        where
            #tag_plan: #runtime::ByteSourceCursor,
            #(#plan_cursor_bounds,)*
    };
    let has_body = variants.iter().any(|variant| {
        variant.body.is_some() && !matches!(variant.selector, VariantSelector::Unknown)
    });
    let (plan_cursor_decl_generics, plan_cursor_type, plan_cursor_definition_where) = if has_body {
        if let Some(lifetime) = wire_lifetime {
            (
                quote! { <#lifetime, '__wire_repr_value, '__wire_repr_source> },
                quote! { #plan_cursor<#lifetime, '__wire_repr_value, '__wire_repr_source> },
                quote! {
                    where
                        #lifetime: '__wire_repr_value,
                        #lifetime: '__wire_repr_source,
                        '__wire_repr_value: '__wire_repr_source,
                        #(#plan_cursor_bounds,)*
                },
            )
        } else {
            (
                quote! { <'__wire_repr_value, '__wire_repr_source> },
                quote! { #plan_cursor<'__wire_repr_value, '__wire_repr_source> },
                quote! {
                    where
                        '__wire_repr_value: '__wire_repr_source,
                        #(#plan_cursor_bounds,)*
                },
            )
        }
    } else {
        (
            quote! { <'__wire_repr_value, '__wire_repr_source> },
            quote! { #plan_cursor<'__wire_repr_value, '__wire_repr_source> },
            quote! {
                where
                    '__wire_repr_value: '__wire_repr_source
            },
        )
    };
    let commit_impl = if cfg!(feature = "bytes") {
        quote! {
            impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
                type Written<'output> = #runtime::Written<'output>;

                fn commit_into<'output>(
                    self,
                    output: &'output mut #runtime::__private::BytesMut,
                ) -> Result<Self::Written<'output>, #runtime::OutputTooShortError> {
                    let start = output.len();
                    #runtime::ByteSource::append_into_bytes_mut(&self, output)?;
                    Ok(#runtime::Written::new(&mut output[start..]))
                }
            }
        }
    } else {
        quote! {
            impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
                type Written<'output> = #runtime::Written<'output>;

                fn commit_into<'output>(
                    self,
                    output: &'output mut [u8],
                ) -> Result<
                    (Self::Written<'output>, &'output mut [u8]),
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
        }
    };

    Output {
        declaration: quote! {
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
                fn byte_len(&self) -> usize {
                    self.encoded_len()
                }

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
                Tag {
                    tag: <#tag_plan as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>,
                },
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

            impl #plan_impl_generics #runtime::ByteSourceCursor for #plan_impl_type #plan_cursor_impl_where {
                type Segments<'__wire_repr_source> = #plan_cursor_type where Self: '__wire_repr_source;

                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    match self {
                        #(#plan_segments_from_arms)*
                    }
                }
            }

            #commit_impl
        },
        prepare_arms,
    }
}

fn prepare_arm(
    variant: &Variant,
    tag_codec: &TokenStream,
    tag_type: &TokenStream,
    uses_operation_input: bool,
    plan: &Ident,
    encode_error: &Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let variant_name = &variant.name;
    if matches!(variant.selector, VariantSelector::Unknown) {
        return quote! {
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
        };
    }
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
        let tag_value = selector_value(&variant.selector, tag_type);
        quote! {
            let tag = match <#tag_codec as #runtime::FixedCodec>::plan(#tag_value) {
                Ok(plan) => plan,
                Err(error) => match error {},
            };
        }
    };
    match &variant.body {
        Some(body) => {
            quote! {
                Self::#variant_name(body) => {
                    #prepare_tag
                    let body = <#body as #runtime::WireEncode>::prepare(body)
                        .map_err(#encode_error::#variant_name)?;
                    let encoded_len = <#tag_codec as #runtime::FixedCodec>::WIDTH
                        .checked_add(#runtime::PreparedLayout::encoded_len(&body))
                        .ok_or(#encode_error::LengthOverflow)?;
                    Ok(#plan::#variant_name {
                        tag,
                        body,
                        encoded_len,
                    })
                }
            }
        }
        None => {
            quote! {
                Self::#variant_name => {
                    #prepare_tag
                    Ok(#plan::#variant_name {
                        tag,
                        encoded_len: <#tag_codec as #runtime::FixedCodec>::WIDTH,
                    })
                }
            }
        }
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
