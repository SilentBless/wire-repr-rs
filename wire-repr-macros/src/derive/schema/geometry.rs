use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

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
pub(super) enum ScalarBase {
    Exact(TokenStream),
    Checked(TokenStream),
}

pub(super) fn scalar_position(
    base: ScalarBase,
    output: &Ident,
    layout: &super::model::FieldLayout,
    runtime: &TokenStream,
    error: &TokenStream,
) -> TokenStream {
    let pad = layout
        .pad_before
        .as_ref()
        .map(|pad| quote!(#pad))
        .unwrap_or_else(|| quote!(0usize));
    let start = match base {
        ScalarBase::Exact(base) => quote!(#base.checked_add(#pad)),
        ScalarBase::Checked(base) => {
            quote!(#base.and_then(|offset| offset.checked_add(#pad)))
        }
    };
    let mutable = layout.align_before.as_ref().map(|_| quote!(mut));
    let aligned = layout.align_before.as_ref().map(|align| {
        quote! {
            #output = #runtime::__private::checked_align(#output, #align)
                .ok_or(#error)?;
        }
    });
    quote! {
        let #mutable #output = #start.ok_or(#error)?;
        #aligned
    }
}
