use super::super::super::model::{UnknownPolicy, Variant, VariantSelector};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) owned: bool,
    pub(super) errors: Errors<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) variants: &'a [Variant],
    pub(super) body_view_paths: &'a [Option<TokenStream>],
    pub(super) unknown: UnknownPolicy,
    pub(super) tag_type: &'a TokenStream,
    pub(super) byte_tag_width: Option<usize>,
    pub(super) has_body: bool,
}

pub(super) struct Operation<'a> {
    pub(super) error: Option<&'a syn::Path>,
    pub(super) uses_input: bool,
}

pub(super) struct Errors<'a> {
    pub(super) decode: DecodeErrors<'a>,
    pub(super) validation: ValidationErrors<'a>,
    pub(super) encode: EncodeErrors<'a>,
}

pub(super) struct DecodeErrors<'a> {
    pub(super) decode_error: &'a proc_macro2::Ident,
    pub(super) decode_error_decl_generics: &'a TokenStream,
    pub(super) decode_error_impl_type: &'a TokenStream,
    pub(super) view_error_type: &'a TokenStream,
}

pub(super) struct ValidationErrors<'a> {
    pub(super) validation_error: &'a proc_macro2::Ident,
    pub(super) validation_error_impl_type: &'a TokenStream,
}

pub(super) struct EncodeErrors<'a> {
    pub(super) encode_error: &'a proc_macro2::Ident,
    pub(super) encode_error_decl_generics: &'a TokenStream,
    pub(super) encode_error_impl_type: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) before_view: TokenStream,
    pub(super) after_field_proxy: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        schema,
        operation,
        owned,
        errors,
        runtime,
    } = input;
    let Schema {
        vis,
        variants,
        body_view_paths,
        unknown,
        tag_type,
        byte_tag_width,
        has_body,
    } = schema;
    let Operation {
        error: operation_error,
        uses_input: uses_operation_input,
    } = operation;

    let Errors {
        decode,
        validation,
        encode,
    } = errors;
    let DecodeErrors {
        decode_error,
        decode_error_decl_generics,
        decode_error_impl_type,
        view_error_type,
    } = decode;
    let ValidationErrors {
        validation_error,
        validation_error_impl_type,
    } = validation;
    let EncodeErrors {
        encode_error,
        encode_error_decl_generics,
        encode_error_impl_type,
    } = encode;
    let decode_variants = variants.iter().enumerate().filter_map(|(index, variant)| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    let error = if owned {
                        quote!(
                            <<#body_view as #runtime::WireViewType>::View<'static>
                                as #runtime::WireView<'static>>::DecodeError
                        )
                    } else {
                        quote!(
                            <<#body_view as #runtime::WireViewType>::View<'__wire_repr_wire>
                                as #runtime::WireView<'__wire_repr_wire>>::DecodeError
                        )
                    };
                    quote! {
                        #[doc = concat!("Nested decode error for variant `", stringify!(#variant_name), "`.")]
                        #variant_name(#error),
                    }
                })
            })
            .flatten()
    });
    let decode_display_arms = variants.iter().filter_map(|variant| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    quote!(
                        Self::#variant_name(error) => write!(
                            formatter,
                            "wire decode failed in variant `{}`: {error}",
                            stringify!(#variant_name)
                        ),
                    )
                })
            })
            .flatten()
    });
    let validation_variants = variants.iter().enumerate().filter_map(|(index, variant)| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    if owned {
                        quote!(
                            /// Nested semantic validation failed in this variant body.
                            #variant_name(<<#body_view as #runtime::WireViewType>::View<'static> as #runtime::WireViewValidation<'static>>::ValidationError),
                        )
                    } else {
                        quote!(
                            /// Nested semantic validation failed in this variant body.
                            #variant_name(<<#body_view as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError),
                        )
                    }
                })
            })
            .flatten()
    });
    let validation_display_arms = variants.iter().filter_map(|variant| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    quote!(
                        Self::#variant_name(error) => write!(
                            formatter,
                            "nested validation failed in variant `{}`: {error}",
                            stringify!(#variant_name)
                        ),
                    )
                })
            })
            .flatten()
    });
    let validation_error_decl_type = if owned {
        quote!(#validation_error)
    } else {
        quote!(#validation_error<'__wire_repr_wire>)
    };
    let validation_error_from = if owned {
        quote! {
            impl From<#view_error_type> for #validation_error {
                fn from(error: #view_error_type) -> Self {
                    Self::Decode(error)
                }
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_wire> From<#view_error_type>
                for #validation_error<'__wire_repr_wire>
            {
                fn from(error: #view_error_type) -> Self {
                    Self::Decode(error)
                }
            }
        }
    };
    let validation_error_decl = has_body.then(|| {
        quote! {
            /// Semantic validation failures for this tagged wire enum.
            #[derive(Debug)]
            #vis enum #validation_error_decl_type {
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

            #validation_error_from
        }
    });
    let encode_variants = variants.iter().filter_map(|variant| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|body| {
                    let variant_name = &variant.name;
                    quote! {
                        #[doc = concat!("Nested preparation error for variant `", stringify!(#variant_name), "`.")]
                        #variant_name(<#body as #runtime::WireEncode>::EncodeError),
                    }
                })
            })
            .flatten()
    });
    let encode_display_arms = variants.iter().filter_map(|variant| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    quote!(
                        Self::#variant_name(error) => write!(
                            formatter,
                            "wire preparation failed for variant `{}`: {error}",
                            stringify!(#variant_name)
                        ),
                    )
                })
            })
            .flatten()
    });
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
    let selector_decode_display_arm = uses_operation_input.then(|| {
        quote! {
            Self::OperationMapping(error) => write!(
                formatter,
                "operation input mapping failed while decoding: {error}"
            ),
        }
    });
    let selector_encode_display_arms = uses_operation_input.then(|| {
        quote! {
            Self::OperationMapping(error) => write!(
                formatter,
                "operation input mapping failed while encoding: {error}",
            ),
            Self::SelectorUnavailable { selector } => write!(
                formatter,
                "operation input has no canonical raw selector for `{selector}`",
            ),
        }
    });
    let needs_decode_lifetime_marker =
        !owned && (byte_tag_width.is_some() || (uses_operation_input && !has_body));
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

    let before_view = quote! {
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
    };

    let after_field_proxy = quote! {
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
    };

    Output {
        before_view,
        after_field_proxy,
    }
}
