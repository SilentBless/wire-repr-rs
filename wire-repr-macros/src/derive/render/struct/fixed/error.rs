//! Fixed-struct decode and encode error rendering.

use crate::derive::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) geometry: Geometry,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) variants: &'a [Ident],
}

pub(super) struct Types<'a> {
    pub(super) decode_error: &'a Ident,
    pub(super) encode_error: &'a Ident,
}

pub(super) struct Geometry {
    pub(super) has_positions: bool,
    pub(super) has_geometry: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema:
            Schema {
                vis,
                fields,
                labels,
                variants,
            },
        types: Types {
            decode_error,
            encode_error,
        },
        geometry: Geometry {
            has_positions,
            has_geometry,
        },
        runtime,
    } = input;

    let encode_variants =
        fields
            .iter()
            .zip(variants)
            .zip(labels)
            .map(|((field, variant), label)| {
                let codec = match &field.kind {
                    FieldKind::Fixed(codec) => super::super::codec_tokens(codec, runtime),
                    _ => unreachable!(),
                };
                quote! {
                    #[doc = concat!("Preparation error for field `", #label, "`.")]
                    #variant(<#codec as #runtime::FixedCodec>::EncodeError),
                }
            });
    let encode_display_arms = variants.iter().zip(labels).map(|(variant, label)| {
        quote! {
            Self::#variant(error) => {
                write!(formatter, "wire preparation failed for field `{}`: {error:?}", #label)
            }
        }
    });
    let decode_position_variants = has_positions.then(|| {
        quote! {
            PositionNotRepresentable {
                field: &'static str,
                value: u128,
            },
            PositionBeforeCursor {
                field: &'static str,
                position: usize,
                cursor: usize,
            },
        }
    });
    let decode_position_arms = has_positions.then(|| {
        quote! {
            Self::PositionNotRepresentable { field, value } => {
                write!(formatter, "position {value} for field `{field}` does not fit in usize")
            }
            Self::PositionBeforeCursor { field, position, cursor } => {
                write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}")
            }
        }
    });
    let encode_position_variants = decode_position_variants.clone();
    let encode_position_arms = decode_position_arms.clone();
    let decode_geometry_variant = has_geometry.then(|| {
        quote! {
            GeometryOverflow {
                field: &'static str,
            },
        }
    });
    let decode_geometry_arm = has_geometry.then(|| {
        quote! {
            Self::GeometryOverflow { field } => {
                write!(formatter, "placement before field `{field}` does not fit in usize")
            }
        }
    });

    quote! {
        /// Typed decoding failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #decode_error {
            InputTooShort {
                field: &'static str,
                required: usize,
                available: usize,
            },
            #decode_position_variants
            #decode_geometry_variant
            TrailingBytes {
                expected: usize,
                actual: usize,
            },
        }

        impl ::core::fmt::Display for #decode_error {
            fn fmt(
                &self,
                formatter: &mut ::core::fmt::Formatter<'_>,
            ) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { field, required, available } => {
                        let required_unit = if *required == 1 { "byte" } else { "bytes" };
                        let available_unit = if *available == 1 { "byte" } else { "bytes" };
                        let available_verb = if *available == 1 { "remains" } else { "remain" };
                        write!(formatter, "field `{field}` needs {required} {required_unit}, but only {available} {available_unit} {available_verb}")
                    }
                    #decode_position_arms
                    #decode_geometry_arm
                    Self::TrailingBytes { expected, actual } => {
                        let trailing = actual.saturating_sub(*expected);
                        let unit = if trailing == 1 { "byte" } else { "bytes" };
                        write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation")
                    }
                }
            }
        }

        impl ::core::error::Error for #decode_error {}

        /// Typed encoding-preparation failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #encode_error {
            #encode_position_variants
            #(#encode_variants)*
            LengthOverflow,
        }

        impl ::core::fmt::Display for #encode_error {
            fn fmt(
                &self,
                formatter: &mut ::core::fmt::Formatter<'_>,
            ) -> ::core::fmt::Result {
                match self {
                    #encode_position_arms
                    #(#encode_display_arms)*
                    Self::LengthOverflow => formatter.write_str(
                        "encoded representation length does not fit in usize",
                    ),
                }
            }
        }

        impl ::core::error::Error for #encode_error {}
    }
}
