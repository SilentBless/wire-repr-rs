//! Source attribute syntax parsing for `Wire` derives.

use syn::{Attribute, Expr, Ident, Lit, Member, Path, Type};

use super::{EnumTag, FieldPosition, OperationInput, TagCodec, UnknownPolicy};

pub(super) struct VariantAttributes {
    pub(super) operation_selector: Option<(Ident, Path)>,
    pub(super) byte_tag: Option<syn::LitByteStr>,
    pub(super) unknown: bool,
}

pub(super) fn parse_variant_attributes(attributes: &[Attribute]) -> syn::Result<VariantAttributes> {
    let mut result = VariantAttributes {
        operation_selector: None,
        byte_tag: None,
        unknown: false,
    };
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
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
                let Some(binding) = meta.path.get_ident() else {
                    return Err(meta.error("operation selector names must be identifiers"));
                };
                if is_reserved_wire_option(binding) {
                    return Err(meta.error("reserved Wire options cannot select operations"));
                }
                if result.operation_selector.is_some() {
                    return Err(meta.error("duplicate operation selector"));
                }
                result.operation_selector = Some((binding.clone(), meta.value()?.parse()?));
                Ok(())
            }
        })?;
    }
    Ok(result)
}

pub(super) enum WireAttribute {
    None,
    Endian(bool),
    Custom(Path),
    Prefix(Path),
    Bytes(Ident),
    Rest,
}

pub(super) enum ComputationSyntax {
    Length(Ident),
    Callback { path: Path, bytes: ComputationBytes },
}

pub(super) enum ComputationBytes {
    Include(Vec<ComputationFieldPath>),
    ExcludeSelf,
}

pub(super) struct ComputationFieldPath {
    pub(super) top_level: Ident,
    pub(super) nested: Vec<Ident>,
}

fn parse_computation(expression: Expr) -> syn::Result<ComputationSyntax> {
    let Expr::Call(call) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed fields require `computed = len(field)`, `computed = callback(include(field, ...))`, or `computed = callback(exclude(self))`",
        ));
    };
    let Expr::Path(function) = call.func.as_ref() else {
        return Err(syn::Error::new_spanned(
            &call.func,
            "computed callbacks must be function paths",
        ));
    };
    if function.path.is_ident("len") {
        let Some(Expr::Path(target)) = call.args.first() else {
            return Err(syn::Error::new_spanned(call, "`len` requires one field"));
        };
        let Some(target) = target.path.get_ident() else {
            return Err(syn::Error::new_spanned(target, "`len` requires one field"));
        };
        if call.args.len() != 1 {
            return Err(syn::Error::new_spanned(call, "`len` requires one field"));
        }
        return Ok(ComputationSyntax::Length(target.clone()));
    }
    if function.qself.is_some() || call.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            call,
            "computed callbacks require one `include(...)` or `exclude(self)` selection",
        ));
    }
    let bytes = parse_computation_bytes(call.args.first().expect("one argument"))?;
    let Expr::Path(function) = *call.func else {
        unreachable!("checked above")
    };
    Ok(ComputationSyntax::Callback {
        path: function.path,
        bytes,
    })
}

fn parse_computation_bytes(expression: &Expr) -> syn::Result<ComputationBytes> {
    let Expr::Call(call) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "expected `include(field, ...)` or `exclude(self)`",
        ));
    };
    let Expr::Path(mode) = call.func.as_ref() else {
        return Err(syn::Error::new_spanned(
            &call.func,
            "expected `include(field, ...)` or `exclude(self)`",
        ));
    };
    if mode.path.is_ident("exclude") {
        if call.args.len() == 1
            && matches!(call.args.first(), Some(Expr::Path(path)) if path.path.is_ident("self"))
        {
            return Ok(ComputationBytes::ExcludeSelf);
        }
        return Err(syn::Error::new_spanned(
            call,
            "`exclude(...)` accepts only `self`",
        ));
    }
    if !mode.path.is_ident("include") {
        return Err(syn::Error::new_spanned(
            mode,
            "expected `include(field, ...)` or `exclude(self)`",
        ));
    }
    if call.args.is_empty() {
        return Err(syn::Error::new_spanned(
            call,
            "`include(...)` requires at least one field path",
        ));
    }
    call.args
        .iter()
        .map(parse_computation_field_path)
        .collect::<syn::Result<Vec<_>>>()
        .map(ComputationBytes::Include)
}

fn parse_computation_field_path(expression: &Expr) -> syn::Result<ComputationFieldPath> {
    match expression {
        Expr::Path(path) if path.qself.is_none() => {
            let Some(top_level) = path.path.get_ident() else {
                return Err(syn::Error::new_spanned(
                    path,
                    "computed byte selections require field paths",
                ));
            };
            Ok(ComputationFieldPath {
                top_level: top_level.clone(),
                nested: Vec::new(),
            })
        }
        Expr::Field(field) => {
            let mut path = parse_computation_field_path(&field.base)?;
            let Member::Named(member) = &field.member else {
                return Err(syn::Error::new_spanned(
                    &field.member,
                    "computed byte selections require named field paths",
                ));
            };
            path.nested.push(member.clone());
            Ok(path)
        }
        _ => Err(syn::Error::new_spanned(
            expression,
            "computed byte selections require field paths",
        )),
    }
}

pub(super) struct FieldWireAttributes {
    pub(super) representation: WireAttribute,
    pub(super) computation: Option<ComputationSyntax>,
    pub(super) padding_before: Option<usize>,
    pub(super) alignment_before: Option<usize>,
    pub(super) position: Option<FieldPosition>,
    pub(super) operation_input: Option<Ident>,
    pub(super) validators: Vec<Path>,
}

pub(super) fn parse_field_wire_attributes(
    attributes: &[Attribute],
) -> syn::Result<FieldWireAttributes> {
    let mut result = FieldWireAttributes {
        representation: WireAttribute::None,
        computation: None,
        padding_before: None,
        alignment_before: None,
        position: None,
        operation_input: None,
        validators: Vec::new(),
    };
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        let mut saw_option = false;
        attribute.parse_nested_meta(|meta| {
            saw_option = true;
            if meta.path.is_ident("validate") {
                result.validators.push(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("at") {
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
            } else if meta.path.is_ident("computed") {
                if result.computation.is_some() {
                    return Err(meta.error("duplicate `computed`"));
                }
                let expression: Expr = meta.value()?.parse()?;
                result.computation = Some(parse_computation(expression)?);
                Ok(())
            } else if meta.path.is_ident("bytes") && meta.input.peek(syn::token::Paren) {
                Err(meta.error(
                    "put the byte selection inside the computation: `computed = callback(include(field, ...))` or `computed = callback(exclude(self))`",
                ))
            } else if !meta.input.peek(syn::Token![=])
                && !is_reserved_wire_option(meta.path.get_ident().expect("single field option"))
            {
                let Some(binding) = meta.path.get_ident() else {
                    return Err(meta.error("operation input bindings must be identifiers"));
                };
                if is_reserved_wire_option(binding) {
                    return Err(meta.error("reserved Wire options cannot bind operation inputs"));
                }
                if result.operation_input.is_some() {
                    return Err(meta.error("duplicate operation input binding"));
                }
                result.operation_input = Some(binding.clone());
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
                        "expected `be`, `le`, `codec = Path`, `prefix = Path`, `bytes = source_field`, `rest`, `computed = len(field)`, `validate = path`, `at = N`, `at = source_field`, `pad_before = N`, `align_before = N`, or a declared operation input binding",
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

pub(super) fn reject_wire_attributes(attributes: &[Attribute], message: &str) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| attribute.path().is_ident("wire"))
    {
        Err(syn::Error::new_spanned(attribute, message))
    } else {
        Ok(())
    }
}

fn is_reserved_wire_option(name: &Ident) -> bool {
    matches!(
        name.to_string().as_str(),
        "tag"
            | "unknown"
            | "validate"
            | "error"
            | "bitfield"
            | "be"
            | "le"
            | "reserved"
            | "at"
            | "pad_before"
            | "align_before"
            | "codec"
            | "prefix"
            | "bytes"
            | "rest"
            | "computed"
            | "bit"
            | "bits"
            | "unchecked"
            | "with_remainder"
            | "without_trailing"
            | "remaining"
            | "next"
            | "prepare"
            | "build_into"
            | "view"
            | "cursor"
            | "builder"
    )
}

pub(super) struct EnumAttributes {
    pub(super) tag: Option<EnumTag>,
    pub(super) unknown: Option<UnknownPolicy>,
    pub(super) operation_input: Option<OperationInput>,
    pub(super) operation_error: Option<(Ident, Path)>,
}

pub(super) struct StructAttributes {
    pub(super) operation_input: Option<OperationInput>,
    pub(super) validators: Vec<Path>,
    pub(super) validation_error: Option<Type>,
    pub(super) bitfield: Option<TagCodec>,
}

pub(super) fn parse_struct_attributes(attributes: &[Attribute]) -> syn::Result<StructAttributes> {
    let mut operation_input = None;
    let mut validators = Vec::new();
    let mut validation_error = None;
    let mut bitfield_type = None;
    let mut endian = None;
    let mut reserved_zero = false;
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("wire"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("validate") {
                validators.push(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("error") {
                if validation_error.is_some() {
                    return Err(meta.error("duplicate validation error type"));
                }
                validation_error = Some(meta.value()?.parse()?);
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
            let Some(name) = meta.path.get_ident() else {
                return Err(meta.error("operation input names must be identifiers"));
            };
            if is_reserved_wire_option(name) {
                return Err(meta.error("reserved Wire options cannot declare operation inputs"));
            }
            let name_text = name.to_string();
            if name_text.ends_with("_error") {
                return Err(meta.error(
                    "operation input error types belong on the selected enum, not a forwarding struct",
                ));
            }
            if operation_input.is_some() {
                return Err(meta.error("duplicate operation input declaration"));
            }
            operation_input = Some(OperationInput {
                name: name.clone(),
                ty: meta.value()?.parse()?,
                error: None,
            });
            Ok(())
        })?;
    }

    let bitfield = match bitfield_type {
        Some(ty) => {
            if operation_input.is_some() {
                return Err(syn::Error::new_spanned(
                    ty,
                    "bitfields cannot declare operation inputs",
                ));
            }
            if !reserved_zero {
                return Err(syn::Error::new_spanned(
                    ty,
                    "bitfields require an explicit reserved-bit policy; add `reserved = zero`",
                ));
            }
            Some(super::parse_bitfield_storage(&ty, endian)?)
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

    Ok(StructAttributes {
        operation_input,
        validators,
        validation_error,
        bitfield,
    })
}

pub(super) fn parse_enum_attributes(attributes: &[Attribute]) -> syn::Result<EnumAttributes> {
    let mut result = EnumAttributes {
        tag: None,
        unknown: None,
        operation_input: None,
        operation_error: None,
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
                result.tag = Some(super::parse_enum_tag(representation)?);
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
            let Some(name) = meta.path.get_ident() else {
                return Err(meta.error("operation input names must be identifiers"));
            };
            if is_reserved_wire_option(name) {
                return Err(meta.error("reserved Wire options cannot declare operation inputs"));
            }
            let name_text = name.to_string();
            if let Some(base) = name_text.strip_suffix("_error") {
                if result.operation_error.is_some() {
                    return Err(meta.error("duplicate operation input error type"));
                }
                result.operation_error = Some((
                    Ident::new(base, name.span()),
                    meta.value()?.parse()?,
                ));
            } else {
                if result.operation_input.is_some() {
                    return Err(meta.error("duplicate operation input declaration"));
                }
                result.operation_input = Some(OperationInput {
                    name: name.clone(),
                    ty: meta.value()?.parse()?,
                    error: None,
                });
            }
            Ok(())
        })?;
    }
    Ok(result)
}
