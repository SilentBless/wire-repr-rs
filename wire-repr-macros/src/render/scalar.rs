//! Renderer for declared nominal fixed integer scalars.

use proc_macro2::TokenStream;
use quote::quote;

use crate::ir::{Builtin, IntegerType, Scalar};

/// Renders one validated transparent scalar and its fixed codec delegation.
pub(super) fn render_scalar(scalar: &Scalar) -> TokenStream {
    let docs = &scalar.docs;
    let visibility = &scalar.visibility;
    let name = &scalar.name;
    let storage = builtin_tokens(scalar.storage);
    let raw = raw_type_tokens(scalar.raw_type);
    quote! {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
        #[doc = "A nominal fixed-width wire integer scalar."]
        #(#docs)*
        #visibility struct #name(#raw);

        impl #name {
            #[doc = "Creates this scalar from its semantic integer value."]
            #[inline]
            #[must_use]
            #visibility const fn new(raw: #raw) -> Self { Self(raw) }

            #[doc = "Returns this scalar's semantic integer value."]
            #[inline]
            #[must_use]
            #visibility const fn raw(self) -> #raw { self.0 }
        }

        impl ::core::convert::From<#raw> for #name {
            #[inline]
            fn from(raw: #raw) -> Self { Self::new(raw) }
        }

        impl ::core::convert::From<#name> for #raw {
            #[inline]
            fn from(value: #name) -> Self { value.raw() }
        }

        impl ::wire_repr::FixedCodec for #name {
            type Value<'wire> = Self where Self: 'wire;
            type EncodeError = <#storage as ::wire_repr::FixedCodec>::EncodeError;
            type Plan<'value> = <#storage as ::wire_repr::FixedCodec>::Plan<'value> where Self: 'value;
            const WIDTH: usize = <#storage as ::wire_repr::FixedCodec>::WIDTH;
            #[inline]
            fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
                Self::new(<#storage as ::wire_repr::FixedCodec>::decode(bytes))
            }
            #[inline]
            fn plan<'value>(value: Self::Value<'value>) -> ::core::result::Result<Self::Plan<'value>, Self::EncodeError> {
                <#storage as ::wire_repr::FixedCodec>::plan(value.raw())
            }
        }
    }
}

fn builtin_tokens(storage: Builtin) -> TokenStream {
    match storage {
        Builtin::U8 => quote!(::wire_repr::U8),
        Builtin::I8 => quote!(::wire_repr::I8),
        Builtin::BeU16 => quote!(::wire_repr::BeU16),
        Builtin::LeU16 => quote!(::wire_repr::LeU16),
        Builtin::BeI16 => quote!(::wire_repr::BeI16),
        Builtin::LeI16 => quote!(::wire_repr::LeI16),
        Builtin::BeU24 => quote!(::wire_repr::BeU24),
        Builtin::LeU24 => quote!(::wire_repr::LeU24),
        Builtin::BeU32 => quote!(::wire_repr::BeU32),
        Builtin::LeU32 => quote!(::wire_repr::LeU32),
        Builtin::BeI32 => quote!(::wire_repr::BeI32),
        Builtin::LeI32 => quote!(::wire_repr::LeI32),
        Builtin::BeU64 => quote!(::wire_repr::BeU64),
        Builtin::LeU64 => quote!(::wire_repr::LeU64),
        Builtin::BeI64 => quote!(::wire_repr::BeI64),
        Builtin::LeI64 => quote!(::wire_repr::LeI64),
        Builtin::BeU128 => quote!(::wire_repr::BeU128),
        Builtin::LeU128 => quote!(::wire_repr::LeU128),
        Builtin::BeI128 => quote!(::wire_repr::BeI128),
        Builtin::LeI128 => quote!(::wire_repr::LeI128),
    }
}

fn raw_type_tokens(raw_type: IntegerType) -> TokenStream {
    match raw_type {
        IntegerType::U8 => quote!(u8),
        IntegerType::I8 => quote!(i8),
        IntegerType::U16 => quote!(u16),
        IntegerType::I16 => quote!(i16),
        IntegerType::U32 => quote!(u32),
        IntegerType::I32 => quote!(i32),
        IntegerType::U64 => quote!(u64),
        IntegerType::I64 => quote!(i64),
        IntegerType::U128 => quote!(u128),
        IntegerType::I128 => quote!(i128),
    }
}
