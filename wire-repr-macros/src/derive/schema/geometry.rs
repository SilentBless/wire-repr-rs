use proc_macro2::TokenStream;
use quote::quote;

use super::model::{LayoutOffset, Schema, SizeTerm};

#[derive(Clone, Copy)]
enum Capability {
    View,
    Builder,
}

fn optional_size(terms: &[SizeTerm], runtime: &TokenStream, capability: Capability) -> TokenStream {
    let parts = terms.iter().map(|term| match term {
        SizeTerm::Fixed(width) => quote!(Some(#width)),
        SizeTerm::Expr(width) => quote!(Some(#width)),
        SizeTerm::Scaled(len, width) => quote!((#len as usize).checked_mul(#width)),
        SizeTerm::Nested(ty) => match capability {
            Capability::View => quote!(<#ty as #runtime::WireView>::FIXED_SIZE),
            Capability::Builder => quote!(<#ty as #runtime::WireBuilder>::FIXED_SIZE),
        },
        SizeTerm::Dynamic => quote!(None),
    });
    quote!(#runtime::__private::checked_optional_sum([#(#parts),*]))
}

pub(super) fn view_offset(offset: &LayoutOffset, runtime: &TokenStream) -> TokenStream {
    optional_size(&offset.terms, runtime, Capability::View)
}

pub(super) fn builder_offset(offset: &LayoutOffset, runtime: &TokenStream) -> TokenStream {
    optional_size(&offset.terms, runtime, Capability::Builder)
}

pub(super) fn view_optional_size(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    optional_size(&schema.size_terms(), runtime, Capability::View)
}

pub(super) fn builder_optional_size(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    optional_size(&schema.size_terms(), runtime, Capability::Builder)
}
