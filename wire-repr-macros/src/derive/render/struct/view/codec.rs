//! Fixed-codec view decoding helpers.

use crate::derive::model::Codec;
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn decode(codec: &Codec, runtime: &TokenStream, bytes: TokenStream) -> TokenStream {
    match codec {
        Codec::Builtin(codec) => decode_builtin(codec, bytes),
        Codec::OwnedBytes(_) | Codec::Custom(_) => {
            let codec = super::super::codec_tokens(codec, runtime);
            quote!(<#codec as #runtime::FixedCodec>::decode(#bytes))
        }
    }
}

pub(super) fn decode_builtin(codec: &str, bytes: TokenStream) -> TokenStream {
    match codec {
        "U8" => quote!(#bytes[0]),
        "I8" => quote!(#bytes[0] as i8),
        "BeU16" => quote!(u16::from_be_bytes(*#bytes)),
        "LeU16" => quote!(u16::from_le_bytes(*#bytes)),
        "BeI16" => quote!(i16::from_be_bytes(*#bytes)),
        "LeI16" => quote!(i16::from_le_bytes(*#bytes)),
        "BeU32" => quote!(u32::from_be_bytes(*#bytes)),
        "LeU32" => quote!(u32::from_le_bytes(*#bytes)),
        "BeI32" => quote!(i32::from_be_bytes(*#bytes)),
        "LeI32" => quote!(i32::from_le_bytes(*#bytes)),
        "BeU64" => quote!(u64::from_be_bytes(*#bytes)),
        "LeU64" => quote!(u64::from_le_bytes(*#bytes)),
        "BeI64" => quote!(i64::from_be_bytes(*#bytes)),
        "LeI64" => quote!(i64::from_le_bytes(*#bytes)),
        "BeU128" => quote!(u128::from_be_bytes(*#bytes)),
        "LeU128" => quote!(u128::from_le_bytes(*#bytes)),
        "BeI128" => quote!(i128::from_be_bytes(*#bytes)),
        "LeI128" => quote!(i128::from_le_bytes(*#bytes)),
        _ => unreachable!("model only classifies supported built-in codecs"),
    }
}
