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
    pub(super) offset: LayoutOffset,
}

pub(super) enum FieldKind {
    Scalar(Scalar),
    Bytes(FixedBytes),
    Nested(NestedField),
}

pub(super) struct FixedBytes {
    pub(super) len: Expr,
    pub(super) constant: Option<Expr>,
}

pub(super) struct NestedField {
    pub(super) ty: Type,
    pub(super) terminal: bool,
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
            Self::Nested(_) => None,
        }
    }

    pub(super) fn size_term(&self) -> SizeTerm {
        match self {
            Self::Scalar(scalar) => SizeTerm::Fixed(scalar.width()),
            Self::Bytes(bytes) => SizeTerm::Expr(bytes.len.clone()),
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
            } = FieldAttributes::parse(&field.attrs)?;
            let ty = field.ty;
            let primitive = primitive_name(&ty);
            let value_type = primitive.as_deref().and_then(ValueType::from_name);
            let kind = if let Some(value_type) = value_type {
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
                FieldKind::Bytes(FixedBytes { len, constant })
            } else {
                if endian.is_some() || constant.is_some() || representation.is_some() {
                    return Err(syn::Error::new_spanned(
                        &ty,
                        "nested schema fields do not accept scalar wire attributes",
                    ));
                }
                FieldKind::Nested(NestedField {
                    ty: ty.clone(),
                    terminal: index + 1 == field_count,
                })
            };

            let offset = LayoutOffset {
                terms: preceding.clone(),
            };
            preceding.push(kind.size_term());
            parsed.push(Field {
                name,
                ty,
                kind,
                offset,
            });
        }

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
        self.size_terms()
            .iter()
            .any(|term| !matches!(term, SizeTerm::Fixed(_)))
    }
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
