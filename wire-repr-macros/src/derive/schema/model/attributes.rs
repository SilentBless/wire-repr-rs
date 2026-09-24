use syn::{Expr, Ident, Path, Type};

use super::{Endian, ScalarType};

pub(super) fn parse_item_attributes(
    attributes: &[syn::Attribute],
    owner: &str,
) -> syn::Result<Vec<Path>> {
    let mut validators = Vec::new();
    for attribute in attributes {
        if !attribute.path().is_ident("wire") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("validate") {
                let validator: Path = meta.value()?.parse()?;
                crate::validator::error_type(&validator)?;
                validators.push(validator);
                return Ok(());
            }
            Err(meta.error(format!("unsupported {owner} schema attribute")))
        })?;
    }
    Ok(validators)
}

#[derive(Default)]
pub(super) struct FieldAttributes {
    pub(super) endian: Option<Endian>,
    pub(super) constant: Option<Expr>,
    pub(super) representation: Option<ScalarType>,
    pub(super) bytes: Option<Ident>,
    pub(super) rest: bool,
    pub(super) pad_before: Option<Expr>,
    pub(super) align_before: Option<Expr>,
    pub(super) position: Option<Expr>,
    pub(super) flag: Option<Ident>,
    pub(super) condition: Option<Ident>,
    pub(super) counted_by: Option<Ident>,
    pub(super) bits_of: Option<Ident>,
    pub(super) bit: Option<u32>,
    pub(super) bits: Option<(u32, u32)>,
    pub(super) computed: Option<Expr>,
    pub(super) try_computed: Option<(Expr, Path)>,
}

impl FieldAttributes {
    pub(super) fn parse(attributes: &[syn::Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();
        for attribute in attributes {
            if !attribute.path().is_ident("wire") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                let endian = if meta.path.is_ident("le") {
                    Some(Endian::Little)
                } else if meta.path.is_ident("be") {
                    Some(Endian::Big)
                } else {
                    None
                };
                if let Some(endian) = endian {
                    if result.endian.is_some() {
                        return Err(meta.error("duplicate or conflicting endian attribute"));
                    }
                    result.endian = Some(endian);
                    return Ok(());
                }
                if meta.path.is_ident("as") {
                    if result.representation.is_some() {
                        return Err(meta.error("duplicate `as` wire type"));
                    }
                    let ty: Type = meta.value()?.parse()?;
                    let representation = primitive_name(&ty)
                        .as_deref()
                        .and_then(ScalarType::from_name)
                        .ok_or_else(|| {
                            syn::Error::new_spanned(
                                ty,
                                "`as` requires a fixed-width primitive scalar type",
                            )
                        })?;
                    result.representation = Some(representation);
                    return Ok(());
                }
                if meta.path.is_ident("constant") {
                    if result.constant.is_some() {
                        return Err(meta.error("duplicate constant attribute"));
                    }
                    result.constant = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("bytes") {
                    if result.bytes.is_some() {
                        return Err(meta.error("duplicate `bytes` controller"));
                    }
                    result.bytes = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("rest") {
                    if result.rest {
                        return Err(meta.error("duplicate `rest` attribute"));
                    }
                    result.rest = true;
                    return Ok(());
                }
                if meta.path.is_ident("pad_before") {
                    if result.pad_before.is_some() {
                        return Err(meta.error("duplicate `pad_before` attribute"));
                    }
                    result.pad_before = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("align_before") {
                    if result.align_before.is_some() {
                        return Err(meta.error("duplicate `align_before` attribute"));
                    }
                    result.align_before = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("counted_by") {
                    if result.counted_by.is_some() {
                        return Err(meta.error("duplicate `counted_by` controller"));
                    }
                    result.counted_by = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("at") {
                    if result.position.is_some() {
                        return Err(meta.error("duplicate `at` attribute"));
                    }
                    result.position = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("flag") {
                    if result.flag.is_some() {
                        return Err(meta.error("duplicate `flag` controller"));
                    }
                    result.flag = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("depends_on") {
                    if result.condition.is_some() {
                        return Err(meta.error("duplicate `depends_on` condition"));
                    }
                    result.condition = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("bits_of") {
                    if result.bits_of.is_some() {
                        return Err(meta.error("duplicate `bits_of` controller"));
                    }
                    result.bits_of = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("bit") {
                    if result.bit.is_some() || result.bits.is_some() {
                        return Err(meta.error("duplicate or conflicting bit range"));
                    }
                    let value: syn::LitInt = meta.value()?.parse()?;
                    result.bit = Some(value.base10_parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("bits") {
                    if result.bit.is_some() || result.bits.is_some() {
                        return Err(meta.error("duplicate or conflicting bit range"));
                    }
                    let expression: Expr = meta.value()?.parse()?;
                    result.bits = Some(parse_bit_range(expression)?);
                    return Ok(());
                }
                if meta.path.is_ident("computed") {
                    if result.computed.is_some() || result.try_computed.is_some() {
                        return Err(meta.error("duplicate or conflicting computed callback"));
                    }
                    result.computed = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                if meta.path.is_ident("try_computed") {
                    if result.computed.is_some() || result.try_computed.is_some() {
                        return Err(meta.error("duplicate or conflicting computed callback"));
                    }
                    let expression: Expr = meta.value()?.parse()?;
                    let path = computed_callback_path(&expression)?;
                    let error = crate::validator::computed_error_type(path)?;
                    result.try_computed = Some((expression, error));
                    return Ok(());
                }
                Err(meta.error("unsupported schema field attribute"))
            })?;
        }
        Ok(result)
    }
}

fn computed_callback_path(expression: &Expr) -> syn::Result<&Path> {
    let Expr::Call(call) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed callback must be a function call",
        ));
    };
    let Expr::Path(path) = call.func.as_ref() else {
        return Err(syn::Error::new_spanned(
            &call.func,
            "computed callback must name a function",
        ));
    };
    Ok(&path.path)
}

pub(super) fn primitive_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments[0].ident.to_string())
}

pub(super) fn fixed_scalar_array(ty: &Type) -> syn::Result<Option<(ScalarType, Expr)>> {
    let Type::Array(array) = ty else {
        return Ok(None);
    };
    let Some(element) = primitive_name(&array.elem)
        .as_deref()
        .and_then(ScalarType::from_name)
    else {
        return Err(syn::Error::new_spanned(
            ty,
            "fixed wire arrays require primitive scalar elements",
        ));
    };
    Ok(Some((element, array.len.clone())))
}

fn parse_bit_range(expression: Expr) -> syn::Result<(u32, u32)> {
    let Expr::Range(range) = expression else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`bits` requires an inclusive integer range",
        ));
    };
    if !matches!(range.limits, syn::RangeLimits::Closed(_)) {
        return Err(syn::Error::new_spanned(
            range,
            "`bits` range must be inclusive",
        ));
    }
    let Some(start) = range.start else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`bits` range requires a start",
        ));
    };
    let Some(end) = range.end else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`bits` range requires an end",
        ));
    };
    let Expr::Lit(start) = *start else {
        return Err(syn::Error::new_spanned(
            start,
            "bit bounds must be integer literals",
        ));
    };
    let Expr::Lit(end) = *end else {
        return Err(syn::Error::new_spanned(
            end,
            "bit bounds must be integer literals",
        ));
    };
    let syn::Lit::Int(start) = start.lit else {
        return Err(syn::Error::new_spanned(
            start,
            "bit bounds must be integer literals",
        ));
    };
    let syn::Lit::Int(end) = end.lit else {
        return Err(syn::Error::new_spanned(
            end,
            "bit bounds must be integer literals",
        ));
    };
    let start = start.base10_parse()?;

    let end = end.base10_parse()?;
    if start > end {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "bit range start must not exceed end",
        ));
    }
    Ok((start, end))
}

pub(super) fn is_raw_bytes(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let mut segments = path.path.segments.iter().rev();
    matches!(
        (segments.next(), segments.next()),
        (Some(bytes), Some(wire)) if bytes.ident == "Bytes" && wire.ident == "wire"
    )
}

pub(super) fn array_item_type(ty: &Type) -> Option<Type> {
    marker_item_type(ty, "Array")
}

pub(super) fn recursive_item_type(ty: &Type) -> Option<Type> {
    marker_item_type(ty, "Recursive")
}

fn marker_item_type(ty: &Type, marker_name: &str) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let mut segments = path.path.segments.iter().rev();
    let marker = segments.next()?;
    let wire = segments.next()?;
    if marker.ident != marker_name || wire.ident != "wire" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &marker.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    match arguments.args.first()? {
        syn::GenericArgument::Type(item) => Some(item.clone()),
        _ => None,
    }
}
