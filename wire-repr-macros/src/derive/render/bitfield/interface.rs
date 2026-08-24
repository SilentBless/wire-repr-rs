//! Bitfield inherent interface rendering.

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) types: Types<'a>,
    pub(super) surface: Surface<'a>,
}

pub(super) struct Types<'a> {
    pub(super) name: &'a Ident,
    pub(super) view: &'a Ident,
    pub(super) plan: &'a Ident,
    pub(super) error: &'a Ident,
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
                error,
                encode_error,
                codec,
            },
        surface: Surface {
            vis,
            runtime,
            owned_mode,
        },
    } = input;
    let state = format_ident!("{view}State");
    let read_interface = quote! {
        /// Validates one complete bitfield representation.
        #vis fn view<T: ::core::convert::AsRef<[u8]>>(input: T) -> Result<impl #view, #error> {
            let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
            let available = input.as_ref().len();
            if available < width {
                return Err(#error::InputTooShort { required: width, available });
            }
            if available > width {
                return Err(#error::TrailingBytes { expected: width, actual: available });
            }
            Ok(#state { input })
        }

        /// Validates complete fixed-width sequence framing and returns an infallible iterator.
        #vis fn views(
            input: &[u8],
        ) -> Result<
            impl ::core::iter::ExactSizeIterator<Item = impl #view>
                + ::core::iter::DoubleEndedIterator
                + ::core::iter::FusedIterator,
            #runtime::FixedViewSequenceError,
        > {
            let width = <#runtime::#codec as #runtime::FixedCodec>::WIDTH;
            if width == 0 {
                return Err(#runtime::FixedViewSequenceError::InvalidItemWidth);
            }
            let chunks = input.chunks_exact(width);
            let trailing = chunks.remainder().len();
            if trailing != 0 {
                return Err(#runtime::FixedViewSequenceError::TrailingPartialItem {
                    item_width: width,
                    trailing,
                });
            }
            Ok(chunks.map(#state::from_sequence))
        }
    };

    let write_interface = if owned_mode {
        quote! {
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
    } else {
        quote! {
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
    };

    quote! {
        impl #name {
            #read_interface
            #write_interface
        }
    }
}
