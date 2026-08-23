//! Nominal bitfield semantic parsing.

use std::collections::BTreeSet;

use syn::{Expr, Fields, Generics, Ident, Lit, Type, Visibility};

use super::codec::parse_tag_codec;
use super::{BitfieldField, TagCodec, WireBitfield, WireType, parse_wire_lifetime};

pub(super) fn parse_storage(ty: &Type, endian: Option<bool>) -> syn::Result<TagCodec> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "bitfield storage must be an unsigned integer type",
        ));
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            ty,
            "bitfield storage must be an unsigned integer type",
        ));
    }
    let name = path.path.segments[0].ident.to_string();
    let codec = match (name.as_str(), endian) {
        ("u8", None) => "U8",
        ("u8", Some(_)) => {
            return Err(syn::Error::new_spanned(
                ty,
                "u8 bitfield storage does not accept a byte-order marker",
            ));
        }
        ("u16", Some(true)) => "BeU16",
        ("u16", Some(false)) => "LeU16",
        ("u32", Some(true)) => "BeU32",
        ("u32", Some(false)) => "LeU32",
        ("u64", Some(true)) => "BeU64",
        ("u64", Some(false)) => "LeU64",
        ("u128", Some(true)) => "BeU128",
        ("u128", Some(false)) => "LeU128",
        ("u16" | "u32" | "u64" | "u128", None) => {
            return Err(syn::Error::new_spanned(
                ty,
                "multi-byte bitfield storage requires `be` or `le`",
            ));
        }
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                "bitfield storage must be u8, u16, u32, u64, or u128",
            ));
        }
    };
    parse_tag_codec(syn::parse_str(codec).expect("built-in codec path"))
}

pub(super) fn parse(
    generics: Generics,
    vis: Visibility,
    name: Ident,
    fields: Fields,
    storage: TagCodec,
) -> syn::Result<WireType> {
    if parse_wire_lifetime(&generics, "Wire bitfields")?.is_some() {
        return Err(syn::Error::new_spanned(
            generics,
            "Wire bitfields do not use a lifetime parameter",
        ));
    }
    let Fields::Named(fields) = fields else {
        return Err(syn::Error::new_spanned(
            name,
            "Wire bitfields require named projection fields",
        ));
    };
    if fields.named.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "Wire bitfields require at least one projection",
        ));
    }

    let storage_bits = 128 - storage.max.leading_zeros();
    let mut occupied = BTreeSet::new();
    let fields = fields
        .named
        .into_iter()
        .map(|field| parse_bitfield_field(field, storage_bits, &mut occupied))
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(WireType::Bitfield(WireBitfield {
        vis,
        name,
        storage,
        fields,
    }))
}

fn parse_bitfield_field(
    field: syn::Field,
    storage_bits: u32,
    occupied: &mut BTreeSet<u32>,
) -> syn::Result<BitfieldField> {
    let name = field.ident.ok_or_else(|| {
        syn::Error::new_spanned(&field.ty, "Wire bitfields require named projection fields")
    })?;
    let mut range = None;
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if range.is_some() {
                return Err(meta.error("each bitfield projection requires exactly one range"));
            }
            if meta.path.is_ident("bit") {
                let bit: syn::LitInt = meta.value()?.parse()?;
                let bit = bit.base10_parse::<u32>()?;
                range = Some((bit, bit));
                Ok(())
            } else if meta.path.is_ident("bits") {
                let expression: syn::ExprRange = meta.value()?.parse()?;
                let Some(start) = expression.start else {
                    return Err(meta.error("bit ranges require an inclusive start"));
                };
                let Some(end) = expression.end else {
                    return Err(meta.error("bit ranges require an inclusive end"));
                };
                if !matches!(expression.limits, syn::RangeLimits::Closed(_)) {
                    return Err(meta.error("bit ranges must use inclusive `start..=end` syntax"));
                }
                let start = integer_expression(&start, "bit range start")?;
                let end = integer_expression(&end, "bit range end")?;
                range = Some((start, end));
                Ok(())
            } else {
                Err(meta.error("expected `bit = N` or `bits = START..=END`"))
            }
        })?;
    }
    let (start, end) = range.ok_or_else(|| {
        syn::Error::new_spanned(
            &name,
            "bitfield projections require #[wire(bit = N)] or #[wire(bits = START..=END)]",
        )
    })?;
    if start > end || end >= storage_bits {
        return Err(syn::Error::new_spanned(
            &name,
            "bitfield projection is outside the storage width",
        ));
    }
    let width = end - start + 1;
    if width == 1 {
        if !is_plain_bool(&field.ty) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "single-bit projections require bool fields",
            ));
        }
    } else if unsigned_integer_bits(&field.ty).is_none_or(|bits| bits < width) {
        return Err(syn::Error::new_spanned(
            &field.ty,
            "multi-bit projections require an unsigned integer field wide enough for the range",
        ));
    }
    for bit in start..=end {
        if !occupied.insert(bit) {
            return Err(syn::Error::new_spanned(
                &name,
                "bitfield projection overlaps an earlier field",
            ));
        }
    }
    Ok(BitfieldField {
        name,
        ty: field.ty,
        start,
        end,
    })
}

fn integer_expression(expression: &Expr, label: &str) -> syn::Result<u32> {
    let Expr::Lit(expression) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            format!("{label} must be an integer literal"),
        ));
    };
    let Lit::Int(literal) = &expression.lit else {
        return Err(syn::Error::new_spanned(
            expression,
            format!("{label} must be an integer literal"),
        ));
    };
    literal.base10_parse()
}

fn is_plain_bool(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident("bool"))
}

fn unsigned_integer_bits(ty: &Type) -> Option<u32> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    match path.path.segments[0].ident.to_string().as_str() {
        "u8" => Some(8),
        "u16" => Some(16),
        "u32" => Some(32),
        "u64" => Some(64),
        "u128" => Some(128),
        _ => None,
    }
}
