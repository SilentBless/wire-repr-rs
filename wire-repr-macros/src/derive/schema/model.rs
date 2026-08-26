use std::collections::BTreeSet;

use syn::{Data, DeriveInput, Expr, Fields, Generics, Ident, Path, Type, Visibility};

pub(super) struct Schema {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) generics: Generics,
    pub(super) fields: Vec<Field>,
    pub(super) validators: Vec<Path>,
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
    Nested(NestedField),
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

pub(super) struct FieldLayout {
    pub(super) pad_before: Option<Expr>,
    pub(super) align_before: Option<Expr>,
    pub(super) position: Option<Position>,
}

pub(super) enum Position {
    Static(Expr),
    Field(Ident),
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

    fn from_name(name: &str) -> Option<Self> {
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
            Self::RawBytes(_) | Self::Nested(_) => None,
        }
    }

    pub(super) fn size_term(&self) -> SizeTerm {
        match self {
            Self::Scalar(scalar) => SizeTerm::Fixed(scalar.width()),
            Self::Bytes(bytes) => SizeTerm::Expr(bytes.len.clone()),
            Self::RawBytes(_) => SizeTerm::Dynamic,
            Self::Nested(nested) => SizeTerm::Nested(nested.ty.clone()),
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
            };
            Self::validate_field_layout(&name, &layout, &preceding)?;
            let ty = field.ty;
            let primitive = primitive_name(&ty);
            let value_type = primitive.as_deref().and_then(ValueType::from_name);
            let kind = if let Some(value_type) = value_type {
                if bytes.is_some() || rest {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "scalar fields do not accept `bytes` or `rest`",
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
                FieldKind::Scalar(Scalar {
                    value_type,
                    wire_type,
                    endian,
                    constant,
                })
            } else if let Some(len) = byte_array_len(&ty)? {
                if endian.is_some() || representation.is_some() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "fixed byte arrays do not accept endian or `as` attributes",
                    ));
                }
                if bytes.is_some() || rest {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "fixed byte arrays do not accept `bytes` or `rest`",
                    ));
                }
                FieldKind::Bytes(FixedBytes { len, constant })
            } else if is_raw_bytes(&ty) {
                if endian.is_some() || representation.is_some() || constant.is_some() {
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
                if endian.is_some() || constant.is_some() || representation.is_some() || rest {
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
            let size = kind.size_term();
            let resets_static_geometry = matches!(size, SizeTerm::Dynamic)
                || layout.pad_before.is_some()
                || layout.align_before.is_some()
                || layout.position.is_some();
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

        validate_geometry_controllers(&parsed)?;

        Ok(Self {
            vis: input.vis,
            name: input.ident,
            generics: input.generics,
            fields: parsed,
            validators,
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

    pub(super) fn layout_can_fail(&self) -> bool {
        self.has_explicit_geometry()
            || self
                .size_terms()
                .iter()
                .any(|term| !matches!(term, SizeTerm::Fixed(_)))
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
            | FieldKind::Nested(_) => false,
        })
    }

    pub(super) fn length_dependent(&self, name: &Ident) -> Option<&Field> {
        self.fields.iter().find(|field| match &field.kind {
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
            | FieldKind::Nested(_) => false,
        })
    }

    pub(super) fn has_explicit_geometry(&self) -> bool {
        self.fields.iter().any(|field| {
            matches!(field.kind, FieldKind::RawBytes(_))
                || matches!(
                    field.kind,
                    FieldKind::Nested(NestedField {
                        extent: Some(_),
                        ..
                    })
                )
                || field.layout.pad_before.is_some()
                || field.layout.align_before.is_some()
                || field.layout.position.is_some()
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
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let mut length_controllers = BTreeSet::new();
    for (index, controller) in &length_roles {
        if !length_controllers.insert(controller.to_string()) {
            return Err(syn::Error::new_spanned(
                *controller,
                format!(
                    "byte length controller `{controller}` cannot control multiple fields before the controller DAG ships"
                ),
            ));
        }
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

fn scalar_endian(ty: ScalarType, declared: Option<Endian>, source: &Type) -> syn::Result<Endian> {
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
                if meta.path.is_ident("at") {
                    if result.position.is_some() {
                        return Err(meta.error("duplicate `at` attribute"));
                    }
                    result.position = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                Err(meta.error("unsupported schema field attribute"))
            })?;
        }
        Ok(result)
    }
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
