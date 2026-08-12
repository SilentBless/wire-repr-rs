//! Layout-mode dispatch and shared codec token mapping.

mod absolute;
mod scalar;
mod sequential;
mod sequential_dynamic;

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{
    Builtin, Codec, Field, IntegerType, Invocation, Item, Layout, MappingRaw, ProjectionKind,
    UnsignedType,
};

/// Renders a validated invocation into its public view families.
pub(crate) fn render(invocation: Invocation) -> TokenStream {
    let items = invocation.items.iter().map(|item| match item {
        Item::Scalar(scalar) => scalar::render_scalar(scalar),
        Item::Layout(Layout::Sequential(layout)) if layout.has_dynamic => {
            sequential_dynamic::render_layout(layout)
        }
        Item::Layout(Layout::Sequential(layout)) => sequential::render_layout(layout),
        Item::Layout(Layout::Absolute(layout)) => absolute::render_layout(layout),
    });
    quote! { #(#items)* }
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

/// Renders the fixed codec used to physically decode and encode a field.
pub(super) fn effective_fixed_codec_tokens(field: &Field) -> Option<TokenStream> {
    match field.mapping.as_ref().map(|mapping| mapping.raw) {
        Some(MappingRaw::Bytes(width)) => Some(quote!(::wire_repr::__private::OwnedBytes<#width>)),
        Some(MappingRaw::Builtin(_)) | None => field.codec().map(codec_tokens),
    }
}

/// Renders the raw value preserved by a validated semantic mapping.
pub(super) fn mapping_raw_type_tokens(field: &Field) -> Option<TokenStream> {
    match field.mapping.as_ref()?.raw {
        MappingRaw::Builtin(IntegerType::U8) => Some(quote!(u8)),
        MappingRaw::Builtin(IntegerType::I8) => Some(quote!(i8)),
        MappingRaw::Builtin(IntegerType::U16) => Some(quote!(u16)),
        MappingRaw::Builtin(IntegerType::I16) => Some(quote!(i16)),
        MappingRaw::Builtin(IntegerType::U32) => Some(quote!(u32)),
        MappingRaw::Builtin(IntegerType::I32) => Some(quote!(i32)),
        MappingRaw::Builtin(IntegerType::U64) => Some(quote!(u64)),
        MappingRaw::Builtin(IntegerType::I64) => Some(quote!(i64)),
        MappingRaw::Builtin(IntegerType::U128) => Some(quote!(u128)),
        MappingRaw::Builtin(IntegerType::I128) => Some(quote!(i128)),
        MappingRaw::Bytes(width) => Some(quote!([u8; #width])),
    }
}

/// Renders direct immutable projection getters shared by both layout renderers.
pub(super) fn projection_getters(field: &Field, visibility: &syn::Visibility) -> Vec<TokenStream> {
    field.projections.iter().map(|projection| {
        let docs = &projection.docs;
        let name = &projection.name;
        let storage = field.raw_name.as_ref().unwrap_or(&field.name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_build_into_for_every_builder_layout() {
        let parsed = syn::parse_str(
            "pub layout Sequential { field value: U8; } pub absolute layout Absolute { field value: U8 { offset: 0; } } pub layout Dynamic { field length: U8; field payload: region(length); }",
        )
        .expect("test layout syntax is valid");
        let rendered =
            render(crate::ir::normalize(parsed).expect("test layouts normalize")).to_string();

        assert_eq!(rendered.matches("fn build_into").count(), 3, "{rendered}");
        assert_eq!(
            rendered.matches("# [inline] pub fn build_into").count(),
            3,
            "{rendered}"
        );
        assert!(!rendered.contains("inline (always)"), "{rendered}");
    }
}
