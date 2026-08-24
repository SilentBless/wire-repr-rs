//! Fixed-struct inherent API rendering.

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

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
    pub(super) self_type: &'a TokenStream,
    pub(super) impl_generics: &'a TokenStream,
}

pub(super) struct Sequence<'a> {
    pub(super) plain: bool,
    pub(super) has_validation: bool,
    pub(super) fixed_widths: &'a [TokenStream],
    pub(super) validation_error: &'a TokenStream,
    pub(super) decode_error: &'a Ident,
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
        types: Types {
            self_type,
            impl_generics,
        },
        sequence:
            Sequence {
                plain,
                has_validation,
                fixed_widths,
                validation_error,
                decode_error,
            },
        runtime,
    } = input;
    let state = format_ident!("{view}State");
    let view_cursor = format_ident!("{view}Cursor");
    let unchecked_cursor = format_ident!("{view}UncheckedCursor");
    let structural_error =
        quote!(<#validation_error as ::core::convert::From<#decode_error>>::from);
    let direct_view = quote! {
        /// Validates one complete bytes-backed read view.
        #vis fn view<T: ::core::convert::AsRef<[u8]>>(input: T) -> Result<impl #view, #validation_error> {
            let input_len = input.as_ref().len();
            let represented = #state::<&[u8]>::__wire_repr_frame(input.as_ref()).map_err(#structural_error)?;
            #state { input: &input.as_ref()[..represented] }.__wire_repr_validate()?;
            if represented != input_len {
                return Err(#structural_error(#decode_error::TrailingBytes {
                    expected: represented,
                    actual: input_len,
                }));
            }
            Ok(#state { input })
        }
    };
    let sequence_methods = plain.then(|| {
        let validation = has_validation.then(|| quote! {
            for view in input.chunks_exact(item_width).map(#state::from_sequence) {
                view.__wire_repr_validate()
                    .map_err(#runtime::FixedValidatedViewSequenceError::Item)?;
            }
        });
        let error_type = if has_validation {
            quote!(#runtime::FixedValidatedViewSequenceError<#validation_error>)
        } else {
            quote!(#runtime::FixedViewSequenceError)
        };
        let framing_error = if has_validation {
            quote!(#runtime::FixedValidatedViewSequenceError::Framing)
        } else {
            quote!(::core::convert::identity)
        };
        let unchecked = has_validation.then(|| quote! {
            /// Structurally frames fixed-width items without running semantic validators.
            #vis fn unchecked_views(input: &[u8]) -> Result<
                impl ::core::iter::ExactSizeIterator<Item = impl #view>
                    + ::core::iter::DoubleEndedIterator
                    + ::core::iter::FusedIterator,
                #runtime::FixedViewSequenceError,
            > {
                let item_width = 0usize #(+ #fixed_widths)*;
                if item_width == 0 { return Err(#runtime::FixedViewSequenceError::InvalidItemWidth); }
                let chunks = input.chunks_exact(item_width);
                let trailing = chunks.remainder().len();
                if trailing != 0 {
                    return Err(#runtime::FixedViewSequenceError::TrailingPartialItem { item_width, trailing });
                }
                Ok(chunks.map(#state::from_sequence))
            }
        });
        quote! {
            #unchecked
            /// Frames and validates every fixed-width item before returning an infallible iterator.
            #vis fn views(input: &[u8]) -> Result<
                impl ::core::iter::ExactSizeIterator<Item = impl #view>
                    + ::core::iter::DoubleEndedIterator
                    + ::core::iter::FusedIterator,
                #error_type,
            > {
                let item_width = 0usize #(+ #fixed_widths)*;
                if item_width == 0 {
                    return Err(#framing_error(#runtime::FixedViewSequenceError::InvalidItemWidth));
                }
                let chunks = input.chunks_exact(item_width);
                let trailing = chunks.remainder().len();
                if trailing != 0 {
                    return Err(#framing_error(#runtime::FixedViewSequenceError::TrailingPartialItem { item_width, trailing }));
                }
                #validation
                Ok(chunks.map(#state::from_sequence))
            }
        }
    });
    let write_interface = if cfg!(feature = "bytes") {
        quote! {
            /// Consumes this value and prepares an atomic encoding.
            #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan<'__wire_repr_value>, #encode_error>
            where Self: '__wire_repr_value,
            {
                <Self as #runtime::WireEncode>::prepare(self)
            }

            /// Consumes this value, prepares it, and commits it into `output`.
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
            /// Consumes this value and prepares an atomic encoding.
            #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan<'__wire_repr_value>, #encode_error>
            where Self: '__wire_repr_value,
            {
                <Self as #runtime::WireEncode>::prepare(self)
            }

            /// Consumes this value, prepares it, and commits it into `output`.
            #vis fn build_into<'__wire_repr_output>(
                self,
                output: &'__wire_repr_output mut [u8],
            ) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error>> {
                let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
            }
        }
    };
    let cursor_types = quote! {
        /// A fail-closed borrowed cursor over consecutive representations.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view_cursor<'__wire_repr_wire> {
            remaining: &'__wire_repr_wire [u8],
        }

        impl<'__wire_repr_wire> #view_cursor<'__wire_repr_wire> {
            /// Returns the unconsumed input.
            #[must_use]
            #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] {
                self.remaining
            }

            /// Skips semantic validation for the remaining items.
            #[must_use]
            #vis const fn unchecked(self) -> #unchecked_cursor<'__wire_repr_wire> {
                #unchecked_cursor { remaining: self.remaining }
            }

            /// Validates and returns the next item without consuming a failing representation.
            #[allow(clippy::should_implement_trait)]
            #vis fn next(&mut self) -> Result<Option<impl #view + '__wire_repr_wire>, #runtime::ViewCursorError<#validation_error>> {
                if self.remaining.is_empty() {
                    return Ok(None);
                }
                let represented = #state::<&[u8]>::__wire_repr_frame(self.remaining)
                    .map_err(|error| #runtime::ViewCursorError::Item(#structural_error(error)))?;
                if represented == 0 {
                    return Err(#runtime::ViewCursorError::EmptyItem);
                }
                let input = &self.remaining[..represented];
                let state = #state { input };
                state.__wire_repr_validate().map_err(#runtime::ViewCursorError::Item)?;
                self.remaining = &self.remaining[represented..];
                Ok(Some(state))
            }
        }

        /// A borrowed cursor which performs structural framing only.
        #[derive(Clone, Copy, Debug)]
        #vis struct #unchecked_cursor<'__wire_repr_wire> {
            remaining: &'__wire_repr_wire [u8],
        }

        impl<'__wire_repr_wire> #unchecked_cursor<'__wire_repr_wire> {
            /// Returns the unconsumed input.
            #[must_use]
            #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] {
                self.remaining
            }

            /// Structurally frames and returns the next item without consuming a failure.
            #[allow(clippy::should_implement_trait)]
            #vis fn next(&mut self) -> Result<Option<impl #view + '__wire_repr_wire>, #runtime::ViewCursorError<#decode_error>> {
                if self.remaining.is_empty() {
                    return Ok(None);
                }
                let represented = #state::<&[u8]>::__wire_repr_frame(self.remaining)
                    .map_err(#runtime::ViewCursorError::Item)?;
                if represented == 0 {
                    return Err(#runtime::ViewCursorError::EmptyItem);
                }
                let input = &self.remaining[..represented];
                self.remaining = &self.remaining[represented..];
                Ok(Some(#state { input }))
            }
        }
    };

    quote! {
        #cursor_types

        impl #impl_generics #self_type {
            #direct_view
            #sequence_methods

            /// Returns a fail-closed cursor over one contiguous borrowed input.
            #vis const fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_cursor<'__wire_repr_view> {
                #view_cursor { remaining: input }
            }

            #write_interface
        }
    }
}
