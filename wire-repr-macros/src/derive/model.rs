//! Derive source model.

mod preparation;
mod syntax;

pub(super) use preparation::{
    Computation, ComputationArgument, ComputationByteSelection, ComputationFieldPath,
    StructPreparation, validate_byte_fields, validate_computations, validate_positions,
};

use proc_macro2::TokenStream;
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, GenericParam, Generics, Ident, Lifetime, Lit, Path,
    Type, Visibility,
};

use syntax::{
    ComputationArgument as ComputationArgumentSyntax, ComputationBytes, ComputationSyntax,
    WireAttribute, parse_enum_attributes, parse_field_wire_attributes, parse_struct_attributes,
    parse_variant_attributes, reject_wire_attributes,
};

pub(super) enum WireType {
    Struct(Box<WireStruct>),
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
    pub(super) operation_input: Option<OperationInput>,
    pub(super) validators: Vec<Path>,
    pub(super) validation_error: Option<Type>,
    pub(super) fields: Vec<Field>,
    pub(super) preparation: StructPreparation,
}

pub(super) struct WireEnum {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) wire_lifetime: Option<Lifetime>,
    pub(super) tag: EnumTag,
    pub(super) unknown: UnknownPolicy,
    pub(super) operation_input: Option<OperationInput>,
    pub(super) variants: Vec<Variant>,
}

pub(super) struct Field {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) kind: FieldKind,
    pub(super) position: Option<FieldPosition>,
    pub(super) padding_before: usize,
    pub(super) alignment_before: Option<usize>,
    pub(super) operation_input: Option<Ident>,
    pub(super) validators: Vec<Path>,
    pub(super) computation: Option<Computation>,
}

pub(super) enum FieldPosition {
    Static(usize),
    Source(Ident),
}

pub(super) enum FieldKind {
    Fixed(Codec),
    Nested,
    Prefix(Path),
    Bytes { source: Ident },
    Rest,
}

pub(super) struct Variant {
    pub(super) name: Ident,
    pub(super) selector: VariantSelector,
    pub(super) operation_selector: Option<Path>,
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
    Operation(String),
}

pub(super) struct OperationInput {
    pub(super) name: Ident,
    pub(super) ty: Path,
    pub(super) error: Option<Path>,
}

pub(super) enum Codec {
    Builtin(&'static str),
    OwnedBytes(TokenStream),
    Custom(Path),
}

pub(super) struct TagCodec {
    pub(super) codec: String,
    pub(super) builtin: bool,
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
            if !attributes.validators.is_empty() || attributes.validation_error.is_some() {
                return Err(syn::Error::new_spanned(
                    &name,
                    "validators are not supported on bitfields",
                ));
            }
            return parse_bitfield(generics, vis, name, fields, storage);
        }
        let operation_input = attributes.operation_input;
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
        let has_explicit_validators = !attributes.validators.is_empty()
            || fields.iter().any(|field| !field.validators.is_empty());
        let has_nested_validation = fields
            .iter()
            .any(|field| matches!(field.kind, FieldKind::Nested));
        match (
            has_explicit_validators,
            has_nested_validation,
            attributes.validation_error.is_some(),
        ) {
            (true, _, false) => {
                return Err(syn::Error::new_spanned(
                    &name,
                    "validators require exactly one `error = ErrorType`",
                ));
            }
            (false, false, true) => {
                return Err(syn::Error::new_spanned(
                    attributes.validation_error.as_ref().expect("checked"),
                    "`error = ErrorType` requires a validator or nested validated field",
                ));
            }
            _ => {}
        }
        let controlled_by = validate_byte_fields(&fields, wire_lifetime.as_ref())?;
        validate_operation_fields(&fields, operation_input.as_ref())?;
        let computation_order = validate_computations(&mut fields)?;
        let position_sources = validate_positions(&mut fields, &controlled_by)?;
        let has_builder = controlled_by.iter().any(Option::is_some)
            || fields.iter().any(|field| field.computation.is_some());
        if has_builder {
            let reserved = operation_input
                .as_ref()
                .map(|input| input.name.to_string())
                .into_iter()
                .chain(["prepare".to_owned(), "build_into".to_owned()]);
            if let Some(field) = fields.iter().enumerate().find_map(|(index, field)| {
                (field.computation.is_none()
                    && controlled_by[index].is_none()
                    && reserved.clone().any(|method| field.name == method))
                .then_some(field)
            }) {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "derived builders reserve operation-input, `prepare`, and `build_into` method names",
                ));
            }
        }
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

        Ok(Self::Struct(Box::new(WireStruct {
            vis,
            name,
            wire_lifetime,
            operation_input,
            validators: attributes.validators,
            validation_error: attributes.validation_error,
            fields,
            preparation: StructPreparation {
                computation_order,
                controlled_by,
                position_sources,
            },
        })))
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

        let operation_input = match (attributes.operation_input, attributes.operation_error) {
            (Some(mut input), Some((error_name, error))) if error_name == input.name => {
                input.error = Some(error);
                Some(input)
            }
            (Some(input), None) => {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!(
                        "dynamic operation enums require `{}_error = ErrorType`",
                        input.name
                    ),
                ));
            }
            (Some(input), Some((error_name, error))) => {
                return Err(syn::Error::new_spanned(
                    error,
                    format!(
                        "`{}_error` does not match declared operation input `{}`",
                        error_name, input.name
                    ),
                ));
            }
            (None, Some((error_name, error))) => {
                return Err(syn::Error::new_spanned(
                    error,
                    format!("`{}_error` requires a matching operation input", error_name),
                ));
            }
            (None, None) => None,
        };
        if matches!(tag, EnumTag::Bytes { .. }) && operation_input.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "dynamic operation enums cannot use fixed byte tag selectors",
            ));
        }
        if matches!(unknown, UnknownPolicy::Preserve) && operation_input.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "dynamic operation enums currently require `unknown = reject`",
            ));
        }

        let mut selectors = BTreeSet::new();
        let variants = variants
            .into_iter()
            .map(|variant| Variant::parse(variant, &tag, operation_input.as_ref(), &mut selectors))
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
            operation_input,
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
        let value_ty = attributes
            .computation
            .is_some()
            .then(|| syn::parse2(field.ty.to_token_stream()))
            .transpose()?;
        let representation_ty = &field.ty;
        let kind = match attributes.representation {
            WireAttribute::Custom(path) => FieldKind::Fixed(Codec::Custom(path)),
            WireAttribute::Endian(big_endian) => FieldKind::Fixed(
                builtin_codec(representation_ty, Some(big_endian)).ok_or_else(|| {
                    syn::Error::new_spanned(
                        representation_ty,
                        "wire endian attributes require a multi-byte integer field",
                    )
                })?,
            ),
            WireAttribute::Prefix(path) => FieldKind::Prefix(path),
            WireAttribute::Bytes(source) => FieldKind::Bytes { source },
            WireAttribute::Rest => FieldKind::Rest,
            WireAttribute::None if attributes.computation.is_some() => {
                match builtin_codec(representation_ty, None) {
                    Some(codec) => FieldKind::Fixed(codec),
                    None if is_multibyte_integer(representation_ty) => {
                        return Err(syn::Error::new_spanned(
                            representation_ty,
                            "multi-byte integer fields require #[wire(be)] or #[wire(le)]",
                        ));
                    }
                    None => FieldKind::Nested,
                }
            }
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
        let computation = match attributes.computation {
            Some(ComputationSyntax::Callback {
                path: callback,
                arguments,
            }) => {
                let value_ty = value_ty.expect("computed fields have a semantic type");
                if attributes.position.is_some()
                    || attributes.padding_before.is_some()
                    || attributes.alignment_before.is_some()
                    || attributes.operation_input.is_some()
                {
                    return Err(syn::Error::new_spanned(
                        &name,
                        "computed fields cannot declare geometry or operation-input attributes",
                    ));
                }
                if !matches!(kind, FieldKind::Fixed(_)) {
                    return Err(syn::Error::new_spanned(
                        &value_ty,
                        "computed fields require a fixed representation; select an explicit fixed codec for nominal semantic types",
                    ));
                }
                let arguments = arguments
                    .into_iter()
                    .map(|argument| match argument {
                        ComputationArgumentSyntax::Semantic(name) => {
                            ComputationArgument::Semantic {
                                name,
                                index: usize::MAX,
                            }
                        }
                        ComputationArgumentSyntax::Bytes(bytes) => {
                            ComputationArgument::Bytes(match bytes {
                                ComputationBytes::Include(paths) => {
                                    ComputationByteSelection::Include(
                                        paths
                                            .into_iter()
                                            .map(|path| ComputationFieldPath {
                                                top_level: path.top_level,
                                                top_level_index: usize::MAX,
                                                nested: path.nested,
                                            })
                                            .collect(),
                                    )
                                }
                                ComputationBytes::Exclude(paths) => {
                                    ComputationByteSelection::Exclude(
                                        paths
                                            .into_iter()
                                            .map(|path| ComputationFieldPath {
                                                top_level: path.top_level,
                                                top_level_index: usize::MAX,
                                                nested: path.nested,
                                            })
                                            .collect(),
                                    )
                                }
                            })
                        }
                    })
                    .collect::<Vec<_>>();
                Some(Computation {
                    value_ty,
                    callback,
                    requires_geometry: false,
                    arguments,
                })
            }
            None => None,
        };
        Ok(Self {
            name,
            ty: field.ty,
            kind,
            position: attributes.position,
            padding_before: attributes.padding_before.unwrap_or(0),
            alignment_before: attributes.alignment_before,
            operation_input: attributes.operation_input,
            validators: attributes.validators,
            computation,
        })
    }
}

impl Variant {
    fn parse(
        variant: syn::Variant,
        tag: &EnumTag,
        operation_input: Option<&OperationInput>,
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

        let (selector, operation_selector) = if let Some(operation_input) = operation_input {
            if discriminant.is_some() {
                return Err(syn::Error::new_spanned(
                    &name,
                    "dynamic operation variants use a named operation selector, not Rust discriminants",
                ));
            }
            if attributes.byte_tag.is_some() || attributes.unknown {
                return Err(syn::Error::new_spanned(
                    &name,
                    "dynamic operation variants cannot use byte tag selectors or #[wire(unknown)]",
                ));
            }
            let (binding, selector) = attributes.operation_selector.ok_or_else(|| {
                syn::Error::new_spanned(
                    &name,
                    format!(
                        "dynamic operation variants require #[wire({} = Selector::Variant)]",
                        operation_input.name
                    ),
                )
            })?;
            if binding != operation_input.name {
                return Err(syn::Error::new_spanned(
                    &binding,
                    format!(
                        "`{}` is not the declared operation input `{}`",
                        binding, operation_input.name
                    ),
                ));
            }
            let key = selector.to_token_stream().to_string();
            if !selectors.insert(SelectorKey::Operation(key)) {
                return Err(syn::Error::new_spanned(
                    &selector,
                    "operation selector duplicates an earlier variant",
                ));
            }
            (VariantSelector::Dynamic, Some(selector))
        } else if attributes.unknown {
            if discriminant.is_some()
                || attributes.byte_tag.is_some()
                || attributes.operation_selector.is_some()
            {
                return Err(syn::Error::new_spanned(
                    &name,
                    "#[wire(unknown)] cannot also declare a discriminant, tag, or operation selector",
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
            if let Some((binding, _)) = attributes.operation_selector {
                return Err(syn::Error::new_spanned(
                    binding,
                    "operation selectors require an enum operation input",
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
            operation_selector,
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

fn validate_operation_fields(
    fields: &[Field],
    operation_input: Option<&OperationInput>,
) -> syn::Result<()> {
    for field in fields {
        let Some(binding) = &field.operation_input else {
            continue;
        };
        let Some(input) = operation_input else {
            return Err(syn::Error::new_spanned(
                binding,
                format!("#[wire({binding})] requires a matching operation input declaration"),
            ));
        };
        if binding != &input.name {
            return Err(syn::Error::new_spanned(
                binding,
                format!(
                    "`{binding}` is not the declared operation input `{}`",
                    input.name
                ),
            ));
        }
        if !matches!(field.kind, FieldKind::Nested) {
            return Err(syn::Error::new_spanned(
                &field.name,
                "operation input bindings are valid only on nested wire fields",
            ));
        }
    }
    Ok(())
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
    use syn::Type;

    use super::{
        Codec, ComputationArgument, ComputationByteSelection, EnumTag, FieldKind, FieldPosition,
        UnknownPolicy, VariantSelector, WireType,
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
            "#[wire(tag = [u8; 2], table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ok)] Ok }",
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
        assert!(matches!(model.fields[1].kind, FieldKind::Bytes { .. }));
        assert_eq!(model.preparation.controlled_by, vec![Some(1), None, None]);
        assert_eq!(
            model.preparation.position_sources,
            vec![false, false, false]
        );
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
        assert_eq!(model.preparation.controlled_by, vec![None, Some(2), None]);
        assert_eq!(model.preparation.position_sources, vec![true, false, false]);

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
    fn schedules_semantic_computed_position_sources_before_geometry() {
        let WireType::Struct(model) = parse(
            "struct Packet { lead: u8, #[wire(computed = offset(lead))] offset: u8, #[wire(at = offset)] payload: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.fields[2].position,
            Some(FieldPosition::Source(ref source)) if source == "offset"
        ));
        assert_eq!(model.preparation.computation_order, vec![1]);
        assert!(
            !model.fields[1]
                .computation
                .as_ref()
                .expect("computed offset")
                .requires_geometry
        );

        let WireType::Struct(model) = parse(
            "struct Packet { #[wire(computed = offset())] offset: u8, #[wire(at = offset)] payload: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(
            !model.fields[0]
                .computation
                .as_ref()
                .expect("computed offset")
                .requires_geometry
        );
    }

    #[test]
    fn rejects_computed_position_sources_that_require_physical_geometry() {
        let accepted = "struct Packet { #[wire(computed = offset(include(marker)))] offset: u8, marker: u8, #[wire(at = offset)] payload: u8 }";
        assert!(parse(accepted).is_ok());

        let source = "struct Packet { #[wire(computed = offset(exclude(marker)))] offset: u8, marker: u8, #[wire(at = offset)] payload: u8 }";
        let Err(error) = parse(source) else {
            panic!("accepted computed geometry cycle: {source}");
        };
        assert!(error.to_string().contains(
            "a computed position source cannot depend on physical geometry that its position controls"
        ));

        let transitive = "struct Packet { #[wire(computed = checksum(exclude(offset)))] checksum: u8, #[wire(computed = offset(include(checksum)))] offset: u8, #[wire(at = offset)] payload: u8 }";
        let Err(error) = parse(transitive) else {
            panic!("accepted transitive computed geometry cycle: {transitive}");
        };
        assert!(error.to_string().contains(
            "a computed position source cannot depend on physical geometry that its position controls"
        ));
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
        assert!(matches!(model.fields[1].kind, FieldKind::Bytes { .. }));
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
    fn rejects_computed_byte_length_sources() {
        let source = "struct Packet<'wire> { #[wire(computed = count())] length: u8, #[wire(bytes = length)] payload: &'wire [u8] }";
        let Err(error) = parse(source) else {
            panic!("accepted conflicting framing source: {source}");
        };
        assert!(error.to_string().contains(
            "a computed field cannot be a byte length source because `bytes` owns framing geometry"
        ));
    }

    #[test]
    fn accepts_named_operation_inputs_and_rejects_mixed_dispatch() {
        let WireType::Enum(model) = parse(
            "#[wire(tag = U8, opcodes = Opcodes, opcodes_error = OpcodeMapError, unknown = reject)] enum MappedOperation { #[wire(opcodes = Opcode::Ping)] Ping(Ping), #[wire(opcodes = Opcode::Halt)] Halt }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(
            model
                .operation_input
                .as_ref()
                .is_some_and(|input| input.name == "opcodes")
        );

        let WireType::Enum(model) = parse(
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping(Body), #[wire(table = Opcode::Halt)] Halt }",
        )
        .unwrap()
        else {
            panic!()
        };
        let input = model.operation_input.as_ref().expect("operation input");
        assert!(input.name == "table");
        assert!(input.error.is_some());
        assert!(
            model
                .variants
                .iter()
                .all(|variant| variant.operation_selector.is_some())
        );

        let WireType::Struct(model) = parse(
            "#[wire(table = OpcodeTable)] struct Packet { lead: u8, #[wire(table)] first: Op, #[wire(table)] second: Op }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(model.operation_input.is_some());
        assert!(model.fields[1].operation_input.is_some());
        assert!(model.fields[2].operation_input.is_some());

        for source in [
            "#[wire(tag = U8, table = OpcodeTable, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
            "#[wire(tag = U8, table_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
            "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping = 1 }",
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { Ping }",
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] A, #[wire(table = Opcode::Ping)] B }",
            "struct Packet { #[wire(table)] operation: Op }",
            "#[wire(table = OpcodeTable)] struct Packet { #[wire(table)] value: u8 }",
            "#[wire(table = OpcodeTable, table_error = OpcodeError)] struct Packet { #[wire(table)] operation: Op }",
            "#[wire(table = OpcodeTable)] struct Packet { #[wire(other)] operation: Op }",
            "#[wire(tag = U8, table = OpcodeTable, other_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, other = OtherTable, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
            "#[wire(tag = U8, view = OpcodeTable, unknown = reject)] enum Op { #[wire(view = Opcode::Ping)] Ping }",
            "#[wire(tag = U8, cursor = OpcodeTable, unknown = reject)] enum Op { #[wire(cursor = Opcode::Ping)] Ping }",
            "#[wire(builder = OpcodeTable)] struct Packet { #[wire(builder)] operation: Op }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_ordered_struct_validators_with_one_human_error_type() {
        let WireType::Struct(model) = parse(
            "#[wire(error = PacketError, validate = validate_model_first, validate = validate_model_second)] struct Packet { #[wire(validate = validate_field_first, validate = validate_field_second)] value: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };

        assert_eq!(model.validators.len(), 2);
        assert!(model.validators[0].is_ident("validate_model_first"));
        assert!(model.validators[1].is_ident("validate_model_second"));
        assert!(model.validation_error.is_some());
        assert_eq!(model.fields[0].validators.len(), 2);
        assert!(model.fields[0].validators[0].is_ident("validate_field_first"));
        assert!(model.fields[0].validators[1].is_ident("validate_field_second"));

        let WireType::Struct(model) =
            parse("#[wire(error = ParentError)] struct Parent { child: Child }").unwrap()
        else {
            panic!()
        };
        assert!(model.validation_error.is_some());
    }

    #[test]
    fn rejects_incomplete_or_unsupported_struct_validator_contracts() {
        for source in [
            "#[wire(validate = validate_model)] struct Packet { value: u8 }",
            "struct Packet { #[wire(validate = validate_field)] value: u8 }",
            "#[wire(error = PacketError)] struct Packet { value: u8 }",
            "#[wire(error = FirstError, error = SecondError, validate = validate_model)] struct Packet { value: u8 }",
            "#[wire(bitfield = u8, reserved = zero, error = FlagsError, validate = validate_flags)] struct Flags { #[wire(bit = 0)] enabled: bool }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }

        for source in [
            "#[wire(table = OpcodeTable, error = PacketError, validate = validate_model)] struct Packet { #[wire(table)] operation: Op }",
            "#[wire(table = OpcodeTable, error = PacketError)] struct Packet { #[wire(table, validate = validate_field)] operation: Op }",
        ] {
            if let Err(error) = parse(source) {
                panic!("{source}: {error}");
            }
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
    #[test]
    fn accepts_computed_semantic_fields() {
        let WireType::Struct(model) = parse("struct Packet<'wire> { #[wire(computed = wire_repr::computation::len(payload))] length: u8, #[wire(rest)] payload: &'wire [u8] }").unwrap() else { panic!() };
        assert!(matches!(
            model.fields[0]
                .computation
                .as_ref()
                .map(|computation| &computation.value_ty),
            Some(Type::Path(path)) if path.path.is_ident("u8")
        ));
        assert!(matches!(
            model.fields[0].kind,
            FieldKind::Fixed(Codec::Builtin("U8"))
        ));
    }

    #[test]
    fn accepts_callback_computations_and_rejects_invalid_byte_selections() {
        let WireType::Struct(model) = parse(
            "struct Packet { #[wire(computed = checksum(include(header, payload.inner)))] checksum: u8, header: u8, payload: Payload }",
        )
        .unwrap()
        else {
            panic!()
        };
        let computation = model.fields[0].computation.as_ref().expect("computation");
        assert!(computation.callback.is_ident("checksum"));
        let ComputationArgument::Bytes(ComputationByteSelection::Include(paths)) =
            &computation.arguments[0]
        else {
            panic!("include selection")
        };
        assert_eq!(paths.len(), 2);
        assert!(paths[0].top_level == "header" && paths[0].nested.is_empty());
        assert!(paths[1].top_level == "payload" && paths[1].nested[0] == "inner");

        let WireType::Struct(model) = parse(
            "struct Packet { head: u8, #[wire(computed = crate::checksum(exclude(self)))] checksum: u8, payload: Payload }",
        )
        .unwrap()
        else {
            panic!()
        };
        assert!(matches!(
            model.fields[1]
                .computation
                .as_ref()
                .map(|computation| &computation.arguments[0]),
            Some(ComputationArgument::Bytes(ComputationByteSelection::Exclude(paths)))
                if paths[0].top_level_index == 1
        ));

        for source in [
            "struct Packet { #[wire(computed = checksum)] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(bytes(include(payload)))] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(computed = wire_repr::computation::len(payload), bytes(include(payload)))] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(computed = checksum(include(missing)))] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(computed = checksum(include(checksum)))] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(computed = checksum(checksum))] checksum: u8, payload: Payload }",
            "struct Packet { #[wire(computed = first(include(second)))] first: u8, #[wire(computed = second(include(first)))] second: u8 }",
            "struct Packet { #[wire(computed = first(exclude(self)))] first: u8, #[wire(computed = second(exclude(self)))] second: u8 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
        assert!(
            parse(
                "struct Packet { #[wire(computed = checksum())] checksum: u8, payload: Payload }"
            )
            .is_ok()
        );
        assert!(parse(
            "struct Packet { #[wire(computed = checksum(include()))] checksum: u8, payload: Payload }"
        )
        .is_ok());
        assert!(parse(
            "struct Packet { #[wire(computed = checksum(exclude()))] checksum: u8, payload: Payload }"
        )
        .is_ok());
    }
}
