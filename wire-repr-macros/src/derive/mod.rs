//! Derive frontend.

mod schema;

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

pub(crate) fn expand_view(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let runtime = runtime_path()?;
    schema::render_view(input, &runtime)
}

pub(crate) fn expand_builder(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let runtime = runtime_path()?;
    schema::render_builder(input, &runtime)
}

fn runtime_path() -> syn::Result<TokenStream> {
    match crate_name("wire-repr").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("failed to locate the wire-repr runtime: {error}"),
        )
    })? {
        FoundCrate::Itself => Ok(quote!(::wire_repr)),
        FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            Ok(quote!(::#name))
        }
    }
}
