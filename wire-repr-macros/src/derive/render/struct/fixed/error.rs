//! Fixed-struct decode and encode error rendering.

use super::super::Validator;
use crate::derive::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) geometry: Geometry,
    pub(super) validators: &'a [Validator],
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
    pub(super) validation_error: Option<&'a Ident>,
    pub(super) aggregate_error: Option<&'a Ident>,
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
        types:
            Types {
                decode_error,
                validation_error,
                aggregate_error,
                encode_error,
            },
        geometry: Geometry {
            has_positions,
            has_geometry,
        },
        validators,
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
                    #[error("wire preparation failed for field `{field}`: {0:?}", field = #label)]
                    #variant(<#codec as #runtime::FixedCodec>::EncodeError),
                }
            });
    let decode_position_variants = has_positions.then(|| {
        quote! {
            #[error("position {value} for field `{field}` does not fit in usize")]
            PositionNotRepresentable {
                field: &'static str,
                value: u128,
            },
            #[error("field `{field}` starts at byte {position}, before the current byte {cursor}")]
            PositionBeforeCursor {
                field: &'static str,
                position: usize,
                cursor: usize,
            },
        }
    });
    let encode_position_variants = decode_position_variants.clone();
    let decode_geometry_variant = has_geometry.then(|| {
        quote! {
            #[error("placement before field `{field}` does not fit in usize")]
            GeometryOverflow {
                field: &'static str,
            },
        }
    });
    let validation_declarations = match (validation_error, aggregate_error) {
        (Some(validation_error), Some(aggregate_error)) => {
            let error_assertions = super::super::validator_error_assertions(validators);
            let variants = validators.iter().map(|validator| {
                let variant = &validator.variant;
                let error = &validator.error;
                let callback = &validator.label;
                if let Some(field) = &validator.field {
                    let field = field.to_string();
                    quote! {
                        #[error(
                            "validator `{callback}` rejected field `{field}`: {0}",
                            callback = #callback,
                            field = #field,
                        )]
                        #variant(#[source] #error),
                    }
                } else {
                    quote! {
                        #[error(
                            "validator `{callback}` rejected the model: {0}",
                            callback = #callback,
                        )]
                        #variant(#[source] #error),
                    }
                }
            });
            Some(quote! {
                #error_assertions

                /// Typed semantic-validation failures for this wire representation.
                #[allow(missing_docs)]
                #[derive(Debug, #runtime::__private::ThisError)]
                #vis enum #validation_error {
                    #(#variants)*
                }

                /// Typed read failures for this wire representation.
                #[allow(missing_docs)]
                #[derive(Debug, #runtime::__private::ThisError)]
                #vis enum #aggregate_error {
                    #[error(transparent)]
                    Decode(#[from] #decode_error),
                    #[error(transparent)]
                    Validate(#[from] #validation_error),
                }
            })
        }
        (None, None) => None,
        _ => unreachable!("inferred validation error types are emitted together"),
    };

    quote! {
        /// Typed decoding failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #decode_error {
            #[error(
                "field `{field}` needs {required} {}, but only {available} {} {}",
                if *.required == 1 { "byte" } else { "bytes" },
                if *.available == 1 { "byte" } else { "bytes" },
                if *.available == 1 { "remains" } else { "remain" },
            )]
            InputTooShort {
                field: &'static str,
                required: usize,
                available: usize,
            },
            #decode_position_variants
            #decode_geometry_variant
            #[error(
                "input has {} trailing {} after the {expected}-byte representation",
                (*.actual).saturating_sub(*.expected),
                if (*.actual).saturating_sub(*.expected) == 1 { "byte" } else { "bytes" },
            )]
            TrailingBytes {
                expected: usize,
                actual: usize,
            },
        }

        #validation_declarations

        /// Typed encoding-preparation failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #encode_error {
            #encode_position_variants
            #(#encode_variants)*
            #[error("encoded representation length does not fit in usize")]
            LengthOverflow,
        }
    }
}
