//! Bitfield decode and encode error rendering.

use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) enum Kind {
    Decode,
    Encode,
}

pub(super) struct Input<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) name: &'a Ident,
    pub(super) kind: Kind,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input { vis, name, kind } = input;

    match kind {
        Kind::Decode => quote! {
            /// Typed structural failures for this bitfield representation.
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            #vis enum #name {
                /// The storage scalar did not fit in the input.
                InputTooShort {
                    /// Required storage width.
                    required: usize,
                    /// Available input bytes.
                    available: usize,
                },
                /// Input had bytes after the complete bitfield representation.
                TrailingBytes {
                    /// Represented byte count.
                    expected: usize,
                    /// Supplied byte count.
                    actual: usize,
                },
            }

            impl ::core::fmt::Display for #name {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::InputTooShort { required, available } => write!(
                            formatter,
                            "bitfield storage needs {required} bytes, but only {available} remain",
                        ),
                        Self::TrailingBytes { expected, actual } => write!(
                            formatter,
                            "input has {} trailing bytes after the {expected}-byte bitfield representation",
                            actual.saturating_sub(*expected),
                        ),
                    }
                }
            }

            impl ::core::error::Error for #name {}
        },
        Kind::Encode => quote! {
            /// Typed preparation failures for this nominal bitfield.
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            #vis enum #name {
                /// A semantic projection does not fit its declared bit range.
                FieldOutOfRange {
                    /// Projection field name.
                    field: &'static str,
                    /// Supplied semantic value.
                    value: u128,
                    /// Declared encoded width.
                    width: u32,
                },
            }

            impl ::core::fmt::Display for #name {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::FieldOutOfRange { field, value, width } => write!(
                            formatter,
                            "bitfield projection `{field}` value {value} does not fit in {width} bits",
                        ),
                    }
                }
            }

            impl ::core::error::Error for #name {}
        },
    }
}
