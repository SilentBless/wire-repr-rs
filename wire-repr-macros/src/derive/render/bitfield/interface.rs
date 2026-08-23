//! Bitfield inherent interface rendering.

use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) types: Types<'a>,
    pub(super) surface: Surface<'a>,
}

pub(super) struct Types<'a> {
    pub(super) name: &'a Ident,
    pub(super) view: &'a Ident,
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
    pub(super) codec: &'a Ident,
}

pub(super) struct Surface<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) runtime: &'a TokenStream,
    pub(super) owned_mode: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        types:
            Types {
                name,
                view,
                plan,
                encode_error,
                codec,
            },
        surface: Surface {
            vis,
            runtime,
            owned_mode,
        },
    } = input;

    if owned_mode {
        quote! {
            impl #name {
                /// Starts validating this bitfield from the supplied input.
                #vis fn view(
                    input: #runtime::__private::Bytes,
                ) -> #runtime::ViewRequest<'static, #view> {
                    #runtime::ViewRequest::new(input)
                }
                /// Validates complete fixed-width sequence framing and returns an infallible iterator.
                #vis fn views(
                    input: #runtime::__private::Bytes,
                ) -> Result<#runtime::FixedViewIterator<'static, #view>, #runtime::FixedViewSequenceError> {
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
                    output: &'__wire_repr_output mut #runtime::__private::BytesMut,
                ) -> Result<#runtime::Written<'__wire_repr_output>, #runtime::BuildIntoError<#encode_error>> {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    } else {
        quote! {
            impl #name {
                /// Starts validating this bitfield from the supplied input.
                #vis fn view<'__wire_repr_wire>(
                    input: &'__wire_repr_wire [u8],
                ) -> #runtime::ViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>> {
                    #runtime::ViewRequest::new(input)
                }
                /// Validates complete fixed-width sequence framing and returns an infallible iterator.
                #vis fn views<'__wire_repr_wire>(
                    input: &'__wire_repr_wire [u8],
                ) -> Result<#runtime::FixedViewIterator<'__wire_repr_wire, #view<'__wire_repr_wire>>, #runtime::FixedViewSequenceError> {
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
                ) -> Result<(
                    #runtime::Written<'__wire_repr_output>,
                    &'__wire_repr_output mut [u8],
                ), #runtime::BuildIntoError<#encode_error>> {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    }
}
