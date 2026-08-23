//! Struct derive error rendering.

use super::super::super::model::{Field, FieldKind};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Lifetime, Visibility};

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) nested: Nested<'a>,
    pub(super) capabilities: Capabilities,
    pub(super) types: Types<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a Visibility,
    pub(super) wire_lifetime: Option<&'a Lifetime>,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) variants: &'a [Ident],
}

pub(super) struct Nested<'a> {
    pub(super) nested_view_paths: &'a [Option<TokenStream>],
    pub(super) nested_decode_error_paths: &'a [Option<TokenStream>],
    pub(super) nested_encode_error_paths: &'a [Option<TokenStream>],
}

pub(super) struct Capabilities {
    pub(super) has_positions: bool,
    pub(super) has_geometry: bool,
    pub(super) has_bytes: bool,
    pub(super) has_builder: bool,
    pub(super) has_computed: bool,
}

pub(super) struct Types<'a> {
    pub(super) decode_error: &'a Ident,
    pub(super) encode_error: &'a Ident,
    pub(super) decode_error_decl_generics: &'a TokenStream,
    pub(super) error_impl_type: &'a TokenStream,
    pub(super) encode_error_decl_generics: &'a TokenStream,
    pub(super) encode_error_impl_type: &'a TokenStream,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema:
            Schema {
                vis,
                wire_lifetime,
                fields,
                labels,
                variants,
            },
        nested:
            Nested {
                nested_view_paths,
                nested_decode_error_paths,
                nested_encode_error_paths,
            },
        capabilities:
            Capabilities {
                has_positions,
                has_geometry,
                has_bytes,
                has_builder,
                has_computed,
            },
        types:
            Types {
                decode_error,
                encode_error,
                decode_error_decl_generics,
                error_impl_type,
                encode_error_decl_generics,
                encode_error_impl_type,
            },
        runtime,
    } = input;
    let encode_lifetime_variant = wire_lifetime.map(|lifetime| {
        quote! {
            #[doc(hidden)]
            __WireLifetime(
                ::core::convert::Infallible,
                ::core::marker::PhantomData<&#lifetime ()>,
            ),
        }
    });
    let encode_lifetime_arm =
        wire_lifetime.map(|_| quote!(Self::__WireLifetime(value, _) => match *value {},));
    let decode_variants = fields
        .iter()
        .zip(variants)
        .zip(labels)
        .enumerate()
        .filter_map(|(index, ((field, variant), label))| match &field.kind {
            FieldKind::Nested => {
                let child_view = nested_view_paths[index]
                    .as_ref()
                    .expect("nested fields have generated view paths");
                let error = if field.operation_input.is_some() {
                    let error = nested_decode_error_paths[index]
                        .as_ref()
                        .expect("operation-backed nested fields have generated error paths");
                    quote!(#error)
                } else if cfg!(feature = "bytes") {
                    quote!(<#child_view as #runtime::WireView<'static>>::DecodeError)
                } else {
                    quote!(<#child_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::DecodeError)
                };
                Some(quote!(
                    #[doc = concat!("Nested decode error for field `", #label, "`.")]
                    #variant(#error),
                ))
            }
            FieldKind::Prefix(codec) => Some(quote!(
                #[doc = concat!("Prefix validation error for field `", #label, "`.")]
                #variant(<#codec as #runtime::PrefixCodec>::DecodeError),
            )),
            FieldKind::Fixed(_) | FieldKind::Bytes { .. } | FieldKind::Rest => None,
        });
    let decode_display_arms = fields.iter().zip(variants).zip(labels).filter_map(
        |((field, variant), label)| match field.kind {
            FieldKind::Nested => Some(quote!(Self::#variant(error) => write!(formatter, "wire decode failed in field `{}`: {error}", #label),)),
            FieldKind::Prefix(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire prefix validation failed in field `{}`: {error:?}", #label),)),
            FieldKind::Fixed(_) | FieldKind::Bytes { .. } | FieldKind::Rest => None,
        },
    );
    let encode_variants = fields.iter().zip(variants).zip(labels).enumerate().filter_map(
        |(index, ((field, variant), label))| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = super::codec_tokens(codec, runtime);
                Some(quote!(#[doc = concat!("Preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::FixedCodec>::EncodeError),))
            }
            FieldKind::Nested => {
                let ty = &field.ty;
                if field.operation_input.is_some() {
                    let error = nested_encode_error_paths[index]
                        .as_ref()
                        .expect("operation-backed nested fields have generated error paths");
                    Some(quote!(#[doc = concat!("Nested preparation error for field `", #label, "`.")] #variant(#error),))
                } else {
                    Some(quote!(#[doc = concat!("Nested preparation error for field `", #label, "`.")] #variant(<#ty as #runtime::WireEncode>::EncodeError),))
                }
            }
            FieldKind::Prefix(codec) => Some(quote! {
                #[doc = concat!("Prefix preparation error for field `", #label, "`.")]
                #variant(<#codec as #runtime::PrefixCodec>::EncodeError),
            }),
            FieldKind::Bytes { .. } | FieldKind::Rest => None,
        },
    );
    let encode_display_arms = fields.iter().zip(variants).zip(labels).filter_map(
        |((field, variant), label)| match field.kind {
            FieldKind::Fixed(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error:?}", #label),)),
            FieldKind::Nested => Some(quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error}", #label),)),
            FieldKind::Prefix(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire prefix preparation failed for field `{}`: {error:?}", #label),)),
            FieldKind::Bytes { .. } | FieldKind::Rest => None,
        },
    );
    let decode_position_variants = has_positions.then(|| {
        quote! {
            /// A decoded position does not fit this platform's address space.
            PositionNotRepresentable {
                /// The positioned field.
                field: &'static str,
                /// The source value that does not fit.
                value: u128,
            },
            /// A field position points before the current cursor.
            PositionBeforeCursor {
                /// The positioned field.
                field: &'static str,
                /// The requested absolute position.
                position: usize,
                /// The current representation cursor.
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
    let encode_position_variants = has_positions.then(|| {
        quote! {
            /// A requested position does not fit this platform's address space.
            PositionNotRepresentable {
                /// The positioned field.
                field: &'static str,
                /// The source value that does not fit.
                value: u128,
            },
            /// A requested position points before the current cursor.
            PositionBeforeCursor {
                /// The positioned field.
                field: &'static str,
                /// The requested absolute position.
                position: usize,
                /// The current representation cursor.
                cursor: usize,
            },
        }
    });
    let encode_position_arms = has_positions.then(|| {
        quote! {
            Self::PositionNotRepresentable { field, value } => {
                write!(formatter, "position {value} for field `{field}` does not fit in usize")
            }
            Self::PositionBeforeCursor { field, position, cursor } => {
                write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}")
            }
        }
    });
    let decode_geometry_variant = has_geometry.then(|| {
        quote! {
            /// The requested padding or alignment overflowed `usize`.
            GeometryOverflow {
                /// The field whose placement overflowed.
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
    let decode_length_variant = has_bytes.then(|| {
        quote! {
            /// A decoded byte length does not fit this platform's address space.
            LengthNotRepresentable {
                /// The byte field controlled by the source value.
                field: &'static str,
            },
        }
    });
    let decode_length_arm = has_bytes.then(|| {
        quote! {
            Self::LengthNotRepresentable { field } => {
                write!(formatter, "byte length for field `{field}` does not fit in usize")
            }
        }
    });
    let encode_missing_variant = has_builder.then(|| {
        quote! {
            /// A caller-owned builder field was not supplied.
            MissingField {
                /// The omitted field.
                field: &'static str,
            },
        }
    });
    let encode_missing_arm = has_builder.then(|| {
        quote! {
            Self::MissingField { field } => write!(formatter, "missing required field `{field}`"),
        }
    });
    let encode_computed_value_variant = has_computed.then(|| {
        quote! {
            /// A computed value does not fit the destination field's semantic type.
            ComputedValueNotRepresentable {
                /// The computed destination field.
                field: &'static str,
            },
        }
    });
    let encode_computed_value_arm = has_computed.then(|| {
        quote! {
            Self::ComputedValueNotRepresentable { field } => {
                write!(formatter, "computed value does not fit field `{field}`")
            }
        }
    });
    let encode_length_variant = has_bytes.then(|| {
        quote! {
            /// A byte field's length is not representable by its source field.
            LengthNotRepresentable {
                /// The byte field whose length could not be represented.
                field: &'static str,
                /// The physical source field that stores the length.
                source: &'static str,
                /// The requested byte count.
                length: usize,
            },
        }
    });
    let encode_length_arm = has_bytes.then(|| quote! {
        Self::LengthNotRepresentable { field, source, length } => {
            write!(formatter, "field `{field}` has {length} bytes, which do not fit in length field `{source}`")
        }
    });

    quote! {
        /// Typed decoding failures for this wire representation.
        #[derive(Debug)]
        #vis enum #decode_error #decode_error_decl_generics {
            /// A fixed-width field did not fit in the remaining input.
            InputTooShort {
                /// The source field that did not fit.
                field: &'static str,
                /// The required byte count.
                required: usize,
                /// The available byte count.
                available: usize,
            },
            #decode_position_variants
            #decode_geometry_variant
            #decode_length_variant
            #(#decode_variants)*
            /// Input had bytes after the complete representation.
            TrailingBytes {
                /// The represented byte count.
                expected: usize,
                /// The supplied byte count.
                actual: usize,
            },
        }
        impl ::core::fmt::Display for #error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { field, required, available } => {
                        let required_unit = if *required == 1 { "byte" } else { "bytes" };
                        let available_unit = if *available == 1 { "byte" } else { "bytes" };
                        let available_verb = if *available == 1 { "remains" } else { "remain" };
                        write!(formatter, "field `{field}` needs {required} {required_unit}, but only {available} {available_unit} {available_verb}")
                    }
                    #decode_position_arms
                    #decode_geometry_arm
                    #decode_length_arm
                    #(#decode_display_arms)*
                    Self::TrailingBytes { expected, actual } => {
                        let trailing = actual.saturating_sub(*expected);
                        let unit = if trailing == 1 { "byte" } else { "bytes" };
                        write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation")
                    }
                }
            }
        }
        impl ::core::error::Error for #error_impl_type {}

        /// Typed encoding-preparation failures for this wire representation.
        #[derive(Debug)]
        #vis enum #encode_error #encode_error_decl_generics {
            #encode_lifetime_variant
            #encode_position_variants
            #encode_missing_variant
            #encode_computed_value_variant
            #encode_length_variant
            #(#encode_variants)*
            /// The encoded length overflowed `usize`.
            LengthOverflow,
        }
        impl ::core::fmt::Display for #encode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #encode_lifetime_arm
                    #encode_position_arms
                    #encode_missing_arm
                    #encode_computed_value_arm
                    #encode_length_arm
                    #(#encode_display_arms)*
                    Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"),
                }
            }
        }
        impl ::core::error::Error for #encode_error_impl_type {}
    }
}
