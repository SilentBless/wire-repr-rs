//! Fixed-struct inherent API rendering.

use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) identity: Identity<'a>,
    pub(super) types: Types<'a>,
    pub(super) sequence: Sequence<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Identity<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) view: &'a Ident,
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
}

pub(super) struct Types<'a> {
    pub(super) wire_lifetime: Option<&'a syn::Lifetime>,
    pub(super) self_type: &'a TokenStream,
    pub(super) impl_generics: &'a TokenStream,
}

pub(super) struct Sequence<'a> {
    pub(super) plain: bool,
    pub(super) has_custom_validation_error: bool,
    pub(super) fixed_widths: &'a [TokenStream],
    pub(super) validation_error: Option<&'a syn::Type>,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        identity:
            Identity {
                vis,
                view,
                plan,
                encode_error,
            },
        types:
            Types {
                wire_lifetime,
                self_type,
                impl_generics,
            },
        sequence:
            Sequence {
                plain,
                has_custom_validation_error,
                fixed_widths,
                validation_error,
            },
        runtime,
    } = input;
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let view_signature = if wire_lifetime.is_some() {
        quote! {
            #vis fn view<'__wire_repr_view>(
                input: &'__wire_repr_view [u8],
            ) -> #view_request<'__wire_repr_view, #view<'__wire_repr_view>>
        }
    } else {
        quote! {
            #vis fn view<'__wire_repr_wire>(
                input: &'__wire_repr_wire [u8],
            ) -> #view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>
        }
    };
    let sequence_methods = plain.then(|| {
        if has_custom_validation_error {
            let validation_error = validation_error.expect("custom validation error is present");
            quote! {
                /// Structurally frames fixed-width items without running semantic validators.
                #vis fn unchecked_views<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> Result<
                    #runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>,
                    #runtime::FixedViewSequenceError,
                > {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                }

                /// Frames and semantically validates every fixed-width item before returning an infallible iterator.
                #vis fn views<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> Result<
                    #runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>,
                    #runtime::FixedValidatedViewSequenceError<#validation_error>,
                > {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    let iterator = #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                        .map_err(#runtime::FixedValidatedViewSequenceError::Framing)?;
                    for view in iterator.clone() {
                        #runtime::WireViewValidation::validate(&view)
                            .map_err(#runtime::FixedValidatedViewSequenceError::Item)?;
                    }
                    Ok(iterator)
                }
            }
        } else {
            quote! {
                /// Validates complete fixed-width sequence framing and returns an infallible iterator.
                #vis fn views<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> Result<
                    #runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>,
                    #runtime::FixedViewSequenceError,
                > {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                }
            }
        }
    });

    quote! {
        impl #impl_generics #self_type {
            #[doc = "Starts validating a bytes-backed read view from the supplied input."]
            #view_signature {
                #view_request::new(input)
            }

            #sequence_methods

            /// Returns a fail-closed cursor over consecutive representations.
            #vis fn cursor<'__wire_repr_view>(
                input: &'__wire_repr_view [u8],
            ) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                #runtime::ValidatedViewCursor::new(input)
            }

            #[doc = "Consumes this value and prepares an atomic encoding."]
            #vis fn prepare<'__wire_repr_value>(
                self,
            ) -> Result<#plan<'__wire_repr_value>, #encode_error>
            where
                Self: '__wire_repr_value,
            {
                <Self as #runtime::WireEncode>::prepare(self)
            }

            #[doc = "Consumes this value, prepares it, and commits it into `output`."]
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
    }
}
