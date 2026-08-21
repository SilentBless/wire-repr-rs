//! Derive source model.

use proc_macro2::TokenStream;
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, GenericParam, Generics, Ident, Lifetime, Lit, Path,
    Type, Visibility,
};

pub(super) enum WireType {
    Struct(WireStruct),
    Enum(WireEnum),
    Bitfield(WireBitfield),
}

pub(super) struct WireBitfield {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) storage: TagCodec,
    pub(super) fields: Vec<BitfieldField>,
}

pub(super) struct BitfieldField {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) start: u32,
    pub(super) end: u32,
}

pub(super) struct WireStruct {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) wire_lifetime: Option<Lifetime>,
    pub(super) opcodes: Option<Path>,
    pub(super) fields: Vec<Field>,
}

pub(super) struct WireEnum {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) wire_lifetime: Option<Lifetime>,
    pub(super) tag: EnumTag,
    pub(super) unknown: UnknownPolicy,
    pub(super) opcodes: Option<OpcodeInput>,
    pub(super) variants: Vec<Variant>,
}

pub(super) struct Field {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) kind: FieldKind,
    pub(super) position: Option<FieldPosition>,
    pub(super) padding_before: usize,
    pub(super) alignment_before: Option<usize>,
    pub(super) uses_opcodes: bool,
}

pub(super) enum FieldPosition {
    Static(usize),
    Source(Ident),
}

pub(super) enum FieldKind {
    Fixed(Codec),
    Nested,
    Prefix(Path),
    Bytes { source: Ident, source_index: usize },
    Rest,
}

pub(super) struct Variant {
    pub(super) name: Ident,
    pub(super) selector: VariantSelector,
    pub(super) opcode: Option<Path>,
    pub(super) body: Option<Type>,
}

pub(super) enum VariantSelector {
    Integer(u128),
    Bytes(Vec<u8>),
    Unknown,
    Dynamic,
}

pub(super) enum EnumTag {
    Integer(TagCodec),
    Bytes { width: usize },
}

#[derive(Clone, Copy)]
pub(super) enum UnknownPolicy {
    Reject,
    Preserve,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum SelectorKey {
    Integer(u128),
    Bytes(Vec<u8>),
    Unknown,
    Opcode(String),
}

pub(super) struct OpcodeInput {
    pub(super) table: Path,
    pub(super) error: Path,
}

pub(super) enum Codec {
    Builtin(&'static str),
    OwnedBytes(TokenStream),
    Custom(Path),
}

pub(super) struct TagCodec {
    pub(super) codec: &'static str,
    pub(super) ty: &'static str,
    pub(super) max: u128,
}

impl WireType {
    pub(super) fn parse(input: DeriveInput) -> syn::Result<Self> {
        match input.data {
            Data::Struct(data) => Self::parse_struct(
                input.attrs,
                input.generics,
                input.vis,
                input.ident,
                data.fields,
            ),
            Data::Enum(data) => Self::parse_enum(
                input.attrs,
                input.generics,
                input.vis,
                input.ident,
                data.variants,
            ),
            _ => Err(syn::Error::new_spanned(
                input.ident,
                "Wire supports named structs and enums only",
            )),
        }
    }

    fn parse_struct(
        attributes: Vec<Attribute>,
        generics: Generics,
        vis: Visibility,
        name: Ident,
        fields: Fields,
    ) -> syn::Result<Self> {
        let attributes = parse_struct_attributes(&attributes)?;
        if let Some(storage) = attributes.bitfield {
            return parse_bitfield(generics, vis, name, fields, storage);
        }
        let opcodes = attributes.opcodes;
        let wire_lifetime = parse_wire_lifetime(&generics, "Wire structs")?;
        let Fields::Named(fields) = fields else {
            return Err(syn::Error::new_spanned(
                name,
                "Wire supports named structs only",
            ));
        };
        if fields.named.is_empty() {
            return Err(syn::Error::new_spanned(
                name,
                "Wire requires at least one field",
            ));
        }

        let mut fields = fields
            .named
            .into_iter()
            .map(Field::parse)
            .collect::<syn::Result<Vec<_>>>()?;
        validate_byte_fields(&mut fields, wire_lifetime.as_ref())?;
        validate_positions(&mut fields)?;
        validate_opcode_fields(&fields, opcodes.as_ref())?;
        for (index, field) in fields.iter().enumerate() {
            if matches!(field.kind, FieldKind::Rest) {
                if index + 1 != fields.len() {
                    return Err(syn::Error::new_spanned(
                        &field.name,
                        "#[wire(rest)] must be the final field",
                    ));
                }
                if !is_borrowed_byte_slice(&field.ty, wire_lifetime.as_ref()) {
                    return Err(syn::Error::new_spanned(
                        &field.ty,
                        "#[wire(rest)] requires a shared byte slice using the struct wire lifetime",
                    ));
                }
            }
        }

        Ok(Self::Struct(WireStruct {
            vis,
            name,
            wire_lifetime,
            opcodes,
            fields,
        }))
    }

    fn parse_enum(
        attributes: Vec<Attribute>,
        generics: Generics,
        vis: Visibility,
        name: Ident,
        variants: syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>,
    ) -> syn::Result<Self> {
        let wire_lifetime = parse_wire_lifetime(&generics, "Wire enums")?;
        let attributes = parse_enum_attributes(&attributes)?;
        let tag = attributes.tag.ok_or_else(|| {
            syn::Error::new_spanned(&name, "Wire enums require #[wire(tag = CODEC)]")
        })?;
        let unknown = attributes.unknown.ok_or_else(|| {
            syn::Error::new_spanned(
                &name,
                "Wire enums require an explicit unknown policy; add #[wire(unknown = reject)] or #[wire(unknown = preserve)]",
            )
        })?;
        if variants.is_empty() {
            return Err(syn::Error::new_spanned(
                name,
                "Wire enums require at least one variant",
            ));
        }

        let opcodes = match (attributes.opcodes, attributes.opcode_error) {
            (Some(table), Some(error)) => Some(OpcodeInput { table, error }),
            (Some(_), None) => {
                return Err(syn::Error::new_spanned(
                    &name,
                    "dynamic opcode enums require `opcode_error = ErrorType`",
                ));
            }
            (None, Some(error)) => {
                return Err(syn::Error::new_spanned(
                    error,
                    "`opcode_error` requires `opcodes = OpcodeTable`",
                ));
            }
            (None, None) => None,
        };
        if matches!(tag, EnumTag::Bytes { .. }) && opcodes.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "dynamic opcode enums cannot use fixed byte tag selectors",
            ));
        }
        if matches!(unknown, UnknownPolicy::Preserve) && opcodes.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "dynamic opcode enums currently require `unknown = reject`",
            ));
        }

        let mut selectors = BTreeSet::new();
        let variants = variants
            .into_iter()
            .map(|variant| Variant::parse(variant, &tag, opcodes.is_some(), &mut selectors))
            .collect::<syn::Result<Vec<_>>>()?;
        let has_unknown = variants
            .iter()
            .any(|variant| matches!(variant.selector, VariantSelector::Unknown));
        match (unknown, has_unknown) {
            (UnknownPolicy::Reject, true) => {
                return Err(syn::Error::new_spanned(
                    &name,
                    "`unknown = reject` cannot declare a #[wire(unknown)] variant",
                ));
            }
            (UnknownPolicy::Preserve, false) => {
                return Err(syn::Error::new_spanned(
                    &name,
                    "`unknown = preserve` requires exactly one #[wire(unknown)] variant",
                ));
            }
            _ => {}
        }

        Ok(Self::Enum(WireEnum {
            vis,
            name,
            wire_lifetime,
            tag,
            unknown,
            opcodes,
            variants,
        }))
    }
}

fn parse_wire_lifetime(generics: &Generics, owner: &str) -> syn::Result<Option<Lifetime>> {
    if generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            generics,
            format!("{owner} do not support where clauses"),
        ));
    }

    let mut lifetime = None;
    for parameter in &generics.params {
        match parameter {
            GenericParam::Lifetime(parameter)
                if lifetime.is_none() && parameter.bounds.is_empty() =>
            {
                lifetime = Some(parameter.lifetime.clone());
            }
            GenericParam::Lifetime(_) => {
                return Err(syn::Error::new_spanned(
                    parameter,
                    format!("{owner} support exactly one unbounded wire lifetime"),
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    parameter,
                    format!("{owner} do not support type or const parameters"),
                ));
            }
        }
    }
    Ok(lifetime)
}

impl Field {
    fn parse(field: syn::Field) -> syn::Result<Self> {
        let Some(name) = field.ident else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "Wire supports named fields only",
            ));
        };
        let attributes = parse_field_wire_attributes(&field.attrs)?;
        let kind = match attributes.representation {
            WireAttribute::Custom(path) => FieldKind::Fixed(Codec::Custom(path)),
            WireAttribute::Endian(big_endian) => {
                FieldKind::Fixed(builtin_codec(&field.ty, Some(big_endian)).ok_or_else(|| {
                    syn::Error::new_spanned(
                        &field.ty,
                        "wire endian attributes require a multi-byte integer field",
                    )
                })?)
            }
            WireAttribute::Prefix(path) => FieldKind::Prefix(path),
            WireAttribute::Bytes(source) => FieldKind::Bytes {
                source,
                source_index: usize::MAX,
            },
            WireAttribute::Rest => FieldKind::Rest,
            WireAttribute::None => match builtin_codec(&field.ty, None) {
                Some(codec) => FieldKind::Fixed(codec),
                None if is_multibyte_integer(&field.ty) => {
                    return Err(syn::Error::new_spanned(
                        &field.ty,
                        "multi-byte integer fields require #[wire(be)] or #[wire(le)]",
                    ));
                }
                None => FieldKind::Nested,
            },
        };

        Ok(Self {
            name,
            ty: field.ty,
            kind,
            position: attributes.position,
            padding_before: attributes.padding_before.unwrap_or(0),
            alignment_before: attributes.alignment_before,
            uses_opcodes: attributes.uses_opcodes,
        })
    }
}

impl Variant {
    fn parse(
        variant: syn::Variant,
        tag: &EnumTag,
        dynamic_opcodes: bool,
        selectors: &mut BTreeSet<SelectorKey>,
    ) -> syn::Result<Self> {
        let syn::Variant {
            attrs,
            ident: name,
            fields,
            discriminant,
            ..
        } = variant;
        let attributes = parse_variant_attributes(&attrs)?;
        let body = parse_variant_body(fields)?;

        let (selector, opcode) = if dynamic_opcodes {
            if discriminant.is_some() {
                return Err(syn::Error::new_spanned(
                    &name,
                    "dynamic opcode variants use #[wire(opcode = Opcode::Variant)], not Rust discriminants",
                ));
            }
            if attributes.byte_tag.is_some() || attributes.unknown {
                return Err(syn::Error::new_spanned(
                    &name,
                    "dynamic opcode variants cannot use byte tag selectors or #[wire(unknown)]",
                ));
            }
            let opcode = attributes.opcode.ok_or_else(|| {
                syn::Error::new_spanned(
                    &name,
                    "dynamic opcode variants require #[wire(opcode = Opcode::Variant)]",
                )
            })?;
            let key = opcode.to_token_stream().to_string();
            if !selectors.insert(SelectorKey::Opcode(key)) {
                return Err(syn::Error::new_spanned(
                    &opcode,
                    "opcode selector duplicates an earlier variant",
                ));
            }
            (VariantSelector::Dynamic, Some(opcode))
        } else if attributes.unknown {
            if discriminant.is_some()
                || attributes.byte_tag.is_some()
                || attributes.opcode.is_some()
            {
                return Err(syn::Error::new_spanned(
                    &name,
                    "#[wire(unknown)] cannot also declare a discriminant, tag, or opcode selector",
                ));
            }
            validate_unknown_variant(&name, body.as_ref(), tag)?;
            if !selectors.insert(SelectorKey::Unknown) {
                return Err(syn::Error::new_spanned(
                    &name,
                    "only one #[wire(unknown)] variant is allowed",
                ));
            }
            (VariantSelector::Unknown, None)
        } else {
            if let Some(opcode) = attributes.opcode {
                return Err(syn::Error::new_spanned(
                    opcode,
                    "#[wire(opcode = ...)] requires an enum with `opcodes = OpcodeTable`",
                ));
            }
            match tag {
                EnumTag::Bytes { width } => {
                    if let Some((_, discriminant)) = discriminant {
                        return Err(syn::Error::new_spanned(
                            discriminant,
                            "fixed byte tag enums use #[wire(tag = b\"...\")] selectors, not Rust discriminants",
                        ));
                    }
                    let literal = attributes.byte_tag.ok_or_else(|| {
                        syn::Error::new_spanned(
                            &name,
                            "fixed byte tag variants require #[wire(tag = b\"...\")] or #[wire(unknown)]",
                        )
                    })?;
                    let bytes = literal.value();
                    if bytes.len() != *width {
                        return Err(syn::Error::new_spanned(
                            literal,
                            "byte tag selector width does not match the enum tag representation",
                        ));
                    }
                    if !selectors.insert(SelectorKey::Bytes(bytes.clone())) {
                        return Err(syn::Error::new_spanned(
                            literal,
                            "byte tag selector duplicates an earlier variant",
                        ));
                    }
                    (VariantSelector::Bytes(bytes), None)
                }
                EnumTag::Integer(codec) => {
                    if let Some(literal) = attributes.byte_tag {
                        return Err(syn::Error::new_spanned(
                            literal,
                            "#[wire(tag = b\"...\")] is valid only for fixed byte tag enums",
                        ));
                    }
                    let (_, discriminant) = discriminant.ok_or_else(|| {
                        syn::Error::new_spanned(
                            &name,
                            "Wire enum variants require an explicit integer literal discriminant",
                        )
                    })?;
                    let Expr::Lit(expression) = discriminant else {
                        return Err(syn::Error::new_spanned(
                            discriminant,
                            "Wire enum discriminants must be integer literals",
                        ));
                    };
                    let Lit::Int(literal) = expression.lit else {
                        return Err(syn::Error::new_spanned(
                            expression,
                            "Wire enum discriminants must be integer literals",
                        ));
                    };
                    let value = literal.base10_parse::<u128>().map_err(|_| {
                        syn::Error::new_spanned(
                            &literal,
                            "Wire enum discriminants must be non-negative integer literals",
                        )
                    })?;
                    if value > codec.max {
                        return Err(syn::Error::new_spanned(
                            literal,
                            "Wire enum discriminant is not representable by its tag codec",
                        ));
                    }
                    if !selectors.insert(SelectorKey::Integer(value)) {
                        return Err(syn::Error::new_spanned(
                            literal,
                            "Wire enum discriminant duplicates an earlier tag",
                        ));
                    }
                    (VariantSelector::Integer(value), None)
                }
            }
        };

        Ok(Self {
            name,
            selector,
            opcode,
            body,
        })
    }
}

fn parse_variant_body(fields: Fields) -> syn::Result<Option<Type>> {
    match fields {
        Fields::Unit => Ok(None),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let field = fields.unnamed.into_iter().next().expect("one field");
            reject_wire_attributes(
                &field.attrs,
                "Wire enum variant bodies do not support field-level #[wire(...)] attributes",
            )?;
            Ok(Some(field.ty))
        }
        Fields::Unnamed(fields) => Err(syn::Error::new_spanned(
            fields,
            "Wire enum tuple variants require exactly one field",
        )),
        Fields::Named(fields) => Err(syn::Error::new_spanned(
            fields,
            "Wire enum named variants are not supported",
        )),
    }
}

fn validate_unknown_variant(name: &Ident, body: Option<&Type>, tag: &EnumTag) -> syn::Result<()> {
    let valid = match (tag, body) {
        (EnumTag::Bytes { width }, Some(Type::Array(array))) => {
            is_plain_u8(array.elem.as_ref()) && fixed_array_len(array).ok() == Some(*width)
        }
        (EnumTag::Integer(codec), Some(Type::Path(path))) => {
            path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == codec.ty
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            name,
            "#[wire(unknown)] requires one raw-tag field matching the enum tag representation",
        ))
    }
}

struct VariantAttributes {
    opcode: Option<Path>,
    byte_tag: Option<syn::LitByteStr>,
    unknown: bool,
}

fn parse_variant_attributes(attributes: &[Attribute]) -> syn::Result<VariantAttributes> {
    let mut result = VariantAttributes {
        opcode: None,
        byte_tag: None,
        unknown: false,
    };
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("opcode") {
                if result.opcode.is_some() {
                    return Err(meta.error("duplicate variant opcode selector"));
                }
                result.opcode = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("tag") {
                if result.byte_tag.is_some() {
                    return Err(meta.error("duplicate variant byte tag selector"));
                }
                let literal: Lit = meta.value()?.parse()?;
                let Lit::ByteStr(literal) = literal else {
                    return Err(syn::Error::new_spanned(
                        literal,
                        "variant byte tag selectors must be byte string literals",
                    ));
                };
                result.byte_tag = Some(literal);
                Ok(())
            } else if meta.path.is_ident("unknown") && meta.input.is_empty() {
                if result.unknown {
                    return Err(meta.error("duplicate #[wire(unknown)] selector"));
                }
                result.unknown = true;
                Ok(())
            } else {
                Err(meta.error(
                    "unsupported Wire enum variant option; expected `tag = b\"...\"`, `unknown`, or `opcode = Path`",
                ))
            }
        })?;
    }
    Ok(result)
}

enum WireAttribute {
    None,
    Endian(bool),
    Custom(Path),
    Prefix(Path),
    Bytes(Ident),
    Rest,
}

struct FieldWireAttributes {
    representation: WireAttribute,
    padding_before: Option<usize>,
    alignment_before: Option<usize>,
    position: Option<FieldPosition>,
    uses_opcodes: bool,
}

fn parse_field_wire_attributes(attributes: &[Attribute]) -> syn::Result<FieldWireAttributes> {
    let mut result = FieldWireAttributes {
        representation: WireAttribute::None,
        padding_before: None,
        alignment_before: None,
        position: None,
        uses_opcodes: false,
    };
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        let mut saw_option = false;
        attribute.parse_nested_meta(|meta| {
            saw_option = true;
            if meta.path.is_ident("at") {
                if result.position.is_some() {
                    return Err(meta.error("duplicate `at`"));
                }
                let expression: Expr = meta.value()?.parse()?;
                result.position = Some(match expression {
                    Expr::Path(path) if path.path.get_ident().is_some() => {
                        FieldPosition::Source(path.path.get_ident().expect("checked").clone())
                    }
                    Expr::Lit(literal) => {
                        let Lit::Int(value) = literal.lit else {
                            return Err(syn::Error::new_spanned(
                                literal,
                                "`at` expects a byte position or an earlier unsigned field",
                            ));
                        };
                        FieldPosition::Static(value.base10_parse()?)
                    }
                    expression => {
                        return Err(syn::Error::new_spanned(
                            expression,
                            "`at` expects a byte position or an earlier unsigned field",
                        ));
                    }
                });
                Ok(())
            } else if meta.path.is_ident("pad_before") {
                if result.padding_before.is_some() {
                    return Err(meta.error("duplicate `pad_before`"));
                }
                result.padding_before = Some(parse_nonzero_usize(&meta, "pad_before")?);
                Ok(())
            } else if meta.path.is_ident("align_before") {
                if result.alignment_before.is_some() {
                    return Err(meta.error("duplicate `align_before`"));
                }
                result.alignment_before = Some(parse_nonzero_usize(&meta, "align_before")?);
                Ok(())
            } else if meta.path.is_ident("opcodes") && meta.input.is_empty() {
                if result.uses_opcodes {
                    return Err(meta.error("duplicate `opcodes`"));
                }
                result.uses_opcodes = true;
                Ok(())
            } else {
                if !matches!(result.representation, WireAttribute::None) {
                    return Err(meta.error("only one wire representation strategy is allowed per field"));
                }
                if meta.path.is_ident("be") {
                    result.representation = WireAttribute::Endian(true);
                    Ok(())
                } else if meta.path.is_ident("le") {
                    result.representation = WireAttribute::Endian(false);
                    Ok(())
                } else if meta.path.is_ident("codec") {
                    result.representation = WireAttribute::Custom(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("prefix") {
                    result.representation = WireAttribute::Prefix(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("bytes") {
                    result.representation = WireAttribute::Bytes(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("rest") {
                    result.representation = WireAttribute::Rest;
                    Ok(())
                } else {
                    Err(meta.error(
                        "expected `be`, `le`, `codec = Path`, `prefix = Path`, `bytes = source_field`, `rest`, `at = N`, `at = source_field`, `pad_before = N`, `align_before = N`, or `opcodes`",
                    ))
                }
            }
        })?;
        if !saw_option {
            return Err(syn::Error::new_spanned(
                attribute,
                "#[wire(...)] requires at least one field option",
            ));
        }
    }
    Ok(result)
}

fn parse_nonzero_usize(meta: &syn::meta::ParseNestedMeta<'_>, name: &str) -> syn::Result<usize> {
    let literal: syn::LitInt = meta.value()?.parse()?;
    let value = literal.base10_parse::<usize>()?;
    if value == 0 {
        Err(syn::Error::new_spanned(
            literal,
            format!("`{name}` must be nonzero"),
        ))
    } else {
        Ok(value)
    }
}

fn validate_byte_fields(fields: &mut [Field], wire_lifetime: Option<&Lifetime>) -> syn::Result<()> {
    let mut controlled_sources = BTreeSet::new();

    for index in 0..fields.len() {
        let FieldKind::Bytes { source, .. } = &fields[index].kind else {
            continue;
        };
        let source = source.clone();
        if !is_borrowed_byte_slice(&fields[index].ty, wire_lifetime) {
            return Err(syn::Error::new_spanned(
                &fields[index].ty,
                "#[wire(bytes = source_field)] requires a shared byte slice using the struct wire lifetime",
            ));
        }
        let source_index = fields[..index]
            .iter()
            .position(|field| field.name == source)
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    &source,
                    "byte length source must name an earlier field in the same struct",
                )
            })?;
        let valid_source = match &fields[source_index].kind {
            FieldKind::Fixed(Codec::Builtin(codec)) => matches!(
                *codec,
                "U8" | "BeU16"
                    | "LeU16"
                    | "BeU32"
                    | "LeU32"
                    | "BeU64"
                    | "LeU64"
                    | "BeU128"
                    | "LeU128"
            ),
            FieldKind::Prefix(_) => is_unsigned_integer(&fields[source_index].ty),
            FieldKind::Fixed(Codec::OwnedBytes(_) | Codec::Custom(_))
            | FieldKind::Nested
            | FieldKind::Bytes { .. }
            | FieldKind::Rest => false,
        };
        if !valid_source {
            return Err(syn::Error::new_spanned(
                &source,
                "byte length source must be an unsigned integer encoded by a built-in fixed or prefix codec",
            ));
        }
        if !controlled_sources.insert(source_index) {
            return Err(syn::Error::new_spanned(
                &source,
                "a byte length source may control only one field",
            ));
        }
        fields[index].kind = FieldKind::Bytes {
            source,
            source_index,
        };
    }
    Ok(())
}

fn validate_positions(fields: &mut [Field]) -> syn::Result<()> {
    let mut controlled_sources = BTreeSet::new();
    for field in fields.iter() {
        if let FieldKind::Bytes { source_index, .. } = field.kind {
            controlled_sources.insert(source_index);
        }
    }

    for index in 0..fields.len() {
        let Some(position) = &fields[index].position else {
            continue;
        };
        if fields[index].padding_before != 0 || fields[index].alignment_before.is_some() {
            return Err(syn::Error::new_spanned(
                &fields[index].name,
                "`at` cannot be combined with `pad_before` or `align_before`",
            ));
        }
        let FieldPosition::Source(source) = position else {
            continue;
        };
        let source = source.clone();
        let source_index = fields[..index]
            .iter()
            .position(|field| field.name == source)
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    &source,
                    "position source must name an earlier field in the same struct",
                )
            })?;
        let FieldKind::Fixed(Codec::Builtin(codec)) = &fields[source_index].kind else {
            return Err(syn::Error::new_spanned(
                &source,
                "position source must be a built-in unsigned integer field",
            ));
        };
        if !matches!(
            *codec,
            "U8" | "BeU16" | "LeU16" | "BeU32" | "LeU32" | "BeU64" | "LeU64" | "BeU128" | "LeU128"
        ) {
            return Err(syn::Error::new_spanned(
                &source,
                "position source must be a built-in unsigned integer field",
            ));
        }
        if !controlled_sources.insert(source_index) {
            return Err(syn::Error::new_spanned(
                &source,
                "a geometry source may control only one field",
            ));
        }
        fields[index].position = Some(FieldPosition::Source(source));
    }
    Ok(())
}

fn reject_wire_attributes(attributes: &[Attribute], message: &str) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| attribute.path().is_ident("wire"))
    {
        Err(syn::Error::new_spanned(attribute, message))
    } else {
        Ok(())
    }
}

struct EnumAttributes {
    tag: Option<EnumTag>,
    unknown: Option<UnknownPolicy>,
    opcodes: Option<Path>,
    opcode_error: Option<Path>,
}

struct StructAttributes {
    opcodes: Option<Path>,
    bitfield: Option<TagCodec>,
}

fn parse_struct_attributes(attributes: &[Attribute]) -> syn::Result<StructAttributes> {
    let mut opcodes = None;
    let mut bitfield_type = None;
    let mut endian = None;
    let mut reserved_zero = false;
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("opcodes") {
                if opcodes.is_some() {
                    return Err(meta.error("duplicate struct opcode input"));
                }
                opcodes = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("bitfield") {
                if bitfield_type.is_some() {
                    return Err(meta.error("duplicate bitfield storage type"));
                }
                bitfield_type = Some(meta.value()?.parse::<Type>()?);
                return Ok(());
            }
            if meta.path.is_ident("be") || meta.path.is_ident("le") {
                if endian.is_some() {
                    return Err(meta.error("duplicate bitfield byte order"));
                }
                endian = Some(meta.path.is_ident("be"));
                return Ok(());
            }
            if meta.path.is_ident("reserved") {
                if reserved_zero {
                    return Err(meta.error("duplicate reserved-bit policy"));
                }
                let policy: Ident = meta.value()?.parse()?;
                if policy != "zero" {
                    return Err(syn::Error::new_spanned(
                        policy,
                        "unsupported reserved-bit policy; use `reserved = zero`",
                    ));
                }
                reserved_zero = true;
                return Ok(());
            }
            Err(meta.error(
                "unsupported Wire struct option; expected `opcodes = OpcodeTable` or bitfield options",
            ))
        })?;
    }

    let bitfield = match bitfield_type {
        Some(ty) => {
            if opcodes.is_some() {
                return Err(syn::Error::new_spanned(
                    ty,
                    "bitfields cannot declare opcode inputs",
                ));
            }
            if !reserved_zero {
                return Err(syn::Error::new_spanned(
                    ty,
                    "bitfields require an explicit reserved-bit policy; add `reserved = zero`",
                ));
            }
            Some(parse_bitfield_storage(&ty, endian)?)
        }
        None => {
            if endian.is_some() || reserved_zero {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "bitfield byte order and reserved-bit policy require `bitfield = unsigned_type`",
                ));
            }
            None
        }
    };

    Ok(StructAttributes { opcodes, bitfield })
}

fn parse_bitfield_storage(ty: &Type, endian: Option<bool>) -> syn::Result<TagCodec> {
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

fn parse_bitfield(
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

fn validate_opcode_fields(fields: &[Field], opcodes: Option<&Path>) -> syn::Result<()> {
    for field in fields {
        if !field.uses_opcodes {
            continue;
        }
        if opcodes.is_none() {
            return Err(syn::Error::new_spanned(
                &field.name,
                "#[wire(opcodes)] requires #[wire(opcodes = OpcodeTable)] on the struct",
            ));
        }
        if !matches!(field.kind, FieldKind::Nested) {
            return Err(syn::Error::new_spanned(
                &field.name,
                "#[wire(opcodes)] is valid only on nested wire fields",
            ));
        }
    }
    Ok(())
}

fn parse_enum_attributes(attributes: &[Attribute]) -> syn::Result<EnumAttributes> {
    let mut result = EnumAttributes {
        tag: None,
        unknown: None,
        opcodes: None,
        opcode_error: None,
    };
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                if result.tag.is_some() {
                    return Err(meta.error("duplicate enum tag codec"));
                }
                let representation: Type = meta.value()?.parse()?;
                result.tag = Some(parse_enum_tag(representation)?);
                return Ok(());
            }
            if meta.path.is_ident("unknown") {
                if result.unknown.is_some() {
                    return Err(meta.error("duplicate enum unknown policy"));
                }
                let policy: Ident = meta.value()?.parse()?;
                result.unknown = Some(if policy == "reject" {
                    UnknownPolicy::Reject
                } else if policy == "preserve" {
                    UnknownPolicy::Preserve
                } else {
                    return Err(syn::Error::new_spanned(
                        policy,
                        "unsupported unknown policy; use `unknown = reject` or `unknown = preserve`",
                    ));
                });
                return Ok(());
            }
            if meta.path.is_ident("opcodes") {
                if result.opcodes.is_some() {
                    return Err(meta.error("duplicate enum opcode input"));
                }
                result.opcodes = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("opcode_error") {
                if result.opcode_error.is_some() {
                    return Err(meta.error("duplicate enum opcode error"));
                }
                result.opcode_error = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error(
                "unsupported Wire enum option; expected `tag`, `unknown`, `opcodes`, or `opcode_error`",
            ))
        })?;
    }
    Ok(result)
}

fn parse_enum_tag(representation: Type) -> syn::Result<EnumTag> {
    match representation {
        Type::Path(path) if path.qself.is_none() => {
            parse_tag_codec(path.path).map(EnumTag::Integer)
        }
        Type::Array(array) => parse_fixed_byte_tag(array),
        representation => Err(syn::Error::new_spanned(
            representation,
            "Wire enum tags must use a built-in unsigned fixed integer codec or [u8; N]",
        )),
    }
}

fn parse_fixed_byte_tag(array: syn::TypeArray) -> syn::Result<EnumTag> {
    if !is_plain_u8(array.elem.as_ref()) {
        return Err(syn::Error::new_spanned(
            array,
            "fixed byte enum tags must use [u8; N]",
        ));
    }
    let width = fixed_array_len(&array)?;
    if width == 0 {
        return Err(syn::Error::new_spanned(
            array,
            "fixed byte enum tag width must be nonzero",
        ));
    }
    Ok(EnumTag::Bytes { width })
}

fn fixed_array_len(array: &syn::TypeArray) -> syn::Result<usize> {
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

fn parse_tag_codec(path: Path) -> syn::Result<TagCodec> {
    let Some(segment) = path.segments.first().filter(|_| path.segments.len() == 1) else {
        return Err(syn::Error::new_spanned(
            path,
            "Wire enum tags must use a built-in unsigned fixed integer codec",
        ));
    };
    match segment.ident.to_string().as_str() {
        "U8" => Ok(TagCodec {
            codec: "U8",
            ty: "u8",
            max: u8::MAX as u128,
        }),
        "BeU16" => Ok(TagCodec {
            codec: "BeU16",
            ty: "u16",
            max: u16::MAX as u128,
        }),
        "LeU16" => Ok(TagCodec {
            codec: "LeU16",
            ty: "u16",
            max: u16::MAX as u128,
        }),
        "BeU32" => Ok(TagCodec {
            codec: "BeU32",
            ty: "u32",
            max: u32::MAX as u128,
        }),
        "LeU32" => Ok(TagCodec {
            codec: "LeU32",
            ty: "u32",
            max: u32::MAX as u128,
        }),
        "BeU64" => Ok(TagCodec {
            codec: "BeU64",
            ty: "u64",
            max: u64::MAX as u128,
        }),
        "LeU64" => Ok(TagCodec {
            codec: "LeU64",
            ty: "u64",
            max: u64::MAX as u128,
        }),
        "BeU128" => Ok(TagCodec {
            codec: "BeU128",
            ty: "u128",
            max: u128::MAX,
        }),
        "LeU128" => Ok(TagCodec {
            codec: "LeU128",
            ty: "u128",
            max: u128::MAX,
        }),
        _ => Err(syn::Error::new_spanned(
            path,
            "Wire enum tags must use a built-in unsigned fixed integer codec",
        )),
    }
}

fn builtin_codec(ty: &Type, endianness: Option<bool>) -> Option<Codec> {
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

fn is_plain_u8(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none() && path.path.is_ident("u8")
}

fn is_unsigned_integer(ty: &Type) -> bool {
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

fn is_multibyte_integer(ty: &Type) -> bool {
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

fn is_borrowed_byte_slice(ty: &Type, wire_lifetime: Option<&Lifetime>) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{
        Codec, EnumTag, FieldKind, FieldPosition, UnknownPolicy, VariantSelector, WireType,
    };

    fn parse(source: &str) -> syn::Result<WireType> {
        WireType::parse(syn::parse_str(source).unwrap())
    }

    #[test]
    fn classifies_fixed_and_nested_fields() {
        let WireType::Struct(model) = parse("struct Packet { a: u8, b: i8, #[wire(be)] c: u16, #[wire(le)] d: i128, #[wire(codec = Custom)] e: Value, #[wire(prefix = Varint)] varint: u32, bytes: [u8; 32], child: Child }").unwrap() else { panic!() };
        assert!(matches!(
            model.fields[0].kind,
            FieldKind::Fixed(Codec::Builtin("U8"))
        ));
        assert!(matches!(
            model.fields[1].kind,
            FieldKind::Fixed(Codec::Builtin("I8"))
        ));
        assert!(matches!(
            model.fields[2].kind,
            FieldKind::Fixed(Codec::Builtin("BeU16"))
        ));
        assert!(matches!(
            model.fields[3].kind,
            FieldKind::Fixed(Codec::Builtin("LeI128"))
        ));
        assert!(matches!(
            model.fields[4].kind,
            FieldKind::Fixed(Codec::Custom(_))
        ));
        assert!(matches!(model.fields[5].kind, FieldKind::Prefix(_)));
        assert!(matches!(
            model.fields[6].kind,
            FieldKind::Fixed(Codec::OwnedBytes(_))
        ));
        assert!(matches!(model.fields[7].kind, FieldKind::Nested));
    }

    #[test]
    fn rejects_unannotated_multibyte_integers() {
        for ty in ["u16", "i16", "u32", "i32", "u64", "i64", "u128", "i128"] {
            assert!(parse(&format!("struct Packet {{ value: {ty} }}")).is_err());
        }
    }

    #[test]
    fn accepts_valid_enum_source_model() {
        let WireType::Enum(model) =
            parse("#[wire(tag = BeU16, unknown = reject)] enum Op { Ping = 1, Data(Body) = 2 }")
                .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.variants[0].selector,
            VariantSelector::Integer(1)
        ));
        assert!(matches!(model.tag, EnumTag::Integer(_)));
        assert!(matches!(model.unknown, UnknownPolicy::Reject));
        let Err(error) = parse("#[wire(tag = U8)] enum Op { Ping = 1 }") else {
            panic!("missing unknown policy must be rejected");
        };
        assert!(error.to_string().contains("explicit unknown policy"));
        assert!(parse("#[wire(tag = U8, unknown = preserve)] enum Op { Ping = 1 }").is_err());
        assert!(parse("#[wire(tag = U8, unknown = preserve)] #[repr(u8)] enum OpenOp { Ping = 1, #[wire(unknown)] Other(u8) }").is_ok());
    }

    #[test]
    fn accepts_fixed_byte_enum_selectors_and_unknown_capture() {
        let WireType::Enum(model) = parse(
            "#[wire(tag = [u8; 2], unknown = preserve)] enum Op { #[wire(tag = b\"OK\")] Ok, #[wire(tag = b\"NO\")] No(Body), #[wire(unknown)] Other([u8; 2]) }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(model.tag, EnumTag::Bytes { width: 2 }));
        assert!(matches!(model.unknown, UnknownPolicy::Preserve));
        assert!(
            matches!(model.variants[0].selector, VariantSelector::Bytes(ref value) if value == b"OK")
        );
        assert!(
            matches!(model.variants[1].selector, VariantSelector::Bytes(ref value) if value == b"NO")
        );
        assert!(matches!(
            model.variants[2].selector,
            VariantSelector::Unknown
        ));
    }

    #[test]
    fn rejects_invalid_fixed_byte_enum_selectors() {
        for source in [
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"A\")] A }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A, #[wire(tag = b\"OK\")] B }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { A = 1 }",
            "#[wire(tag = [u8; 2])] enum Op { #[wire(tag = b\"OK\")] A }",
            "#[wire(tag = [u8; 2], unknown = preserve)] enum Op { #[wire(tag = b\"OK\")] A }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A, #[wire(unknown)] Other([u8; 2]) }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { A }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] A([u8; 2]), #[wire(unknown)] B([u8; 2]) }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] Other }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] Other([u8; 1]) }",
            "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\", unknown)] Other([u8; 2]) }",
            "#[wire(tag = [u8; 0], unknown = reject)] enum Op { #[wire(tag = b\"\")] Empty }",
            "#[wire(tag = [u16; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A }",
            "#[wire(tag = [u8; WIDTH], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A }",
            "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(tag = b\"A\")] A = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(unknown)] Other([u8; 1]) }",
            "#[wire(tag = [u8; 2], opcodes = OpcodeTable, opcode_error = OpcodeError, unknown = reject)] enum Op { #[wire(opcode = Opcode::Ok)] Ok }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_bounded_byte_fields_with_earlier_unsigned_sources() {
        let WireType::Struct(model) = parse(
            "struct Packet<'wire> { length: u8, #[wire(bytes = length)] payload: &'wire [u8], tail: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.fields[1].kind,
            FieldKind::Bytes {
                source_index: 0,
                ..
            }
        ));
    }

    #[test]
    fn accepts_forward_positions_and_rejects_ambiguous_sources() {
        let WireType::Struct(model) = parse(
            "struct Packet<'wire> { offset: u8, length: u8, #[wire(at = offset, bytes = length)] payload: &'wire [u8] }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.fields[2].position.as_ref().expect("position"),
            FieldPosition::Source(source) if source == "offset"
        ));

        let WireType::Struct(model) =
            parse("struct Header { kind: u8, #[wire(at = 8)] value: u8 }").unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.fields[1].position,
            Some(FieldPosition::Static(8))
        ));

        for source in [
            "struct Packet { #[wire(at = missing)] value: u8 }",
            "struct Packet { #[wire(at = offset)] value: u8, offset: u8 }",
            "struct Packet { offset: i8, #[wire(at = offset)] value: u8 }",
            "struct Packet { offset: u8, #[wire(at = offset, pad_before = 1)] value: u8 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_unsigned_prefix_byte_length_sources() {
        let WireType::Struct(model) = parse(
            "struct Packet<'wire> { #[wire(prefix = Varint)] length: u32, #[wire(bytes = length)] payload: &'wire [u8] }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(model.fields[0].kind, FieldKind::Prefix(_)));
        assert!(matches!(
            model.fields[1].kind,
            FieldKind::Bytes {
                source_index: 0,
                ..
            }
        ));
    }

    #[test]
    fn rejects_ambiguous_or_noncanonical_byte_length_sources() {
        for source in [
            "struct Packet<'wire> { #[wire(bytes = missing)] payload: &'wire [u8] }",
            "struct Packet<'wire> { #[wire(bytes = length)] payload: &'wire [u8], length: u8 }",
            "struct Packet<'wire> { length: i8, #[wire(bytes = length)] payload: &'wire [u8] }",
            "struct Packet<'wire> { #[wire(codec = Custom)] length: u8, #[wire(bytes = length)] payload: &'wire [u8] }",
            "struct Packet<'wire> { length: u8, #[wire(bytes = length)] payload: &'wire [u16] }",
            "struct Packet<'wire> { length: u8, #[wire(bytes = length)] first: &'wire [u8], #[wire(bytes = length)] second: &'wire [u8] }",
            "struct Packet { #[wire] value: u8 }",
            "struct Packet { #[wire()] value: u8 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_named_opcode_inputs_and_rejects_mixed_dispatch() {
        let WireType::Enum(model) = parse(
            "#[wire(tag = U8, opcodes = OpcodeTable, opcode_error = OpcodeError, unknown = reject)] enum Op { #[wire(opcode = Opcode::Ping)] Ping(Body), #[wire(opcode = Opcode::Halt)] Halt }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(model.opcodes.is_some());
        assert!(
            model
                .variants
                .iter()
                .all(|variant| variant.opcode.is_some())
        );

        let WireType::Struct(model) = parse(
            "#[wire(opcodes = OpcodeTable)] struct Packet { lead: u8, #[wire(opcodes)] operation: Op }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(model.opcodes.is_some());
        assert!(model.fields[1].uses_opcodes);

        for source in [
            "#[wire(tag = U8, opcodes = OpcodeTable, unknown = reject)] enum Op { #[wire(opcode = Opcode::Ping)] Ping }",
            "#[wire(tag = U8, opcode_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
            "#[wire(tag = U8, opcodes = OpcodeTable, opcode_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(opcode = Opcode::Ping)] Ping = 1 }",
            "#[wire(tag = U8, opcodes = OpcodeTable, opcode_error = OpcodeError, unknown = reject)] enum Op { Ping }",
            "#[wire(tag = U8, opcodes = OpcodeTable, opcode_error = OpcodeError, unknown = reject)] enum Op { #[wire(opcode = Opcode::Ping)] A, #[wire(opcode = Opcode::Ping)] B }",
            "struct Packet { #[wire(opcodes)] operation: Op }",
            "#[wire(opcodes = OpcodeTable)] struct Packet { #[wire(opcodes)] value: u8 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_nominal_bitfields_and_rejects_invalid_projections() {
        assert!(matches!(
            parse("#[wire(bitfield = u16, be, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: bool, #[wire(bits = 1..=3)] mode: u8 }").unwrap(),
            WireType::Bitfield(_)
        ));
        for source in [
            "#[wire(bitfield = u16, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: bool }",
            "#[wire(bitfield = u8)] struct Flags { #[wire(bit = 0)] enabled: bool }",
            "#[wire(bitfield = u8, reserved = preserve)] struct Flags { #[wire(bit = 0)] enabled: bool }",
            "#[wire(bitfield = u8, reserved = zero)] struct Flags { enabled: bool }",
            "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bit = 8)] enabled: bool }",
            "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: u8 }",
            "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bits = 0..=3)] mode: bool }",
            "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bits = 0..=4)] mode: u8, #[wire(bit = 4)] other: bool }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn rejects_invalid_enum_source_models() {
        for source in [
            "enum Op { Ping = 1 }",
            "#[wire(tag = I8, unknown = reject)] enum Op { Ping = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { Ping }",
            "#[wire(tag = U8, unknown = reject)] enum Op { Ping = 1 + 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { A = 1, B = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { A = 256 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { A(u8, u8) = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { A { value: u8 } = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(tag = 1, unknown = reject)] A = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { A(#[wire(be)] Body) = 1 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
    #[test]
    fn rejects_container_attributes_on_structs() {
        assert!(parse("#[wire(tag = U8, unknown = reject)] struct Packet { value: u8 }").is_err());
    }
}
