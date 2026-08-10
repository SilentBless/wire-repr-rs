//! Internal compiler implementation for `wire-repr` layout declarations.
//!
//! It owns canonical syntax parsing, normalized layout semantic IR,
//! and concrete Rust rendering.

#![deny(missing_docs)]

use proc_macro::TokenStream;

mod ir;
mod render;
mod syntax;

/// Declares sequential or fixed absolute-offset byte-backed wire layouts.
#[proc_macro]
pub fn wire_repr(input: TokenStream) -> TokenStream {
    match syn::parse(input).and_then(ir::normalize) {
        Ok(invocation) => render::render(invocation).into(),
        Err(error) => error.into_compile_error().into(),
    }
}
