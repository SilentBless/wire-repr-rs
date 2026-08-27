use std::collections::BTreeSet;

use syn::{Data, DeriveInput, Expr, Fields, Generics, Ident, Member, Path, Type, Visibility};

pub(super) struct Schema {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) generics: Generics,
    pub(super) fields: Vec<Field>,
    pub(super) validators: Vec<Path>,
    pub(super) computed_order: Vec<usize>,
}

pub(super) struct Field {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) kind: FieldKind,
    pub(super) layout: FieldLayout,
    pub(super) offset: LayoutOffset,
}

pub(super) enum FieldKind {
    Scalar(Scalar),
    Bytes(FixedBytes),
    RawBytes(RawBytes),
    Array(ArrayField),
    Recursive(RecursiveField),
    Flag(FlagField),
    Nested(NestedField),
    BitProjection(BitProjection),
}

pub(super) struct FixedBytes {
    pub(super) len: Expr,
    pub(super) constant: Option<Expr>,
}

pub(super) struct RawBytes {
    pub(super) extent: DynamicExtent,
}

pub(super) enum DynamicExtent {
    Bounded(Ident),
    Rest,
}

pub(super) struct ArrayField {
    pub(super) item: Type,
    pub(super) controller: Ident,
}

pub(super) struct RecursiveField {
    pub(super) root: Type,
}

pub(super) struct FieldLayout {
    pub(super) pad_before: Option<Expr>,
    pub(super) align_before: Option<Expr>,
    pub(super) position: Option<Position>,
    pub(super) condition: Option<Ident>,
}

pub(super) enum Position {
    Static(Expr),
    Field(Ident),
}

pub(super) struct FlagField {
    pub(super) controller: Ident,
}

pub(super) struct BitProjection {
    pub(super) controller: Ident,
    pub(super) start: u32,
    pub(super) end: u32,
}

pub(super) struct NestedField {
    pub(super) ty: Type,
    pub(super) terminal: bool,
    pub(super) extent: Option<Ident>,
}

#[derive(Clone)]
pub(super) struct LayoutOffset {
    pub(super) terms: Vec<SizeTerm>,
}

#[derive(Clone)]
pub(super) enum SizeTerm {
    Fixed(usize),
    Expr(Expr),
    Nested(Type),
    Dynamic,
}

pub(super) struct Scalar {
    pub(super) value_type: ValueType,
    pub(super) wire_type: ScalarType,
    pub(super) endian: Endian,
    pub(super) constant: Option<Expr>,
    pub(super) computed: Option<Computed>,
}

pub(super) struct Computed {
    pub(super) expression: Expr,
    pub(super) error: Option<Path>,
}

#[derive(Clone, Copy)]
pub(super) enum ScalarType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    U128,
    I128,
    F32,
    F64,
}

#[derive(Clone, Copy)]
pub(super) enum ValueType {
    Scalar(ScalarType),
    Usize,
    Isize,
    Bool,
    Char,
}

#[derive(Clone, Copy)]
pub(super) enum Endian {
    Native,
    Little,
    Big,
}

impl ScalarType {
    pub(super) const fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
            Self::U128 | Self::I128 => 16,
        }
    }

    pub(super) const fn is_unsigned_integer(self) -> bool {
        matches!(
            self,
            Self::U8 | Self::U16 | Self::U32 | Self::U64 | Self::U128
        )
    }

    pub(super) const fn is_signed_integer(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128
        )
    }

    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "u8" => Self::U8,
            "i8" => Self::I8,
            "u16" => Self::U16,
            "i16" => Self::I16,
            "u32" => Self::U32,
            "i32" => Self::I32,
            "u64" => Self::U64,
            "i64" => Self::I64,
            "u128" => Self::U128,
            "i128" => Self::I128,
            "f32" => Self::F32,
            "f64" => Self::F64,
            _ => return None,
        })
    }
}

impl ValueType {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "usize" => Some(Self::Usize),
            "isize" => Some(Self::Isize),
            "bool" => Some(Self::Bool),
            "char" => Some(Self::Char),
            _ => ScalarType::from_name(name).map(Self::Scalar),
        }
    }

    pub(super) const fn is_converted(self) -> bool {
        !matches!(self, Self::Scalar(_))
    }
}

impl Scalar {
    pub(super) const fn width(&self) -> usize {
        self.wire_type.width()
    }
}

impl FieldKind {
    pub(super) fn constant(&self) -> Option<&Expr> {
        match self {
            Self::Scalar(scalar) => scalar.constant.as_ref(),
            Self::Bytes(bytes) => bytes.constant.as_ref(),
            Self::RawBytes(_)
            | Self::Array(_)
            | Self::Recursive(_)
            | Self::Flag(_)
            | Self::BitProjection(_)
            | Self::Nested(_) => None,
        }
    }

    pub(super) fn size_term(&self) -> SizeTerm {
        match self {
            Self::Scalar(scalar) => SizeTerm::Fixed(scalar.width()),
            Self::Bytes(bytes) => SizeTerm::Expr(bytes.len.clone()),
            Self::RawBytes(_) | Self::Array(_) | Self::Recursive(_) => SizeTerm::Dynamic,
            Self::Flag(_) | Self::BitProjection(_) => SizeTerm::Fixed(0),
            Self::Nested(nested) => SizeTerm::Nested(nested.ty.clone()),
        }
    }

    pub(super) fn computed(&self) -> Option<&Computed> {
        match self {
            Self::Scalar(scalar) => scalar.computed.as_ref(),
            Self::Bytes(_)
            | Self::RawBytes(_)
            | Self::Array(_)
            | Self::Recursive(_)
            | Self::Flag(_)
            | Self::Nested(_)
            | Self::BitProjection(_) => None,
        }
    }
}

impl Schema {
    pub(super) fn parse(input: DeriveInput, owner: &str) -> syn::Result<Self> {
        let validators = parse_item_attributes(&input.attrs, owner)?;
        let fields = match input.data {
            Data::Struct(data) => match data.fields {
                Fields::Named(fields) => fields.named,
                _ => {
                    return Err(syn::Error::new_spanned(
                        input.ident,
                        format!("{owner} supports named schema structs only"),
                    ));
                }
            },
            _ => {
                return Err(syn::Error::new_spanned(
                    input.ident,
                    format!("{owner} supports schema structs only"),
                ));
            }
        };
        let field_count = fields.len();
        let mut parsed = Vec::with_capacity(field_count);
        let mut preceding = Vec::new();

        for (index, field) in fields.into_iter().enumerate() {
            let name = field.ident.expect("named fields have identifiers");
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
            } = FieldAttributes::parse(&field.attrs)?;
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
            let position = position.map(|position| classify_position(position, &parsed));
            let layout = FieldLayout {
                pad_before,
                align_before,
                position,
                condition,
            };
            Self::validate_field_layout(&name, &layout, &preceding)?;
            let ty = field.ty;
            let primitive = primitive_name(&ty);
            let value_type = primitive.as_deref().and_then(ValueType::from_name);
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
                    &parsed,
                    &name,
                    &ty,
                    value_type,
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
                let wire_type = match value_type {
                    ValueType::Scalar(scalar_type) => {
                        if representation.is_some() {
                            return Err(syn::Error::new_spanned(
                                &ty,
                                "`as` is only used for Rust primitives without an implicit wire width",
                            ));
                        }
                        scalar_type
                    }
                    ValueType::Usize | ValueType::Bool | ValueType::Char => {
                        let wire_type = representation.ok_or_else(|| {
                            syn::Error::new_spanned(
                                &ty,
                                "this Rust primitive requires an explicit unsigned `as` wire type",
                            )
                        })?;
                        if !wire_type.is_unsigned_integer() {
                            return Err(syn::Error::new_spanned(
                                &ty,
                                "this Rust primitive requires an unsigned integer wire type",
                            ));
                        }
                        wire_type
                    }
                    ValueType::Isize => {
                        let wire_type = representation.ok_or_else(|| {
                            syn::Error::new_spanned(
                                &ty,
                                "isize requires an explicit signed `as` wire type",
                            )
                        })?;
                        if !wire_type.is_signed_integer() {
                            return Err(syn::Error::new_spanned(
                                &ty,
                                "isize requires a signed integer wire type",
                            ));
                        }
                        wire_type
                    }
                };
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
            } else if let Some(len) = byte_array_len(&ty)? {
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
                if bytes.is_some() || rest || counted_by.is_some() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "fixed byte arrays do not accept dynamic extent or count attributes",
                    ));
                }
                FieldKind::Bytes(FixedBytes { len, constant })
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

            parsed.push(Field {
                name,
                ty,
                kind,
                layout,
                offset,
            });
        }
        for field in &parsed {
            if field.kind.computed().is_none() {
                continue;
            }
            if field.layout.condition.is_some() {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "computed destinations cannot be conditional",
                ));
            }
            if field.layout.pad_before.is_some()
                || field.layout.align_before.is_some()
                || field.layout.position.is_some()
            {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "computed destinations cannot declare placement geometry",
                ));
            }
            if field
                .offset
                .terms
                .iter()
                .any(|term| matches!(term, SizeTerm::Dynamic))
            {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "computed destinations must have a fixed offset before demand geometry",
                ));
            }
        }
        validate_conditions(&parsed)?;

        validate_geometry_controllers(&parsed)?;
        validate_arrays(&parsed)?;
        validate_bit_controller_roles(&parsed)?;

        validate_computed_controller_roles(&parsed)?;
        let computed_order = validate_computed_dependencies(&parsed)?;
        Ok(Self {
            vis: input.vis,
            name: input.ident,
            generics: input.generics,
            fields: parsed,
            validators,
            computed_order,
        })
    }

    pub(super) fn nested_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::Nested(_)))
    }

    pub(super) fn size_terms(&self) -> Vec<SizeTerm> {
        self.fields
            .iter()
            .map(|field| field.kind.size_term())
            .collect()
    }

    pub(super) fn computed_fields(&self) -> impl Iterator<Item = &Field> {
        self.computed_order.iter().map(|index| &self.fields[*index])
    }
    pub(super) fn is_presence_controller(&self, name: &Ident) -> bool {
        self.fields.iter().any(|field| {
            matches!(
                &field.kind,
                FieldKind::Flag(FlagField { controller }) if controller == name
            )
        })
    }

    pub(super) fn flag_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::Flag(_)))
    }

    pub(super) fn bit_projection_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::BitProjection(_)))
    }

    pub(super) fn is_bit_controller(&self, name: &Ident) -> bool {
        self.bit_projection_fields().any(|field| {
            matches!(
                &field.kind,
                FieldKind::BitProjection(projection) if projection.controller == *name
            )
        })
    }

    pub(super) fn condition_dependents<'schema>(
        &'schema self,
        name: &Ident,
    ) -> impl Iterator<Item = &'schema Field> {
        let name = name.clone();
        self.fields
            .iter()
            .filter(move |field| field.layout.condition.as_ref() == Some(&name))
    }
    pub(super) fn is_count_controller(&self, name: &Ident) -> bool {
        self.fields.iter().any(|field| {
            matches!(
                &field.kind,
                FieldKind::Array(array) if array.controller == *name
            )
        })
    }
    pub(super) fn array_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::Array(_)))
    }

    pub(super) fn layout_can_fail(&self) -> bool {
        self.has_explicit_geometry()
            || self.bit_projection_fields().next().is_some()
            || self
                .fields
                .iter()
                .any(|field| field.kind.computed().is_some())
            || self
                .size_terms()
                .iter()
                .any(|term| !matches!(term, SizeTerm::Fixed(_)))
    }

    pub(super) fn is_syntactically_fixed(&self) -> bool {
        !self.has_explicit_geometry()
            && self.fields.iter().all(|field| {
                matches!(
                    field.kind,
                    FieldKind::Scalar(_) | FieldKind::Bytes(_) | FieldKind::BitProjection(_)
                )
            })
    }

    pub(super) fn has_leading_extent(&self) -> bool {
        if self.is_syntactically_fixed() {
            return true;
        }
        !matches!(
            self.fields.last().map(|field| &field.kind),
            Some(FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Rest,
            })) | Some(FieldKind::Array(_))
        )
    }

    pub(super) fn is_length_controller(&self, name: &Ident) -> bool {
        self.fields.iter().any(|field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            }) => controller == name,
            FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == name,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => false,
        })
    }

    pub(super) fn length_dependents<'schema>(
        &'schema self,
        name: &Ident,
    ) -> impl Iterator<Item = &'schema Field> {
        let name = name.clone();
        self.fields.iter().filter(move |field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            }) => controller == &name,
            FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == &name,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => false,
        })
    }

    pub(super) fn has_explicit_geometry(&self) -> bool {
        self.fields.iter().any(|field| {
            matches!(
                field.kind,
                FieldKind::RawBytes(_) | FieldKind::Array(_) | FieldKind::Flag(_)
            ) || matches!(
                field.kind,
                FieldKind::Nested(NestedField {
                    extent: Some(_),
                    ..
                })
            ) || field.layout.pad_before.is_some()
                || field.layout.align_before.is_some()
                || field.layout.position.is_some()
                || field.layout.condition.is_some()
        })
    }
    fn validate_field_layout(
        field: &Ident,
        layout: &FieldLayout,
        _preceding: &[SizeTerm],
    ) -> syn::Result<()> {
        if layout.position.is_some()
            && (layout.pad_before.is_some() || layout.align_before.is_some())
        {
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
            SizeTerm::Expr(_) | SizeTerm::Nested(_) | SizeTerm::Dynamic => return None,
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
        Some(ValueType::Isize | ValueType::Char) | None => false,
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
    validate_unsigned_controller(fields, fields.len(), controller, "bit projection")?;
    let controller_field = fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated bit projection controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated bit projection controller is scalar")
    };
    if !matches!(
        scalar.value_type,
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
fn validate_computed_controller_roles(fields: &[Field]) -> syn::Result<()> {
    for computed in fields
        .iter()
        .filter(|field| field.kind.computed().is_some())
    {
        let name = &computed.name;
        let controls_geometry = fields.iter().any(|field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == name,
            FieldKind::Array(array) => &array.controller == name,
            FieldKind::Flag(flag) => &flag.controller == name,
            FieldKind::BitProjection(projection) => &projection.controller == name,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Recursive(_)
            | FieldKind::Nested(_) => false,
        }) || fields.iter().any(|field| {
            matches!(
                &field.layout.position,
                Some(Position::Field(controller)) if controller == name
            )
        });
        if controls_geometry {
            return Err(syn::Error::new_spanned(
                name,
                "computed fields cannot control representation geometry",
            ));
        }
    }
    Ok(())
}

fn validate_computed_dependencies(fields: &[Field]) -> syn::Result<Vec<usize>> {
    let computed = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| field.kind.computed().map(|_| index))
        .collect::<Vec<_>>();
    let computed_set = computed.iter().copied().collect::<BTreeSet<_>>();
    let mut dependencies = Vec::with_capacity(computed.len());
    for index in &computed {
        let field = &fields[*index];
        let expression = &field.kind.computed().expect("computed field").expression;
        let names = computed_dependency_names(expression, &field.name, fields)?;
        let deps = names
            .into_iter()
            .filter_map(|name| fields.iter().position(|candidate| candidate.name == name))
            .filter(|dependency| computed_set.contains(dependency))
            .collect::<BTreeSet<_>>();
        dependencies.push((*index, deps));
    }
    let mut remaining = computed_set;
    let mut ordered = Vec::with_capacity(computed.len());
    while !remaining.is_empty() {
        let Some(next) = computed.iter().copied().find(|candidate| {
            remaining.contains(candidate)
                && dependencies
                    .iter()
                    .find(|(index, _)| index == candidate)
                    .is_some_and(|(_, deps)| deps.is_disjoint(&remaining))
        }) else {
            let field = &fields[*remaining.iter().next().expect("nonempty cycle")];
            return Err(syn::Error::new_spanned(
                &field.name,
                "computed field dependency cycle",
            ));
        };
        remaining.remove(&next);
        ordered.push(next);
    }
    Ok(ordered)
}

fn computed_dependency_names(
    expression: &Expr,
    destination: &Ident,
    fields: &[Field],
) -> syn::Result<Vec<Ident>> {
    let Expr::Call(callback) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed callback must be a call",
        ));
    };
    let mut names = Vec::new();
    for argument in &callback.args {
        match argument {
            Expr::Path(path) => {
                let name = simple_computed_name(path)?;
                names.push(if name == "self" {
                    destination.clone()
                } else {
                    name.clone()
                });
            }
            Expr::Call(selection) => {
                let Expr::Path(operation) = selection.func.as_ref() else {
                    return Err(syn::Error::new_spanned(
                        &selection.func,
                        "selection operation must be include or exclude",
                    ));
                };
                let operation = simple_computed_name(operation)?;
                let selected = selection
                    .args
                    .iter()
                    .map(|argument| {
                        fn root(expression: &Expr) -> syn::Result<(Ident, bool)> {
                            match expression {
                                Expr::Path(path) => Ok((simple_computed_name(path)?.clone(), true)),
                                Expr::Field(field) => {
                                    if !matches!(&field.member, Member::Named(_)) {
                                        return Err(syn::Error::new_spanned(
                                            &field.member,
                                            "selection paths require named fields",
                                        ));
                                    }
                                    let (root, _) = root(&field.base)?;
                                    Ok((root, false))
                                }
                                _ => Err(syn::Error::new_spanned(
                                    expression,
                                    "selection fields must be physical field paths",
                                )),
                            }
                        }
                        let (name, whole) = root(argument)?;
                        let name = if name == "self" {
                            destination.clone()
                        } else {
                            name
                        };
                        Ok((name, whole))
                    })
                    .collect::<syn::Result<Vec<_>>>()?;
                if operation == "include" {
                    names.extend(selected.into_iter().map(|(name, _)| name));
                } else if operation == "exclude" {
                    let wholly_excluded = selected
                        .into_iter()
                        .filter_map(|(name, whole)| whole.then_some(name))
                        .collect::<BTreeSet<_>>();
                    names.extend(
                        fields
                            .iter()
                            .filter(|field| !wholly_excluded.contains(&field.name))
                            .map(|field| field.name.clone()),
                    );
                } else {
                    return Err(syn::Error::new_spanned(
                        operation,
                        "selection operation must be include or exclude",
                    ));
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    argument,
                    "computed arguments must be logical fields or selections",
                ));
            }
        }
    }
    for name in &names {
        if !fields.iter().any(|field| field.name == *name) {
            return Err(syn::Error::new_spanned(
                name,
                "computed callback references an unknown field",
            ));
        }
    }
    Ok(names)
}

fn simple_computed_name(path: &syn::ExprPath) -> syn::Result<&Ident> {
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            path,
            "expected a simple field name",
        ));
    }
    Ok(&path.path.segments[0].ident)
}

fn validate_geometry_controllers(fields: &[Field]) -> syn::Result<()> {
    let length_roles = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => Some((index, controller)),
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let mut length_controllers = BTreeSet::new();
    for (index, controller) in &length_roles {
        length_controllers.insert(controller.to_string());
        validate_unsigned_controller(fields, *index, controller, "byte length")?;
    }
    for (index, field) in fields.iter().enumerate() {
        if let Some(Position::Field(controller)) = &field.layout.position {
            if length_controllers.contains(&controller.to_string()) {
                return Err(syn::Error::new_spanned(
                    controller,
                    format!(
                        "controller `{controller}` cannot control both byte length and field position before the controller DAG ships"
                    ),
                ));
            }
            validate_unsigned_controller(fields, index, controller, "field position")?;
        }
    }
    Ok(())
}

fn validate_arrays(fields: &[Field]) -> syn::Result<()> {
    let mut controllers = BTreeSet::new();
    for (index, field) in fields.iter().enumerate() {
        let FieldKind::Array(array) = &field.kind else {
            continue;
        };
        if !controllers.insert(array.controller.to_string()) {
            return Err(syn::Error::new_spanned(
                &array.controller,
                format!(
                    "item count controller `{}` cannot control multiple arrays",
                    array.controller
                ),
            ));
        }
        if field.layout.condition.is_some() {
            return Err(syn::Error::new_spanned(
                &field.name,
                "runtime arrays cannot be conditional in this vertical",
            ));
        }
        let shared_role = fields.iter().any(|candidate| match &candidate.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == &array.controller,
            FieldKind::Flag(flag) => flag.controller == array.controller,
            FieldKind::BitProjection(projection) => projection.controller == array.controller,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Nested(_) => false,
        });
        let placement_role = fields.iter().any(|candidate| {
            matches!(
                &candidate.layout.position,
                Some(Position::Field(controller)) if controller == &array.controller
            )
        });
        if shared_role || placement_role {
            return Err(syn::Error::new_spanned(
                &array.controller,
                "item count controller cannot control another dependency role",
            ));
        }

        validate_unsigned_controller(fields, index, &array.controller, "item count")?;
    }
    Ok(())
}
fn validate_bit_controller_roles(fields: &[Field]) -> syn::Result<()> {
    let controllers = fields
        .iter()
        .filter_map(|field| {
            let FieldKind::BitProjection(projection) = &field.kind else {
                return None;
            };
            Some(projection.controller.clone())
        })
        .collect::<BTreeSet<_>>();
    for controller in controllers {
        let shared = fields.iter().any(|field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(other),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(other),
                ..
            }) => *other == controller,
            FieldKind::Array(array) => array.controller == controller,
            FieldKind::Flag(flag) => flag.controller == controller,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Recursive(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => false,
        }) || fields.iter().any(|field| {
            matches!(
                &field.layout.position,
                Some(Position::Field(other)) if *other == controller
            )
        });
        if shared {
            return Err(syn::Error::new_spanned(
                controller,
                "bit projection controller cannot control another dependency role",
            ));
        }
    }
    Ok(())
}

fn validate_conditions(fields: &[Field]) -> syn::Result<()> {
    let mut controllers = BTreeSet::new();
    for (index, field) in fields.iter().enumerate() {
        if let FieldKind::Flag(flag) = &field.kind {
            if !controllers.insert(flag.controller.to_string()) {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    format!(
                        "presence controller `{}` cannot define multiple logical groups",
                        flag.controller
                    ),
                ));
            }
            let Some(controller_index) = fields
                .iter()
                .position(|candidate| candidate.name == flag.controller)
            else {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller is not a schema field",
                ));
            };
            if controller_index >= index {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be physically earlier",
                ));
            }
            let FieldKind::Scalar(scalar) = &fields[controller_index].kind else {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be a bool scalar",
                ));
            };
            if !matches!(scalar.value_type, ValueType::Bool) || scalar.constant.is_some() {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be a nonconstant bool scalar",
                ));
            }
            let controller_field = &fields[controller_index];
            if controller_field.layout.pad_before.is_some()
                || controller_field.layout.align_before.is_some()
                || controller_field.layout.position.is_some()
                || controller_field.layout.condition.is_some()
                || controller_field
                    .offset
                    .terms
                    .iter()
                    .any(|term| matches!(term, SizeTerm::Dynamic))
            {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "presence controller must have fixed sequential geometry",
                ));
            }
        }
        if let Some(condition) = &field.layout.condition {
            let Some(flag_index) = fields
                .iter()
                .position(|candidate| candidate.name == *condition)
            else {
                return Err(syn::Error::new_spanned(
                    condition,
                    "condition does not name a logical flag field",
                ));
            };
            if flag_index >= index || !matches!(fields[flag_index].kind, FieldKind::Flag(_)) {
                return Err(syn::Error::new_spanned(
                    condition,
                    "condition must name a physically earlier logical flag field",
                ));
            }
            if field.layout.pad_before.is_some()
                || field.layout.align_before.is_some()
                || field.layout.position.is_some()
            {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "conditional dependent fields cannot declare independent geometry",
                ));
            }
            let FieldKind::Scalar(scalar) = &field.kind else {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "this conditional-group vertical currently requires scalar dependent fields",
                ));
            };
            if scalar.constant.is_some() || scalar.value_type.is_converted() {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "conditional dependent scalars must have direct nonconstant representations",
                ));
            }
        }
    }
    for flag in fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Flag(_)))
    {
        let flag_index = fields
            .iter()
            .position(|field| field.name == flag.name)
            .expect("flag belongs to schema");
        let dependent_indices = fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                (field.layout.condition.as_ref() == Some(&flag.name)).then_some(index)
            })
            .collect::<Vec<_>>();
        let Some(first) = dependent_indices.first().copied() else {
            return Err(syn::Error::new_spanned(
                &flag.name,
                "logical flag has no dependent fields",
            ));
        };
        let last = *dependent_indices.last().expect("nonempty dependent fields");
        if first != flag_index + 1
            || fields[first..=last]
                .iter()
                .any(|field| field.layout.condition.as_ref() != Some(&flag.name))
        {
            return Err(syn::Error::new_spanned(
                &flag.name,
                "conditional group fields must be contiguous immediately after their logical flag",
            ));
        }
    }
    Ok(())
}

fn validate_unsigned_controller(
    fields: &[Field],
    dependent_index: usize,
    controller: &Ident,
    role: &str,
) -> syn::Result<()> {
    let Some(controller_index) = fields.iter().position(|field| field.name == *controller) else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` is not a schema field"),
        ));
    };
    if controller_index >= dependent_index {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be physically earlier"),
        ));
    }
    let controller_field = &fields[controller_index];
    if controller_field.layout.pad_before.is_some()
        || controller_field.layout.align_before.is_some()
        || controller_field.layout.position.is_some()
        || controller_field.layout.condition.is_some()
        || controller_field
            .offset
            .terms
            .iter()
            .any(|term| matches!(term, SizeTerm::Dynamic))
    {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must have fixed sequential geometry"),
        ));
    }
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be an integer scalar"),
        ));
    };
    if !scalar.wire_type.is_unsigned_integer()
        || !matches!(
            scalar.value_type,
            ValueType::Scalar(
                ScalarType::U8
                    | ScalarType::U16
                    | ScalarType::U32
                    | ScalarType::U64
                    | ScalarType::U128
            ) | ValueType::Usize
        )
    {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be an unsigned integer"),
        ));
    }
    if scalar.constant.is_some() {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` cannot be a constant"),
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

pub(super) fn scalar_endian(
    ty: ScalarType,
    declared: Option<Endian>,
    source: &Type,
) -> syn::Result<Endian> {
    if ty.width() == 1 {
        if declared.is_some() {
            return Err(syn::Error::new_spanned(
                source,
                "one-byte scalar fields do not accept an endian attribute",
            ));
        }
        return Ok(Endian::Native);
    }
    declared.ok_or_else(|| {
        syn::Error::new_spanned(
            source,
            "multi-byte scalar wire fields require #[wire(le)] or #[wire(be)]",
        )
    })
}

fn parse_item_attributes(attributes: &[syn::Attribute], owner: &str) -> syn::Result<Vec<Path>> {
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
struct FieldAttributes {
    endian: Option<Endian>,
    constant: Option<Expr>,
    representation: Option<ScalarType>,
    bytes: Option<Ident>,
    rest: bool,
    pad_before: Option<Expr>,
    align_before: Option<Expr>,
    position: Option<Expr>,
    flag: Option<Ident>,
    condition: Option<Ident>,
    counted_by: Option<Ident>,
    bits_of: Option<Ident>,
    bit: Option<u32>,
    bits: Option<(u32, u32)>,
    computed: Option<Expr>,
    try_computed: Option<(Expr, Path)>,
}

impl FieldAttributes {
    fn parse(attributes: &[syn::Attribute]) -> syn::Result<Self> {
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
fn primitive_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments[0].ident.to_string())
}

fn byte_array_len(ty: &Type) -> syn::Result<Option<Expr>> {
    let Type::Array(array) = ty else {
        return Ok(None);
    };

    if primitive_name(&array.elem).as_deref() != Some("u8") {
        return Err(syn::Error::new_spanned(
            ty,
            "fixed wire arrays currently require `u8` elements",
        ));
    }
    Ok(Some(array.len.clone()))
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
fn is_raw_bytes(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let mut segments = path.path.segments.iter().rev();
    matches!(
        (segments.next(), segments.next()),
        (Some(bytes), Some(wire)) if bytes.ident == "Bytes" && wire.ident == "wire"
    )
}
fn array_item_type(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let mut segments = path.path.segments.iter().rev();
    let array = segments.next()?;
    let wire = segments.next()?;
    if array.ident != "Array" || wire.ident != "wire" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &array.arguments else {
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
fn recursive_item_type(ty: &Type) -> Option<Type> {
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
