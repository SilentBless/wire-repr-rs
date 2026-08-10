//! Layout-mode dispatch and shared codec token mapping.

mod absolute;
mod sequential;
mod sequential_dynamic;

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{Builtin, Codec, Field, Invocation, Layout, ProjectionKind, UnsignedType};

/// Renders a validated invocation into its public view families.
pub(crate) fn render(invocation: Invocation) -> TokenStream {
    let layouts = invocation.layouts.iter().map(|layout| match layout {
        Layout::Sequential(layout) if layout.has_dynamic => {
            sequential_dynamic::render_layout(layout)
        }
        Layout::Sequential(layout) => sequential::render_layout(layout),
        Layout::Absolute(layout) => absolute::render_layout(layout),
    });
    quote! { #(#layouts)* }
}

pub(super) fn codec_tokens(codec: &Codec) -> TokenStream {
    match codec {
        Codec::Builtin(Builtin::U8) => quote!(::wire_repr::U8),
        Codec::Builtin(Builtin::I8) => quote!(::wire_repr::I8),
        Codec::Builtin(Builtin::BeU16) => quote!(::wire_repr::BeU16),
        Codec::Builtin(Builtin::LeU16) => quote!(::wire_repr::LeU16),
        Codec::Builtin(Builtin::BeI16) => quote!(::wire_repr::BeI16),
        Codec::Builtin(Builtin::LeI16) => quote!(::wire_repr::LeI16),
        Codec::Builtin(Builtin::BeU24) => quote!(::wire_repr::BeU24),
        Codec::Builtin(Builtin::LeU24) => quote!(::wire_repr::LeU24),
        Codec::Builtin(Builtin::BeI32) => quote!(::wire_repr::BeI32),
        Codec::Builtin(Builtin::LeI32) => quote!(::wire_repr::LeI32),
        Codec::Builtin(Builtin::BeU32) => quote!(::wire_repr::BeU32),
        Codec::Builtin(Builtin::LeU32) => quote!(::wire_repr::LeU32),
        Codec::Builtin(Builtin::BeI64) => quote!(::wire_repr::BeI64),
        Codec::Builtin(Builtin::LeI64) => quote!(::wire_repr::LeI64),
        Codec::Builtin(Builtin::BeU64) => quote!(::wire_repr::BeU64),
        Codec::Builtin(Builtin::LeU64) => quote!(::wire_repr::LeU64),
        Codec::Builtin(Builtin::BeI128) => quote!(::wire_repr::BeI128),
        Codec::Builtin(Builtin::LeI128) => quote!(::wire_repr::LeI128),
        Codec::Builtin(Builtin::BeU128) => quote!(::wire_repr::BeU128),
        Codec::Builtin(Builtin::LeU128) => quote!(::wire_repr::LeU128),
        Codec::Custom(path) | Codec::Prefix(path) => quote!(#path),
        Codec::Bytes(width) => quote!(::wire_repr::Bytes<#width>),
    }
}

/// Renders direct immutable projection getters shared by both layout renderers.
pub(super) fn projection_getters(field: &Field, visibility: &syn::Visibility) -> Vec<TokenStream> {
    field.projections.iter().map(|projection| {
        let docs = &projection.docs;
        let name = &projection.name;
        let storage = &field.name;
        let start = projection.start;
        let end = projection.end;
        let ty = unsigned_type_tokens(projection.value_type);
        let bits = end - start + 1;
        let mask: u128 = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        let mask = syn::LitInt::new(&format!("{mask}"), proc_macro2::Span::call_site());
        match projection.kind {
            ProjectionKind::Bit => quote! { #[doc = "Returns whether this validated storage bit is set."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> bool { let value = self.#storage(); ((value >> #start) & 1) != 0 } },
            ProjectionKind::Bits => quote! { #[doc = "Returns this validated storage bit range normalized to bit zero."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> #ty { let value = self.#storage(); (value >> #start) & (#mask as #ty) } },
        }
    }).collect()
}

fn unsigned_type_tokens(value_type: UnsignedType) -> TokenStream {
    match value_type {
        UnsignedType::U8 => quote!(u8),
        UnsignedType::U16 => quote!(u16),
        UnsignedType::U32 => quote!(u32),
        UnsignedType::U64 => quote!(u64),
        UnsignedType::U128 => quote!(u128),
    }
}
