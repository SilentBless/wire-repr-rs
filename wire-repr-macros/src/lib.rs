//! Derive macro implementation for `wire-repr`.

#![deny(missing_docs)]

use proc_macro::TokenStream;

mod derive;

/// Derives wire decoding and prepared encoding for a supported Rust struct or enum.
#[proc_macro_derive(Wire, attributes(wire))]
pub fn derive_wire(input: TokenStream) -> TokenStream {
    match syn::parse(input).and_then(derive::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}
