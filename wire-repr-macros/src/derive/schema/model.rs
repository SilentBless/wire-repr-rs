mod attributes;
mod controller;
mod normalization;
mod validation;

use syn::{Data, DeriveInput, Expr, Fields, Generics, Ident, Path, Type, Visibility};

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
    ScalarArray(FixedScalarArray),
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

pub(super) struct FixedScalarArray {
    pub(super) element: ScalarType,
    pub(super) len: Expr,
    pub(super) endian: Endian,
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
    Scaled(Expr, usize),
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

#[derive(Clone)]
pub(super) enum ValueType {
    Scalar(ScalarType),
    Usize,
    Isize,
    Bool,
    Char,
    Custom(Box<Type>),
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

    pub(super) const fn is_converted(&self) -> bool {
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
            Self::ScalarArray(_)
            | Self::RawBytes(_)
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
            Self::ScalarArray(array) => SizeTerm::Scaled(array.len.clone(), array.element.width()),
            Self::RawBytes(_) | Self::Array(_) | Self::Recursive(_) => SizeTerm::Dynamic,
            Self::Flag(_) | Self::BitProjection(_) => SizeTerm::Fixed(0),
            Self::Nested(nested) => SizeTerm::Nested(nested.ty.clone()),
        }
    }

    pub(super) fn computed(&self) -> Option<&Computed> {
        match self {
            Self::Scalar(scalar) => scalar.computed.as_ref(),
            Self::Bytes(_)
            | Self::ScalarArray(_)
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
        let validators = attributes::parse_item_attributes(&input.attrs, owner)?;
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
            let attributes = attributes::FieldAttributes::parse(&field.attrs)?;
            let parsed_field = normalization::normalize_field(
                name,
                field.ty,
                attributes,
                index,
                field_count,
                &parsed,
                &mut preceding,
            )?;
            parsed.push(parsed_field);
        }
        let computed_order = validation::validate(&parsed)?;
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
                    FieldKind::Scalar(_)
                        | FieldKind::Bytes(_)
                        | FieldKind::ScalarArray(_)
                        | FieldKind::BitProjection(_)
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
            }))
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
            | FieldKind::ScalarArray(_)
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
            | FieldKind::ScalarArray(_)
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
