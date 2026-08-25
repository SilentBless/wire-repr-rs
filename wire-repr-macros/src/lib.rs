//! Derive macro implementation for `wire-repr`.

#![deny(missing_docs)]

use proc_macro::TokenStream;

mod derive;
mod validator;

/// Marks a semantic validator so schema derives can infer its error type.
#[proc_macro_attribute]
pub fn validator(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    match syn::parse(input).and_then(validator::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Derives the schema-only exact-source read surface.
#[proc_macro_derive(WireView, attributes(wire))]
pub fn derive_wire_view(input: TokenStream) -> TokenStream {
    match syn::parse(input).and_then(derive::expand_view) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Derives the schema-only typestate builder surface.
#[proc_macro_derive(WireBuilder, attributes(wire))]
pub fn derive_wire_builder(input: TokenStream) -> TokenStream {
    match syn::parse(input).and_then(derive::expand_builder) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}
