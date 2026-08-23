//! Source-type and codec classification helpers.

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{Expr, Lifetime, Lit, Path, Type};

use super::{Codec, TagCodec};

impl Codec {
    pub(in crate::derive) fn static_width(&self) -> Option<TokenStream> {
        match self {
            Self::Builtin(codec) => {
                let width = match *codec {
                    "U8" | "I8" => 1usize,
                    "BeU16" | "LeU16" | "BeI16" | "LeI16" => 2,
                    "BeU32" | "LeU32" | "BeI32" | "LeI32" => 4,
                    "BeU64" | "LeU64" | "BeI64" | "LeI64" => 8,
                    "BeU128" | "LeU128" | "BeI128" | "LeI128" => 16,
                    _ => return None,
                };
                Some(quote!(#width))
            }
            Self::OwnedBytes(width) => Some(width.clone()),
            Self::Custom(_) => None,
        }
    }
}

pub(super) fn fixed_array_len(array: &syn::TypeArray) -> syn::Result<usize> {
    let Expr::Lit(expression) = &array.len else {
        return Err(syn::Error::new_spanned(
            &array.len,
            "fixed byte array length must be an integer literal",
        ));
    };
    let Lit::Int(literal) = &expression.lit else {
        return Err(syn::Error::new_spanned(
            expression,
            "fixed byte array length must be an integer literal",
        ));
    };
    literal.base10_parse()
}

pub(super) fn parse_tag_codec(path: Path) -> syn::Result<TagCodec> {
    let Some(segment) = path.segments.first().filter(|_| path.segments.len() == 1) else {
        return Err(syn::Error::new_spanned(
            path,
            "Wire enum tags must use a built-in unsigned fixed integer codec",
        ));
    };
    match segment.ident.to_string().as_str() {
        "U8" => Ok(TagCodec {
            codec: "U8".into(),
            builtin: true,
            ty: "u8",
            max: u8::MAX as u128,
        }),
        "BeU16" => Ok(TagCodec {
            codec: "BeU16".into(),
            builtin: true,
            ty: "u16",
            max: u16::MAX as u128,
        }),
        "LeU16" => Ok(TagCodec {
            codec: "LeU16".into(),
            builtin: true,
            ty: "u16",
            max: u16::MAX as u128,
        }),
        "BeU32" => Ok(TagCodec {
            codec: "BeU32".into(),
            builtin: true,
            ty: "u32",
            max: u32::MAX as u128,
        }),
        "LeU32" => Ok(TagCodec {
            codec: "LeU32".into(),
            builtin: true,
            ty: "u32",
            max: u32::MAX as u128,
        }),
        "BeU64" => Ok(TagCodec {
            codec: "BeU64".into(),
            builtin: true,
            ty: "u64",
            max: u64::MAX as u128,
        }),
        "LeU64" => Ok(TagCodec {
            codec: "LeU64".into(),
            builtin: true,
            ty: "u64",
            max: u64::MAX as u128,
        }),
        "BeU128" => Ok(TagCodec {
            codec: "BeU128".into(),
            builtin: true,
            ty: "u128",
            max: u128::MAX,
        }),
        "LeU128" => Ok(TagCodec {
            codec: "LeU128".into(),
            builtin: true,
            ty: "u128",
            max: u128::MAX,
        }),
        "I8" | "BeI16" | "LeI16" | "BeI32" | "LeI32" | "BeI64" | "LeI64" | "BeI128" | "LeI128" => {
            Err(syn::Error::new_spanned(
                path,
                "Wire enum tags must use an unsigned fixed codec",
            ))
        }
        codec => Ok(TagCodec {
            codec: codec.to_owned(),
            builtin: false,
            ty: "u8",
            max: u8::MAX as u128,
        }),
    }
}

pub(super) fn builtin_codec(ty: &Type, endianness: Option<bool>) -> Option<Codec> {
    if let Type::Array(array) = ty {
        if endianness.is_some() || !is_plain_u8(array.elem.as_ref()) {
            return None;
        }
        return Some(Codec::OwnedBytes(array.len.to_token_stream()));
    }

    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() || type_path.path.segments.len() != 1 {
        return None;
    }
    let name = type_path.path.segments.first()?.ident.to_string();
    let codec = match (name.as_str(), endianness) {
        ("u8", None) => "U8",
        ("i8", None) => "I8",
        ("u16", Some(true)) => "BeU16",
        ("u16", Some(false)) => "LeU16",
        ("i16", Some(true)) => "BeI16",
        ("i16", Some(false)) => "LeI16",
        ("u32", Some(true)) => "BeU32",
        ("u32", Some(false)) => "LeU32",
        ("i32", Some(true)) => "BeI32",
        ("i32", Some(false)) => "LeI32",
        ("u64", Some(true)) => "BeU64",
        ("u64", Some(false)) => "LeU64",
        ("i64", Some(true)) => "BeI64",
        ("i64", Some(false)) => "LeI64",
        ("u128", Some(true)) => "BeU128",
        ("u128", Some(false)) => "LeU128",
        ("i128", Some(true)) => "BeI128",
        ("i128", Some(false)) => "LeI128",
        _ => return None,
    };
    Some(Codec::Builtin(codec))
}

pub(super) fn is_plain_u8(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none() && path.path.is_ident("u8")
}

pub(super) fn is_unsigned_integer(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path.qself.is_none()
        && type_path.path.segments.len() == 1
        && matches!(
            type_path.path.segments[0].ident.to_string().as_str(),
            "u8" | "u16" | "u32" | "u64" | "u128"
        )
}

pub(super) fn is_multibyte_integer(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path.qself.is_none()
        && type_path.path.segments.len() == 1
        && matches!(
            type_path.path.segments[0].ident.to_string().as_str(),
            "u16" | "i16" | "u32" | "i32" | "u64" | "i64" | "u128" | "i128"
        )
}

pub(super) fn is_borrowed_byte_slice(ty: &Type, wire_lifetime: Option<&Lifetime>) -> bool {
    let (Type::Reference(reference), Some(wire_lifetime)) = (ty, wire_lifetime) else {
        return false;
    };
    let Some(lifetime) = &reference.lifetime else {
        return false;
    };
    let Type::Slice(slice) = reference.elem.as_ref() else {
        return false;
    };
    let Type::Path(element) = slice.elem.as_ref() else {
        return false;
    };

    reference.mutability.is_none()
        && lifetime.ident == wire_lifetime.ident
        && element.qself.is_none()
        && element.path.is_ident("u8")
}
