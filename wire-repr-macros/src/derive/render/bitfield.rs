//! Nominal bitfield derive rendering.

use super::super::model::WireBitfield;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireBitfield, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireBitfield {
        vis,
        name,
        storage,
        fields,
    } = model;
    let view = format_ident!("{name}View");
    let plan = format_ident!("{name}Plan");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let codec = format_ident!("{}", storage.codec);
    let storage_type = format_ident!("{}", storage.ty);

    let getters = fields.iter().map(|field| {
        let field_name = &field.name;
        let field_type = &field.ty;
        let start = field.start;
        let width = field.end - field.start + 1;
        let value = if width == 1 {
            quote!(((raw >> #start) & 1) != 0)
        } else {
            let mask = if width == 128 {
                quote!(u128::MAX)
            } else {
                quote!(((1 as #storage_type) << #width) - 1)
            };
            quote!(((raw >> #start) & #mask) as #field_type)
        };
        quote! {
            #[doc = concat!("Returns the decoded `", stringify!(#field_name), "` projection.")]
            #[must_use]
            #vis fn #field_name(&self) -> #field_type {
                let raw = <#runtime::#codec as #runtime::FixedCodec>::decode(self.bytes);
                #value
            }
        }
    });

    let prepare_fields = fields.iter().map(|field| {
        let field_name = &field.name;
        let label = field_name.to_string();
        let start = field.start;
        let width = field.end - field.start + 1;
        if width == 1 {
            quote! {
                if self.#field_name {
                    raw |= (1 as #storage_type) << #start;
                }
            }
        } else {
            let max = if width == 128 {
                quote!(u128::MAX)
            } else {
                quote!((1u128 << #width) - 1)
            };
            quote! {
                let value = self.#field_name as u128;
                if value > #max {
                    return Err(#encode_error::FieldOutOfRange {
                        field: #label,
                        value,
                        width: #width,
                    });
                }
                raw |= (self.#field_name as #storage_type) << #start;
            }
        }
    });

    Ok(quote! {
        /// Typed structural failures for this bitfield representation.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #vis enum #decode_error {
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

        impl ::core::fmt::Display for #decode_error {
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

        impl ::core::error::Error for #decode_error {}

        /// A bytes-backed validated view of this nominal bitfield.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> {
            bytes: &'__wire_repr_wire [u8],
        }

        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            /// Returns the exact represented storage bytes.
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }

            fn from_sequence(bytes: &'__wire_repr_wire [u8]) -> Self {
                Self { bytes }
            }

            #(#getters)*
        }

        impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type DecodeError = #decode_error;

            fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
                let available = input.len();
                let Some((bytes, suffix)) = input.split_at_checked(width) else {
                    return Err(#decode_error::InputTooShort { required: width, available });
                };
                Ok((Self { bytes }, suffix))
            }

            fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                #decode_error::TrailingBytes { expected: represented, actual: input }
            }

            fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }
        }

        impl #runtime::WireViewType for #name {
            type DecodeError<'__wire_repr_wire> = #decode_error;
            type View<'__wire_repr_wire> = #view<'__wire_repr_wire>;
        }

        /// Typed preparation failures for this nominal bitfield.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #vis enum #encode_error {
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

        impl ::core::fmt::Display for #encode_error {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::FieldOutOfRange { field, value, width } => write!(
                        formatter,
                        "bitfield projection `{field}` value {value} does not fit in {width} bits",
                    ),
                }
            }
        }

        impl ::core::error::Error for #encode_error {}

        /// A prepared canonical bitfield encoding.
        #vis struct #plan<'__wire_repr_value> {
            storage: <#runtime::#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
        }

        impl #plan<'_> {
            /// Returns the exact encoded storage width.
            #[must_use]
            #vis fn encoded_len(&self) -> usize {
                <#runtime::#codec as #runtime::FixedCodec>::WIDTH
            }
        }

        impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> {
            type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;

            fn encoded_len(&self) -> usize {
                self.encoded_len()
            }

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
                #runtime::EncodePlan::write_into(&self.storage, bytes);
                Ok((#runtime::Written::new(bytes), suffix))
            }
        }

        impl #name {
            /// Starts validating this bitfield from the supplied input.
            #vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #runtime::ViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>> {
                #runtime::ViewRequest::new(input)
            }

            /// Validates complete fixed-width sequence framing and returns an infallible iterator.
            #vis fn views<'__wire_repr_wire>(
                input: &'__wire_repr_wire [u8],
            ) -> Result<
                #runtime::FixedViewIterator<'__wire_repr_wire, #view<'__wire_repr_wire>>,
                #runtime::FixedViewSequenceError,
            > {
                #runtime::FixedViewIterator::new(
                    input,
                    <#runtime::#codec as #runtime::FixedCodec>::WIDTH,
                    #view::from_sequence,
                )
            }

            /// Consumes this semantic value and prepares its canonical storage.
            #vis fn prepare(self) -> Result<#plan<'static>, #encode_error> {
                <Self as #runtime::WireEncode>::prepare(self)
            }

            /// Prepares and commits this bitfield into `output` atomically.
            #vis fn build_into<'__wire_repr_output>(
                self,
                output: &'__wire_repr_output mut [u8],
            ) -> Result<
                (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                #runtime::BuildIntoError<#encode_error>,
            > {
                let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                #runtime::PreparedLayout::commit_into(plan, output)
                    .map_err(#runtime::BuildIntoError::Output)
            }
        }

        impl #runtime::WireEncode for #name {
            type EncodeError = #encode_error;
            type Plan<'__wire_repr_value> = #plan<'__wire_repr_value> where Self: '__wire_repr_value;

            fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
            where
                Self: '__wire_repr_value,
            {
                let mut raw: #storage_type = 0;
                #(#prepare_fields)*
                let storage = match <#runtime::#codec as #runtime::FixedCodec>::plan(raw) {
                    Ok(plan) => plan,
                    Err(error) => match error {},
                };
                Ok(#plan { storage })
            }
        }
    })
}
