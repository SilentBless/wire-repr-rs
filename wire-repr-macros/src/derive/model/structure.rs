//! Struct and field semantic parsing.

use quote::ToTokens;
use syn::{Attribute, Fields, Generics, Ident, Visibility};

use super::codec::{builtin_codec, is_borrowed_byte_slice, is_multibyte_integer};
use super::syntax::{
    ComputationArgument as ComputationArgumentSyntax, ComputationBytes, ComputationSyntax,
    WireAttribute, parse_field_wire_attributes, parse_struct_attributes,
};
use super::{
    Codec, Computation, ComputationArgument, ComputationByteSelection, ComputationFieldPath, Field,
    FieldKind, OperationInput, StructPreparation, WireStruct, WireType, bitfield,
    parse_wire_lifetime, validate_byte_fields, validate_computations, validate_positions,
};

pub(super) fn parse(
    attributes: Vec<Attribute>,
    generics: Generics,
    vis: Visibility,
    name: Ident,
    fields: Fields,
) -> syn::Result<WireType> {
    let attributes = parse_struct_attributes(&attributes)?;
    if let Some(storage) = attributes.bitfield {
        if !attributes.validators.is_empty() || attributes.validation_error.is_some() {
            return Err(syn::Error::new_spanned(
                &name,
                "validators are not supported on bitfields",
            ));
        }
        return bitfield::parse(generics, vis, name, fields, storage);
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

    Ok(WireType::Struct(Box::new(WireStruct {
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
