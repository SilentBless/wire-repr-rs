//! Enum semantic parsing.

use std::collections::BTreeSet;

use quote::ToTokens;
use syn::{Attribute, Expr, Fields, Generics, Ident, Lit, Type, Visibility};

use super::codec::{fixed_array_len, is_plain_u8, parse_tag_codec};
use super::syntax::{parse_enum_attributes, parse_variant_attributes, reject_wire_attributes};
use super::{
    EnumTag, OperationInput, UnknownPolicy, Variant, VariantSelector, WireEnum, WireType,
    parse_wire_lifetime,
};

#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum SelectorKey {
    Integer(u128),
    Bytes(Vec<u8>),
    Unknown,
    Operation(String),
}

pub(super) fn parse(
    attributes: Vec<Attribute>,
    generics: Generics,
    vis: Visibility,
    name: Ident,
    variants: syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>,
) -> syn::Result<WireType> {
    let wire_lifetime = parse_wire_lifetime(&generics, "Wire enums")?;
    let attributes = parse_enum_attributes(&attributes)?;
    let tag = attributes
        .tag
        .ok_or_else(|| syn::Error::new_spanned(&name, "Wire enums require #[wire(tag = CODEC)]"))?;
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

    Ok(WireType::Enum(WireEnum {
        vis,
        name,
        wire_lifetime,
        tag,
        unknown,
        operation_input,
        variants,
    }))
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

pub(in crate::derive) fn parse_enum_tag(representation: Type) -> syn::Result<EnumTag> {
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
