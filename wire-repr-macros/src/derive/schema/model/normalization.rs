use syn::{Expr, Ident, Type};

use super::attributes::{
    FieldAttributes, array_item_type, fixed_scalar_array, is_raw_bytes, primitive_name,
    recursive_item_type,
};
use super::validation::validate_unsigned_controller;
use super::{
    ArrayField, BitProjection, Computed, DynamicExtent, Endian, Field, FieldKind, FieldLayout,
    FixedBytes, FixedScalarArray, FlagField, LayoutOffset, NestedField, Position, RawBytes,
    RecursiveField, Scalar, ScalarType, SizeTerm, ValueType, scalar_endian,
};

pub(super) fn normalize_field(
    name: Ident,
    ty: Type,
    attributes: FieldAttributes,
    index: usize,
    field_count: usize,
    parsed: &[Field],
    preceding: &mut Vec<SizeTerm>,
) -> syn::Result<Field> {
    let FieldAttributes {
        endian,
        constant,
        representation,
        bytes,
        rest,
        pad_before,
        align_before,
        position,
        flag,
        condition,
        bits_of,
        bit,
        computed,
        try_computed,
        bits,
        counted_by,
    } = attributes;
    if bytes.is_some() && rest {
        return Err(syn::Error::new_spanned(
            &name,
            "`bytes` and `rest` are mutually exclusive",
        ));
    }
    if rest && index + 1 != field_count {
        return Err(syn::Error::new_spanned(
            &name,
            "`rest` is only valid on the final physical field",
        ));
    }
    let position = position.map(|position| classify_position(position, parsed));
    let layout = FieldLayout {
        pad_before,
        align_before,
        position,
        condition,
    };
    validate_field_layout(&name, &layout, preceding)?;
    let primitive = primitive_name(&ty);
    let value_type = primitive
        .as_deref()
        .and_then(ValueType::from_name)
        .or_else(|| representation.map(|_| ValueType::Custom(Box::new(ty.clone()))));
    let bit_range = match (bit, bits) {
        (Some(bit), None) => Some((bit, bit)),
        (None, Some(bits)) => Some(bits),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("parser rejects conflicting bit ranges"),
    };
    let computed = match (computed, try_computed) {
        (Some(expression), None) => Some(Computed {
            expression,
            error: None,
        }),
        (None, Some((expression, error))) => Some(Computed {
            expression,
            error: Some(error),
        }),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("parser rejects conflicting callbacks"),
    };
    if computed.is_some() && value_type.is_none() {
        return Err(syn::Error::new_spanned(
            &name,
            "computed destinations must be scalar fields",
        ));
    }
    if computed.is_some() && bits_of.is_some() {
        return Err(syn::Error::new_spanned(
            &name,
            "computed fields cannot be logical bit projections",
        ));
    }
    let kind = if let Some(controller) = bits_of {
        let (start, end) = bit_range.ok_or_else(|| {
            syn::Error::new_spanned(&name, "`bits_of` requires `bit = N` or `bits = A..=B`")
        })?;
        validate_bit_projection(
            parsed,
            &name,
            &ty,
            value_type.clone(),
            &controller,
            start,
            end,
            endian,
            constant.as_ref(),
            representation,
            bytes.as_ref(),
            rest,
            counted_by.as_ref(),
            &layout,
        )?;
        FieldKind::BitProjection(BitProjection {
            controller,
            start,
            end,
        })
    } else if bit_range.is_some() {
        return Err(syn::Error::new_spanned(
            &name,
            "`bit` and `bits` require `bits_of = earlier_field`",
        ));
    } else if let Some(controller) = flag {
        if !matches!(value_type, Some(ValueType::Bool))
            || endian.is_some()
            || constant.is_some()
            || representation.is_some()
            || bytes.is_some()
            || rest
            || layout.pad_before.is_some()
            || layout.align_before.is_some()
            || layout.position.is_some()
            || layout.condition.is_some()
            || counted_by.is_some()
        {
            return Err(syn::Error::new_spanned(
                &ty,
                "logical flag fields must be plain bool fields",
            ));
        }
        FieldKind::Flag(FlagField { controller })
    } else if let Some(value_type) = value_type {
        if bytes.is_some() || rest || counted_by.is_some() {
            return Err(syn::Error::new_spanned(
                &ty,
                "scalar fields do not accept dynamic extent or count attributes",
            ));
        }
        let wire_type = match &value_type {
            ValueType::Scalar(scalar_type) => {
                if representation.is_some() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "`as` is only used for Rust types without an implicit wire width",
                    ));
                }
                *scalar_type
            }
            ValueType::Usize | ValueType::Bool | ValueType::Char => {
                let wire_type = representation.ok_or_else(|| {
                    syn::Error::new_spanned(
                        &ty,
                        "this Rust type requires an explicit unsigned `as` wire type",
                    )
                })?;
                if !wire_type.is_unsigned_integer() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "this Rust type requires an unsigned integer wire type",
                    ));
                }
                wire_type
            }
            ValueType::Isize => {
                let wire_type = representation.ok_or_else(|| {
                    syn::Error::new_spanned(&ty, "isize requires an explicit signed `as` wire type")
                })?;
                if !wire_type.is_signed_integer() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "isize requires a signed integer wire type",
                    ));
                }
                wire_type
            }
            ValueType::Custom(_) => representation.expect("custom scalar has `as`"),
        };
        if matches!(&value_type, ValueType::Custom(_)) && constant.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "custom TryFrom scalar fields do not support stored constants",
            ));
        }
        let endian = scalar_endian(wire_type, endian, &ty)?;
        if constant.is_some() && computed.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "computed fields cannot also be constants",
            ));
        }
        FieldKind::Scalar(Scalar {
            value_type,
            wire_type,
            endian,
            constant,
            computed,
        })
    } else if let Some((element, len)) = fixed_scalar_array(&ty)? {
        if bytes.is_some() || rest || counted_by.is_some() {
            return Err(syn::Error::new_spanned(
                &ty,
                "fixed arrays do not accept dynamic extent or count attributes",
            ));
        }
        if matches!(element, ScalarType::U8) {
            if endian.is_some() || representation.is_some() {
                return Err(syn::Error::new_spanned(
                    &ty,
                    "fixed byte arrays do not accept endian or `as` attributes",
                ));
            }
            if computed.is_some() {
                return Err(syn::Error::new_spanned(
                    &name,
                    "computed destinations must be scalar fields",
                ));
            }
            FieldKind::Bytes(FixedBytes { len, constant })
        } else {
            if representation.is_some() || constant.is_some() || computed.is_some() {
                return Err(syn::Error::new_spanned(
                    &ty,
                    "fixed scalar arrays do not accept `as`, constant, or computed attributes",
                ));
            }
            let endian = scalar_endian(element, endian, &ty)?;
            FieldKind::ScalarArray(FixedScalarArray {
                element,
                len,
                endian,
            })
        }
    } else if let Some(root) = recursive_item_type(&ty) {
        if endian.is_some()
            || representation.is_some()
            || constant.is_some()
            || computed.is_some()
            || bytes.is_some()
            || rest
            || counted_by.is_some()
            || layout.condition.is_some()
            || layout.position.is_some()
            || layout.pad_before.is_some()
            || layout.align_before.is_some()
        {
            return Err(syn::Error::new_spanned(
                &ty,
                "recursive fields do not accept scalar, extent, dependency, or placement attributes",
            ));
        }
        FieldKind::Recursive(RecursiveField { root })
    } else if let Some(item) = array_item_type(&ty) {
        if endian.is_some()
            || representation.is_some()
            || constant.is_some()
            || bytes.is_some()
            || rest
        {
            return Err(syn::Error::new_spanned(
                &ty,
                "runtime arrays do not accept scalar or byte-extent attributes",
            ));
        }
        let controller = counted_by.ok_or_else(|| {
            syn::Error::new_spanned(
                &ty,
                "`wire::Array<T>` requires `counted_by = earlier_field`",
            )
        })?;
        FieldKind::Array(ArrayField { item, controller })
    } else if is_raw_bytes(&ty) {
        if endian.is_some()
            || representation.is_some()
            || constant.is_some()
            || counted_by.is_some()
        {
            return Err(syn::Error::new_spanned(
                &ty,
                "raw byte fields do not accept scalar wire attributes",
            ));
        }
        let extent = match (bytes, rest) {
            (Some(controller), false) => DynamicExtent::Bounded(controller),
            (None, true) => DynamicExtent::Rest,
            (None, false) => {
                return Err(syn::Error::new_spanned(
                    &ty,
                    "`wire::Bytes` requires `bytes = earlier_field` or `rest`",
                ));
            }
            (Some(_), true) => unreachable!("validated mutually exclusive extents"),
        };
        FieldKind::RawBytes(RawBytes { extent })
    } else {
        if endian.is_some()
            || constant.is_some()
            || representation.is_some()
            || rest
            || counted_by.is_some()
        {
            return Err(syn::Error::new_spanned(
                &ty,
                "nested schema fields do not accept scalar wire attributes or `rest`",
            ));
        }
        FieldKind::Nested(NestedField {
            ty: ty.clone(),
            terminal: index + 1 == field_count,
            extent: bytes,
        })
    };

    let offset = LayoutOffset {
        terms: preceding.clone(),
    };
    let bounded_nested = matches!(
        &kind,
        FieldKind::Nested(NestedField {
            extent: Some(_),
            ..
        })
    );
    let size = kind.size_term();
    let resets_static_geometry = bounded_nested
        || matches!(size, SizeTerm::Dynamic)
        || layout.pad_before.is_some()
        || layout.align_before.is_some()
        || layout.position.is_some()
        || layout.condition.is_some();
    if resets_static_geometry {
        preceding.clear();
        preceding.push(SizeTerm::Dynamic);
    } else {
        preceding.push(size);
    }

    Ok(Field {
        name,
        ty,
        kind,
        layout,
        offset,
    })
}

fn validate_field_layout(
    field: &Ident,
    layout: &FieldLayout,
    _preceding: &[SizeTerm],
) -> syn::Result<()> {
    if layout.position.is_some() && (layout.pad_before.is_some() || layout.align_before.is_some()) {
        return Err(syn::Error::new_spanned(
            field,
            "`at` cannot be combined with `pad_before` or `align_before`",
        ));
    }
    if let Some(Position::Static(position)) = &layout.position {
        let Expr::Lit(expression) = position else {
            return Err(syn::Error::new_spanned(
                position,
                "`at` requires an integer literal or a physically earlier unsigned field",
            ));
        };
        if !matches!(expression.lit, syn::Lit::Int(_)) {
            return Err(syn::Error::new_spanned(
                position,
                "`at` requires an integer literal",
            ));
        }
        let syn::Lit::Int(position) = &expression.lit else {
            unreachable!("validated integer literal")
        };
        let requested = position.base10_parse::<usize>()?;
        if let Some(current) = static_geometry_end(_preceding)
            && requested < current
        {
            return Err(syn::Error::new_spanned(
                position,
                format!("static `at` position {requested} precedes cursor {current}"),
            ));
        }
    }
    if let Some(Expr::Lit(expression)) = &layout.align_before
        && let syn::Lit::Int(alignment) = &expression.lit
        && alignment.base10_parse::<usize>()? == 0
    {
        return Err(syn::Error::new_spanned(
            alignment,
            "`align_before` must be nonzero",
        ));
    }
    Ok(())
}

fn static_geometry_end(terms: &[SizeTerm]) -> Option<usize> {
    let mut position = 0usize;
    for term in terms {
        let width = match term {
            SizeTerm::Fixed(width) => *width,
            SizeTerm::Expr(Expr::Lit(expression)) => {
                let syn::Lit::Int(width) = &expression.lit else {
                    return None;
                };
                width.base10_parse::<usize>().ok()?
            }
            SizeTerm::Scaled(Expr::Lit(expression), scale) => {
                let syn::Lit::Int(len) = &expression.lit else {
                    return None;
                };
                len.base10_parse::<usize>().ok()?.checked_mul(*scale)?
            }
            SizeTerm::Expr(_)
            | SizeTerm::Scaled(_, _)
            | SizeTerm::Nested(_)
            | SizeTerm::Dynamic => return None,
        };
        position = position.checked_add(width)?;
    }
    Some(position)
}

#[allow(clippy::too_many_arguments)]
fn validate_bit_projection(
    fields: &[Field],
    name: &Ident,
    ty: &Type,
    value_type: Option<ValueType>,
    controller: &Ident,
    start: u32,
    end: u32,
    endian: Option<Endian>,
    constant: Option<&Expr>,
    representation: Option<ScalarType>,
    bytes: Option<&Ident>,
    rest: bool,
    counted_by: Option<&Ident>,
    layout: &FieldLayout,
) -> syn::Result<()> {
    if start > end {
        return Err(syn::Error::new_spanned(
            name,
            "bit range start must not exceed end",
        ));
    }
    let width = end - start + 1;
    let valid_logical = match value_type {
        Some(ValueType::Bool) => width == 1,
        Some(ValueType::Scalar(scalar)) => {
            scalar.is_unsigned_integer() && width <= (scalar.width() * 8) as u32
        }
        Some(ValueType::Usize) => width <= usize::BITS,
        Some(ValueType::Isize | ValueType::Char | ValueType::Custom(_)) | None => false,
    };
    if !valid_logical {
        return Err(syn::Error::new_spanned(
            ty,
            "bit projections require bool for one bit or a sufficiently wide unsigned integer",
        ));
    }
    if endian.is_some()
        || constant.is_some()
        || representation.is_some()
        || bytes.is_some()
        || rest
        || counted_by.is_some()
        || layout.pad_before.is_some()
        || layout.align_before.is_some()
        || layout.position.is_some()
        || layout.condition.is_some()
    {
        return Err(syn::Error::new_spanned(
            name,
            "bit projection fields cannot declare independent physical attributes",
        ));
    }
    if !fields.iter().any(|field| field.name == *controller) {
        return Err(syn::Error::new_spanned(
            controller,
            "bit projection controller must be physically earlier",
        ));
    }
    validate_unsigned_controller(fields, fields.len(), controller, "bit projection", false)?;
    let controller_field = fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated bit projection controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated bit projection controller is scalar")
    };
    if !matches!(
        &scalar.value_type,
        ValueType::Scalar(value) if value.is_unsigned_integer()
    ) {
        return Err(syn::Error::new_spanned(
            controller,
            "bit projection controller must be an unsigned fixed-width scalar",
        ));
    }
    if end >= (scalar.width() * 8) as u32 {
        return Err(syn::Error::new_spanned(
            controller,
            "bit projection controller is not wide enough for the range",
        ));
    }
    if fields.iter().any(|field| {
        matches!(
            &field.kind,
            FieldKind::BitProjection(projection)
                if projection.controller == *controller
                    && start <= projection.end
                    && projection.start <= end
        )
    }) {
        return Err(syn::Error::new_spanned(
            name,
            "bit projection ranges cannot overlap",
        ));
    }
    Ok(())
}

fn classify_position(position: Expr, previous: &[Field]) -> Position {
    if let Expr::Path(path) = &position
        && path.qself.is_none()
        && path.path.segments.len() == 1
    {
        let identifier = &path.path.segments[0].ident;
        if previous.iter().any(|field| field.name == *identifier) {
            return Position::Field(identifier.clone());
        }
    }
    Position::Static(position)
}
