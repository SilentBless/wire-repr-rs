//! Bitfield PLAN and encoding rendering.

use super::super::super::model::BitfieldField;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) mode: Mode<'a>,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) name: &'a Ident,
    pub(super) fields: &'a [BitfieldField],
}

pub(super) struct Types<'a> {
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
    pub(super) codec: &'a Ident,
    pub(super) storage_type: &'a Ident,
}

pub(super) struct Mode<'a> {
    pub(super) runtime: &'a TokenStream,
    pub(super) owned_mode: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema: Schema { vis, name, fields },
        types:
            Types {
                plan,
                encode_error,
                codec,
                storage_type,
            },
        mode: Mode {
            runtime,
            owned_mode,
        },
    } = input;
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
    let commit = if owned_mode {
        quote! {
            impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> {
                type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;
                fn commit_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut #runtime::__private::BytesMut,
                ) -> Result<Self::Written<'__wire_repr_output>, #runtime::OutputTooShortError> {
                    let appended = #runtime::ByteSource::append_into_bytes_mut(&self, output)?;
                    Ok(#runtime::Written::new(appended))
                }
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> {
                type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;
                fn commit_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<(
                    Self::Written<'__wire_repr_output>,
                    &'__wire_repr_output mut [u8],
                ), #runtime::OutputTooShortError> {
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

    quote! {
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

        impl<'__wire_repr_value> #runtime::ByteSource for #plan<'__wire_repr_value> {
            #[inline(always)]
            fn byte_len(&self) -> usize {
                <#runtime::#codec as #runtime::FixedCodec>::WIDTH
            }

            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                #runtime::ByteSource::emit_to(&self.storage, sink);
            }
        }

        impl<'__wire_repr_value> #runtime::ByteSourceCursor for #plan<'__wire_repr_value>
        where
            <#runtime::#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>: #runtime::ByteSourceCursor,
        {
            type Segments<'__wire_repr_source> = <<#runtime::#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                #runtime::ByteSourceCursor::segments(&self.storage)
            }

            type Bytes<'__wire_repr_source> = <<#runtime::#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Bytes<'__wire_repr_source>
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn bytes(&self) -> Self::Bytes<'_> {
                #runtime::ByteSourceCursor::bytes(&self.storage)
            }
        }

        #commit

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
    }
}
