//! Derive frontend.

mod model;
mod render;

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

pub(crate) fn expand(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let runtime = match crate_name("wire-repr").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("failed to locate the wire-repr runtime: {error}"),
        )
    })? {
        FoundCrate::Itself => quote!(::wire_repr),
        FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
    };
    render::render(model::WireType::parse(input)?, &runtime)
}
