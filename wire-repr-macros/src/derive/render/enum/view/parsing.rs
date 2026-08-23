use super::super::super::super::model::{Variant, VariantSelector};
use super::{Geometry, Names, Types};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) names: &'a Names<'a>,
    pub(super) types: &'a Types<'a>,
    pub(super) geometry: &'a Geometry<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) operation_view_helper: TokenStream,
    pub(super) view_impl: TokenStream,
}

struct Context<'a> {
    names: &'a Names<'a>,
    types: &'a Types<'a>,
    geometry: &'a Geometry<'a>,
    runtime: &'a TokenStream,
}

struct StaticParse {
    tag_prelude: TokenStream,
    decode_arms: Vec<TokenStream>,
    unknown_fallback: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        names,
        types,
        geometry,
        runtime,
    } = input;
    let context = Context {
        names,
        types,
        geometry,
        runtime,
    };
    let static_parse = StaticParse {
        tag_prelude: static_tag_prelude(&context),
        decode_arms: decode_arms(&context),
        unknown_fallback: unknown_fallback(&context),
    };

    Output {
        operation_view_helper: operation_view_helper(&context, &static_parse.decode_arms),
        view_impl: view_impl(&context, &static_parse),
    }
}

fn decode_arms(context: &Context<'_>) -> Vec<TokenStream> {
    let decode_context = DecodeArmContext {
        view_variant: context.names.view_variant,
        decode_error: context.names.decode_error,
        tag_type: context.types.tag_type,
        uses_operation_input: context.geometry.operation.uses_input,
        owned: context.geometry.owned,
        runtime: context.runtime,
    };
    context
        .geometry
        .schema
        .variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| !matches!(variant.selector, VariantSelector::Unknown))
        .map(|(index, variant)| {
            decode_arm(
                variant,
                context.geometry.schema.body_view_paths[index].as_ref(),
                &decode_context,
            )
        })
        .collect()
}

fn static_tag_prelude(context: &Context<'_>) -> TokenStream {
    let tag_codec = context.types.tag_codec;
    let tag_type = context.types.tag_type;
    let decode_error = context.names.decode_error;
    let runtime = context.runtime;
    if context.geometry.owned {
        let decode = if let Some(width) = context.geometry.framing.byte_tag_width {
            quote! {
                let tag: #tag_type = input[..#width]
                    .try_into()
                    .expect("checked tag width");
            }
        } else {
            quote! {
                let tag = <#tag_codec as #runtime::FixedCodec>::decode(&input[..width]);
            }
        };
        quote! {
            let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
            let available = input.len();
            if available < width {
                return Err(#decode_error::InputTooShort { required: width, available });
            }
            #decode
            let tag_bytes = input.slice(..width);
            let remaining = input.slice(width..);
        }
    } else if let Some(width) = context.geometry.framing.byte_tag_width {
        quote! {
            let available = input.len();
            let Some((tag, remaining)) = input.split_first_chunk::<#width>() else {
                return Err(#decode_error::InputTooShort { required: #width, available });
            };
            let tag_bytes = &input[..#width];
        }
    } else {
        quote! {
            let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
            let available = input.len();
            let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort { required: width, available });
            };
            let tag = <#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);
        }
    }
}

fn unknown_fallback(context: &Context<'_>) -> TokenStream {
    let view_variant = context.names.view_variant;
    let decode_error = context.names.decode_error;
    if let Some(variant) = context.geometry.schema.unknown_variant {
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
        let error_tag =
            if context.geometry.framing.byte_tag_width.is_some() && !context.geometry.owned {
                quote!(*tag)
            } else {
                quote!(tag)
            };
        quote!(Err(#decode_error::UnknownTag { tag: #error_tag }))
    }
}

fn operation_view_helper(context: &Context<'_>, decode_arms: &[TokenStream]) -> TokenStream {
    let names = context.names;
    let types = context.types;
    let geometry = context.geometry;
    let runtime = context.runtime;
    let (Some(operation_input), Some(operation_parse)) =
        (geometry.operation.operation_input, names.operation_parse)
    else {
        return quote!();
    };
    let vis = names.vis;
    let tag_codec = types.tag_codec;
    let view_error = types.view_error;
    let decode_error = names.decode_error;
    if geometry.owned {
        quote! {
            #[doc(hidden)]
            #vis fn #operation_parse(
                input: #runtime::__private::Bytes,
                operation: &#operation_input,
            ) -> Result<(Self, #runtime::__private::Bytes), #view_error> {
                let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
                let available = input.len();
                if available < width {
                    return Err(#decode_error::InputTooShort { required: width, available });
                }
                let tag = <#tag_codec as #runtime::FixedCodec>::decode(&input[..width]);
                let remaining = input.slice(width..);
                let selector = operation
                    .decode(tag)
                    .map_err(#decode_error::OperationMapping)?;
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
        quote! {
            #[doc(hidden)]
            #vis fn #operation_parse(
                input: &'__wire_repr_wire [u8],
                operation: &#operation_input,
            ) -> Result<(Self, &'__wire_repr_wire [u8]), #view_error> {
                let width = <#tag_codec as #runtime::FixedCodec>::WIDTH;
                let available = input.len();
                let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                    return Err(#decode_error::InputTooShort { required: width, available });
                };
                let tag = <#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);
                let selector = operation
                    .decode(tag)
                    .map_err(#decode_error::OperationMapping)?;
                let Some(selector) = selector else {
                    return Err(#decode_error::UnknownTag { tag });
                };
                match selector {
                    #(#decode_arms)*
                    _ => Err(#decode_error::UnknownTag { tag }),
                }
            }
        }
    }
}

fn view_impl(context: &Context<'_>, static_parse: &StaticParse) -> TokenStream {
    let names = context.names;
    let types = context.types;
    let geometry = context.geometry;
    let runtime = context.runtime;
    let static_tag_prelude = &static_parse.tag_prelude;
    let decode_arms = &static_parse.decode_arms;
    let unknown_fallback = &static_parse.unknown_fallback;
    if geometry.operation.operation_input.is_some() {
        return quote!();
    }
    let view = names.view;
    let view_error = types.view_error;
    let decode_error = names.decode_error;
    if geometry.owned {
        quote! {
            impl #runtime::WireView<'static> for #view {
                type DecodeError = #view_error;

                fn parse_view(
                    input: #runtime::__private::Bytes,
                ) -> Result<(Self, #runtime::__private::Bytes), Self::DecodeError> {
                    #static_tag_prelude
                    match tag {
                        #(#decode_arms)*
                        _ => #unknown_fallback
                    }
                }

                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }

                fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire>
                for #view<'__wire_repr_wire>
            {
                type DecodeError = #view_error;

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
                    #decode_error::TrailingBytes { expected: represented, actual: input }
                }

                fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }
            }
        }
    }
}

pub(super) struct DecodeArmContext<'a> {
    pub(super) view_variant: &'a proc_macro2::Ident,
    pub(super) decode_error: &'a proc_macro2::Ident,
    pub(super) tag_type: &'a TokenStream,
    pub(super) uses_operation_input: bool,
    pub(super) owned: bool,
    pub(super) runtime: &'a TokenStream,
}

pub(super) fn decode_arm(
    variant: &Variant,
    body_view: Option<&TokenStream>,
    context: &DecodeArmContext<'_>,
) -> TokenStream {
    let DecodeArmContext {
        view_variant,
        decode_error,
        tag_type,
        uses_operation_input,
        owned,
        runtime,
    } = context;
    let variant_name = &variant.name;
    let selector = if *uses_operation_input {
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
                if *owned {
                    quote!(value if value == *#literal)
                } else {
                    quote!(value if value == #literal)
                }
            }
            VariantSelector::Unknown | VariantSelector::Dynamic => {
                unreachable!("known static variants have concrete selectors")
            }
        }
    };

    match &variant.body {
        Some(_) => {
            let body_view = body_view.expect("body variants have generated view paths");
            if *owned {
                return quote! {
                    #selector => {
                        let (body, suffix) = <#body_view as #runtime::WireView<'static>>::parse_view(remaining)
                            .map_err(#decode_error::#variant_name)?;
                        let represented = input.slice(..input.len() - suffix.len());
                        Ok((
                            Self {
                                bytes: represented,
                                variant: #view_variant::#variant_name(body),
                            },
                            suffix,
                        ))
                    }
                };
            }
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
        None if *owned => quote! {
            #selector => Ok((
                Self {
                    bytes: input.slice(..input.len() - remaining.len()),
                    variant: #view_variant::#variant_name,
                },
                remaining,
            )),
        },
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
