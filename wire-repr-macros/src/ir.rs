//! Validated, renderer-ready layout model.

use std::collections::HashMap;

use proc_macro2::Span;
use syn::{Attribute, Error, Ident, Path, Result, Visibility};

use crate::syntax;

/// Fully validated invocation in source declaration order.
pub(crate) struct Invocation {
    pub(crate) layouts: Vec<Layout>,
}

/// A renderer-ready layout with unambiguous placement semantics.
pub(crate) enum Layout {
    /// A sequential layout.
    Sequential(SequentialLayout),
    /// A fixed absolute-offset layout.
    Absolute(AbsoluteLayout),
}

/// Fields and generated names shared by both layout modes.
pub(crate) struct LayoutData {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) view_name: Ident,
    pub(crate) error_name: Ident,
    /// Mutable-view name reserved for sequential rendering.
    pub(crate) view_mut_name: Ident,
    /// Builder name reserved for sequential rendering.
    pub(crate) builder_name: Ident,
    /// Mutation-error name reserved for sequential rendering.
    pub(crate) mutation_error_name: Ident,
    /// Builder write-error name reserved for sequential rendering.
    pub(crate) write_error_name: Ident,
    /// Fields in source declaration order.
    pub(crate) fields: Vec<Field>,
}

/// A sequential layout.
pub(crate) struct SequentialLayout {
    pub(crate) data: LayoutData,
    /// Renderer-ready entries sorted by one-based physical position.
    pub(crate) physical_order: Vec<PhysicalItem>,
    /// Whether at least one field has a runtime-discovered extent.
    pub(crate) has_dynamic: bool,
}

/// A validated sequential physical entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalItem {
    Field { index: usize, position: usize },
    Padding { position: usize, length: usize },
    Alignment { position: usize, boundary: usize },
}

impl PhysicalItem {
    pub(crate) const fn position(&self) -> usize {
        match self {
            Self::Field { position, .. }
            | Self::Padding { position, .. }
            | Self::Alignment { position, .. } => *position,
        }
    }
}

/// A fixed absolute-offset layout.
pub(crate) struct AbsoluteLayout {
    pub(crate) data: LayoutData,
    /// Indices into fields, sorted by zero-based byte offset.
    pub(crate) offset_order: Vec<usize>,
}

/// A validated field. Its placement is interpreted only by its enclosing layout mode.
pub(crate) struct Field {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) name: Ident,
    pub(crate) kind: FieldKind,
    pub(crate) declaration_index: usize,
    pub(crate) placement: usize,
    pub(crate) placement_span: Span,
    pub(crate) error_variant: Ident,
    /// Fixed-sequential setter name, normalized with the source identifier.
    pub(crate) setter_name: Ident,
    /// Exact-representation getter name used when this is a prefix field.
    pub(crate) encoded_getter: Ident,
    /// Stored end-boundary name used when this is a prefix field.
    pub(crate) boundary: Ident,
    /// Whether any region is framed by this field's decoded value.
    pub(crate) is_region_length_source: bool,
    pub(crate) projections: Vec<Projection>,
}

impl Field {
    pub(crate) fn codec(&self) -> Option<&Codec> {
        match &self.kind {
            FieldKind::Codec(codec) => Some(codec),
            FieldKind::Region { .. } => None,
        }
    }

    pub(crate) fn is_prefix(&self) -> bool {
        self.codec().is_some_and(Codec::is_prefix)
    }

    pub(crate) const fn is_region(&self) -> bool {
        matches!(self.kind, FieldKind::Region { .. })
    }
}

/// Fully resolved semantic kind of a named field.
pub(crate) enum FieldKind {
    Codec(Codec),
    Region {
        length_source: usize,
        length_source_span: Span,
    },
}

/// Renderer-ready immutable bit projection.
pub(crate) struct Projection {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) name: Ident,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: ProjectionKind,
    pub(crate) value_type: UnsignedType,
}

/// Validated projection shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionKind {
    Bit,
    Bits,
}

/// Unsigned decoded type for a projection storage codec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnsignedType {
    U8,
    U16,
    U32,
    U64,
    U128,
}

/// A renderer-facing codec category.
pub(crate) enum Codec {
    Builtin(Builtin),
    Custom(Path),
    Bytes(usize),
    Prefix(Path),
}

impl Codec {
    pub(crate) fn is_prefix(&self) -> bool {
        matches!(self, Self::Prefix(_))
    }
}

/// Builtin fixed codecs accepted by the DSL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Builtin {
    U8,
    I8,
    BeU16,
    LeU16,
    BeI16,
    LeI16,
    BeU24,
    LeU24,
    BeU32,
    LeU32,
    BeI32,
    LeI32,
    BeU64,
    LeU64,
    BeI64,
    LeI64,
    BeU128,
    LeU128,
    BeI128,
    LeI128,
}

/// Normalizes parsed syntax and rejects semantic violations before rendering.
pub(crate) fn normalize(parsed: syntax::Invocation) -> Result<Invocation> {
    let mut errors = None;
    if parsed.layouts.is_empty() {
        push(
            &mut errors,
            Error::new(Span::call_site(), "wire_repr requires at least one layout"),
        );
    }
    let mut stems = HashMap::new();
    let mut generated_layout_names = HashMap::new();
    let mut layouts = Vec::new();
    for source in parsed.layouts {
        if stems
            .insert(source.name.to_string(), source.name.span())
            .is_some()
        {
            push(
                &mut errors,
                Error::new(source.name.span(), "duplicate layout stem"),
            );
        }
        if let Some(layout) = normalize_layout(source, &mut generated_layout_names, &mut errors) {
            layouts.push(layout);
        }
    }
    match errors {
        Some(error) => Err(error),
        None => Ok(Invocation { layouts }),
    }
}

fn normalize_layout(
    source: syntax::Layout,
    generated_layout_names: &mut HashMap<String, Span>,
    errors: &mut Option<Error>,
) -> Option<Layout> {
    if source.fields.is_empty() {
        push(
            errors,
            Error::new(source.name.span(), "layout must contain at least one field"),
        );
    }
    let stem_text = source.name.to_string();
    if stem_text.starts_with("r#") {
        push(
            errors,
            Error::new(
                source.name.span(),
                "raw layout identifiers cannot form generated names",
            ),
        );
    }
    let view_name = generated_ident(
        &(stem_text.clone() + "View"),
        source.name.span(),
        "layout view",
        errors,
    )?;
    let error_name = generated_ident(
        &(stem_text.clone() + "Error"),
        source.name.span(),
        "layout error",
        errors,
    )?;
    let view_mut_name = generated_ident(
        &(stem_text.clone() + "ViewMut"),
        source.name.span(),
        "mutable layout view",
        errors,
    )?;
    let builder_name = generated_ident(
        &(stem_text.clone() + "Builder"),
        source.name.span(),
        "layout builder",
        errors,
    )?;
    let mutation_error_name = generated_ident(
        &(stem_text.clone() + "MutationError"),
        source.name.span(),
        "layout mutation error",
        errors,
    )?;
    let write_error_name = generated_ident(
        &(stem_text + "WriteError"),
        source.name.span(),
        "layout write error",
        errors,
    )?;
    register_name(
        generated_layout_names,
        &view_name,
        errors,
        "generated layout name collision",
    );
    register_name(
        generated_layout_names,
        &error_name,
        errors,
        "generated layout name collision",
    );
    for name in [
        &view_mut_name,
        &builder_name,
        &mutation_error_name,
        &write_error_name,
    ] {
        register_name(
            generated_layout_names,
            name,
            errors,
            "generated layout name collision",
        );
    }

    let kind = source.kind;
    let source_physical = source.physical;
    let source_field_indices: HashMap<_, _> = source
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| (normalized_name(&field.name), index))
        .collect();
    let mut fields = Vec::new();
    let mut normalized_field_indices = Vec::new();
    let mut field_names = HashMap::new();
    let mut getter_names = HashMap::new();
    let mut variants = HashMap::new();
    let mut placements = HashMap::new();
    for (declaration_index, field) in source.fields.into_iter().enumerate() {
        normalized_field_indices.push(None);
        let placement_span = field.placement.span();
        let generated_stem = normalized_name(&field.name);
        if field_names
            .insert(generated_stem.clone(), field.name.span())
            .is_some()
        {
            push(
                errors,
                Error::new(field.name.span(), "duplicate field identifier"),
            );
        }
        if getter_names
            .insert(generated_stem.clone(), field.name.span())
            .is_some()
        {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with an existing generated getter",
                ),
            );
        }
        if matches!(
            generated_stem.as_str(),
            "parse_prefix" | "parse_exact" | "as_bytes" | "WIDTH"
        ) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with a reserved generated member",
                ),
            );
        }
        let error_variant = generated_ident(
            &format!("Field{}", pascal_case(&generated_stem)),
            field.name.span(),
            "field error variant",
            errors,
        )?;
        let setter_name = generated_ident(
            &format!("set_{generated_stem}"),
            field.name.span(),
            "fixed-sequential field setter",
            errors,
        )?;
        let placement_label = match kind {
            syntax::LayoutKind::Sequential => "position",
            syntax::LayoutKind::Absolute => "offset",
        };
        let placement = parse_placement(&field.placement, placement_label, errors)?;
        if kind == syntax::LayoutKind::Sequential && placement == 0 {
            push(
                errors,
                Error::new(
                    field.placement.span(),
                    "position must be one-based (at least 1)",
                ),
            );
            continue;
        }
        if kind == syntax::LayoutKind::Absolute
            && placements
                .insert(placement, field.placement.span())
                .is_some()
        {
            push(
                errors,
                Error::new(field.placement.span(), "duplicate field offset"),
            );
            continue;
        }
        let field_kind = match field.codec {
            syntax::Codec::Bare(name) => match builtin(&name) {
                Some(value) => FieldKind::Codec(Codec::Builtin(value)),
                None => {
                    push(
                        errors,
                        Error::new(
                            name.span(),
                            "unknown bare codec; use a supported builtin or `codec(path::ToCodec)`",
                        ),
                    );
                    continue;
                }
            },
            syntax::Codec::Custom(path) => FieldKind::Codec(Codec::Custom(path)),
            syntax::Codec::Bytes(width) => FieldKind::Codec(Codec::Bytes(width)),
            syntax::Codec::Prefix(path) => {
                if path.get_ident().and_then(builtin).is_some() {
                    push(
                        errors,
                        Error::new_spanned(&path, "fixed builtins cannot be used as prefix codecs"),
                    );
                }
                FieldKind::Codec(Codec::Prefix(path))
            }
            syntax::Codec::Region(source) => {
                if kind == syntax::LayoutKind::Absolute {
                    push(
                        errors,
                        Error::new(
                            field.name.span(),
                            "regions are unsupported in absolute layouts",
                        ),
                    );
                }
                let source_name = normalized_name(&source);
                let Some(&length_source) = source_field_indices.get(&source_name) else {
                    push(
                        errors,
                        Error::new(source.span(), "unknown region length field"),
                    );
                    continue;
                };
                FieldKind::Region {
                    length_source,
                    length_source_span: source.span(),
                }
            }
        };
        let is_prefix = matches!(
            &field_kind,
            FieldKind::Codec(codec) if codec.is_prefix()
        );
        let is_region = matches!(&field_kind, FieldKind::Region { .. });
        if kind == syntax::LayoutKind::Absolute && is_prefix {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "prefix codecs are unsupported in absolute layouts",
                ),
            );
        }
        if !is_region {
            register_name(
                &mut variants,
                &error_variant,
                errors,
                "generated field error variant collision",
            );
        }
        let encoded_getter = generated_ident(
            &format!("{generated_stem}_encoded"),
            field.name.span(),
            "prefix representation getter",
            errors,
        )?;
        if is_prefix {
            register_name(
                &mut getter_names,
                &encoded_getter,
                errors,
                "prefix representation getter conflicts with an existing generated getter",
            );
        }
        let boundary = generated_ident(
            &format!("__wire_end_{generated_stem}"),
            field.name.span(),
            "dynamic field boundary",
            errors,
        )?;
        let projections = if is_prefix || is_region {
            if !field.projections.is_empty() {
                let message = if is_region {
                    "region fields cannot own bit projections"
                } else {
                    "prefix codec fields cannot own bit projections"
                };
                for projection in field.projections {
                    push(errors, Error::new(projection.name.span(), message));
                }
            }
            Vec::new()
        } else {
            let FieldKind::Codec(codec) = &field_kind else {
                continue;
            };
            normalize_projections(field.projections, codec, &mut getter_names, errors)?
        };
        let normalized_index = fields.len();
        fields.push(Field {
            docs: field.docs,
            name: field.name,
            kind: field_kind,
            declaration_index,
            placement,
            placement_span,
            error_variant,
            setter_name,
            encoded_getter,
            boundary,
            is_region_length_source: false,
            projections,
        });
        normalized_field_indices[declaration_index] = Some(normalized_index);
    }

    let pending_regions: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(region_index, field)| match &field.kind {
            FieldKind::Region {
                length_source,
                length_source_span,
            } => Some((region_index, *length_source, *length_source_span)),
            FieldKind::Codec(_) => None,
        })
        .collect();
    for (region_index, source_declaration, source_span) in pending_regions {
        let Some(Some(source_index)) = normalized_field_indices.get(source_declaration) else {
            continue;
        };
        let source_index = *source_index;
        if fields[source_index].is_region() {
            push(
                errors,
                Error::new(source_span, "region fields cannot be region length sources"),
            );
            continue;
        }
        if fields[source_index].placement >= fields[region_index].placement {
            push(
                errors,
                Error::new(
                    source_span,
                    "region length field must physically precede the region",
                ),
            );
            continue;
        }
        if let FieldKind::Region { length_source, .. } = &mut fields[region_index].kind {
            *length_source = source_index;
        }
        fields[source_index].is_region_length_source = true;
    }

    let mut physical_order = Vec::new();
    if kind == syntax::LayoutKind::Sequential {
        let mut physical_placements = HashMap::new();
        for entry in source_physical {
            let (item, span) = match entry {
                syntax::PhysicalEntry::Field(source_index) => {
                    let Some(Some(index)) = normalized_field_indices.get(source_index) else {
                        continue;
                    };
                    let field = &fields[*index];
                    (
                        PhysicalItem::Field {
                            index: *index,
                            position: field.placement,
                        },
                        field.placement_span,
                    )
                }
                syntax::PhysicalEntry::Padding { position, length } => {
                    let span = position.span();
                    let Some(position_value) = parse_placement(&position, "position", errors)
                    else {
                        continue;
                    };
                    let Some(length_value) = parse_placement(&length, "padding length", errors)
                    else {
                        continue;
                    };
                    if position_value == 0 {
                        push(
                            errors,
                            Error::new(position.span(), "position must be one-based (at least 1)"),
                        );
                    }
                    if length_value == 0 {
                        push(
                            errors,
                            Error::new(length.span(), "padding length must be nonzero"),
                        );
                    }
                    (
                        PhysicalItem::Padding {
                            position: position_value,
                            length: length_value,
                        },
                        span,
                    )
                }
                syntax::PhysicalEntry::Alignment { position, boundary } => {
                    let span = position.span();
                    let Some(position_value) = parse_placement(&position, "position", errors)
                    else {
                        continue;
                    };
                    let Some(boundary_value) =
                        parse_placement(&boundary, "alignment boundary", errors)
                    else {
                        continue;
                    };
                    if position_value == 0 {
                        push(
                            errors,
                            Error::new(position.span(), "position must be one-based (at least 1)"),
                        );
                    }
                    if boundary_value == 0 {
                        push(
                            errors,
                            Error::new(boundary.span(), "alignment boundary must be nonzero"),
                        );
                    }
                    (
                        PhysicalItem::Alignment {
                            position: position_value,
                            boundary: boundary_value,
                        },
                        span,
                    )
                }
            };
            let position = item.position();
            if physical_placements.insert(position, span).is_some() {
                let field_only = matches!(item, PhysicalItem::Field { .. })
                    && physical_order.iter().any(|previous: &PhysicalItem| {
                        previous.position() == position
                            && matches!(previous, PhysicalItem::Field { .. })
                    });
                push(
                    errors,
                    Error::new(
                        span,
                        if field_only {
                            "duplicate field position"
                        } else {
                            "duplicate physical position"
                        },
                    ),
                );
            }
            physical_order.push(item);
        }
        physical_order.sort_by_key(PhysicalItem::position);
        for expected in 1..=physical_order.len() {
            if !physical_placements.contains_key(&expected) {
                push(
                    errors,
                    Error::new(
                        source.name.span(),
                        format!("positions must be contiguous; missing position {expected}"),
                    ),
                );
            }
        }
    } else {
        for entry in source_physical {
            match entry {
                syntax::PhysicalEntry::Field(_) => {}
                syntax::PhysicalEntry::Padding { position, .. } => push(
                    errors,
                    Error::new(
                        position.span(),
                        "padding is unsupported in absolute layouts",
                    ),
                ),
                syntax::PhysicalEntry::Alignment { position, .. } => push(
                    errors,
                    Error::new(
                        position.span(),
                        "alignment is unsupported in absolute layouts",
                    ),
                ),
            }
        }
    }

    let has_dynamic = fields
        .iter()
        .any(|field| field.is_prefix() || field.is_region());
    if kind == syntax::LayoutKind::Absolute
        || (kind == syntax::LayoutKind::Sequential && !has_dynamic)
    {
        validate_fixed_layout_namespace(&fields, errors);
    } else {
        validate_dynamic_layout_namespace(&fields, errors);
    }
    let data = LayoutData {
        docs: source.docs,
        visibility: source.visibility,
        view_name,
        error_name,
        view_mut_name,
        builder_name,
        mutation_error_name,
        write_error_name,
        fields,
    };
    Some(match kind {
        syntax::LayoutKind::Sequential => Layout::Sequential(SequentialLayout {
            data,
            physical_order,
            has_dynamic,
        }),
        syntax::LayoutKind::Absolute => {
            let mut offset_order: Vec<_> = (0..data.fields.len()).collect();
            offset_order.sort_by_key(|&index| data.fields[index].placement);
            Layout::Absolute(AbsoluteLayout { data, offset_order })
        }
    })
}

fn validate_dynamic_layout_namespace(fields: &[Field], errors: &mut Option<Error>) {
    let reserved = [
        "parse_prefix_mut",
        "parse_exact_mut",
        "as_view",
        "into_view",
    ];
    let mut getters = HashMap::new();
    for field in fields {
        let name = normalized_name(&field.name);
        if reserved.contains(&name.as_str()) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with a reserved generated member",
                ),
            );
        }
        getters.insert(name, field.name.span());
        for projection in &field.projections {
            let name = normalized_name(&projection.name);
            if reserved.contains(&name.as_str()) {
                push(
                    errors,
                    Error::new(
                        projection.name.span(),
                        "projection name conflicts with a reserved generated member",
                    ),
                );
            }
            getters.insert(name, projection.name.span());
        }
    }
    let mut setters = HashMap::new();
    for field in fields.iter().filter(|field| {
        matches!(field.codec(), Some(codec) if !codec.is_prefix()) && !field.is_region_length_source
    }) {
        let name = normalized_name(&field.setter_name);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated field setter conflicts with an existing getter",
                ),
            );
        }
        if setters.insert(name, field.name.span()).is_some() {
            push(
                errors,
                Error::new(field.name.span(), "generated field setter collision"),
            );
        }
    }
    let mut fluent = HashMap::new();
    for field in fields.iter().filter(|field| !field.is_region_length_source) {
        let name = normalized_name(&field.name);
        if matches!(name.as_str(), "new" | "build_into") {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with a generated builder member",
                ),
            );
        }
        if fluent.insert(name, field.name.span()).is_some() {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated builder fluent method collision",
                ),
            );
        }
    }
}

fn validate_fixed_layout_namespace(fields: &[Field], errors: &mut Option<Error>) {
    let reserved = [
        "parse_prefix",
        "parse_exact",
        "as_bytes",
        "WIDTH",
        "parse_prefix_mut",
        "parse_exact_mut",
        "as_view",
        "into_view",
    ];
    let mut getters = HashMap::new();
    for field in fields {
        let name = normalized_name(&field.name);
        if reserved.contains(&name.as_str()) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with a reserved generated member",
                ),
            );
        }
        if matches!(name.as_str(), "new" | "build_into") {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "field name conflicts with a generated builder member",
                ),
            );
        }
        getters.insert(name, field.name.span());
        for projection in &field.projections {
            let name = normalized_name(&projection.name);
            if reserved.contains(&name.as_str()) {
                push(
                    errors,
                    Error::new(
                        projection.name.span(),
                        "projection name conflicts with a reserved generated member",
                    ),
                );
            }
            getters.insert(name, projection.name.span());
        }
    }
    let mut setters = HashMap::new();
    for field in fields {
        let name = normalized_name(&field.setter_name);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated field setter conflicts with an existing getter",
                ),
            );
        }
        if setters.insert(name, field.name.span()).is_some() {
            push(
                errors,
                Error::new(field.name.span(), "generated field setter collision"),
            );
        }
    }
}

fn projection_storage(codec: &Codec) -> Option<(UnsignedType, usize)> {
    match codec {
        Codec::Builtin(Builtin::U8) => Some((UnsignedType::U8, 8)),
        Codec::Builtin(Builtin::BeU16 | Builtin::LeU16) => Some((UnsignedType::U16, 16)),
        Codec::Builtin(Builtin::BeU24 | Builtin::LeU24) => Some((UnsignedType::U32, 24)),
        Codec::Builtin(Builtin::BeU32 | Builtin::LeU32) => Some((UnsignedType::U32, 32)),
        Codec::Builtin(Builtin::BeU64 | Builtin::LeU64) => Some((UnsignedType::U64, 64)),
        Codec::Builtin(Builtin::BeU128 | Builtin::LeU128) => Some((UnsignedType::U128, 128)),
        _ => None,
    }
}
fn normalize_projections(
    source: Vec<syntax::Projection>,
    codec: &Codec,
    getter_names: &mut HashMap<String, Span>,
    errors: &mut Option<Error>,
) -> Option<Vec<Projection>> {
    if source.is_empty() {
        return Some(Vec::new());
    }
    let Some((value_type, width)) = projection_storage(codec) else {
        for projection in source {
            push(
                errors,
                Error::new(
                    projection.name.span(),
                    "projections require an unsigned builtin fixed codec",
                ),
            );
        }
        return None;
    };
    let mut result = Vec::new();
    for projection in source {
        let name_text = projection.name.to_string();
        let normalized = name_text
            .strip_prefix("r#")
            .unwrap_or(&name_text)
            .to_owned();
        if getter_names
            .insert(normalized, projection.name.span())
            .is_some()
        {
            push(
                errors,
                Error::new(
                    projection.name.span(),
                    "projection name conflicts with an existing generated getter",
                ),
            );
        }
        if matches!(
            name_text.strip_prefix("r#").unwrap_or(&name_text),
            "parse_prefix" | "parse_exact" | "as_bytes" | "WIDTH"
        ) {
            push(
                errors,
                Error::new(
                    projection.name.span(),
                    "projection name conflicts with a reserved generated member",
                ),
            );
        }
        let start = parse_placement(&projection.start, "projection bit index", errors)?;
        let end = parse_placement(&projection.end, "projection bit index", errors)?;
        if start > end {
            push(
                errors,
                Error::new(
                    projection.end.span(),
                    "projection range start must not exceed end",
                ),
            );
        }
        if end >= width {
            push(
                errors,
                Error::new(
                    projection.end.span(),
                    "projection bit index is outside the storage width",
                ),
            );
        }
        if result
            .iter()
            .any(|prior: &Projection| start <= prior.end && prior.start <= end)
        {
            push(
                errors,
                Error::new(projection.name.span(), "projection ranges must not overlap"),
            );
        }
        let kind = match projection.kind {
            syntax::ProjectionKind::Bit => ProjectionKind::Bit,
            syntax::ProjectionKind::Bits => ProjectionKind::Bits,
        };
        result.push(Projection {
            docs: projection.docs,
            name: projection.name,
            start,
            end,
            kind,
            value_type,
        });
    }
    Some(result)
}

fn push(slot: &mut Option<Error>, error: Error) {
    if let Some(existing) = slot {
        existing.combine(error);
    } else {
        *slot = Some(error);
    }
}
fn register_name(
    names: &mut HashMap<String, Span>,
    name: &Ident,
    errors: &mut Option<Error>,
    message: &str,
) {
    if names.insert(name.to_string(), name.span()).is_some() {
        push(errors, Error::new(name.span(), message));
    }
}
fn parse_placement(
    literal: &syn::LitInt,
    label: &str,
    errors: &mut Option<Error>,
) -> Option<usize> {
    let digits = literal.to_string().replace('_', "");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        push(
            errors,
            Error::new(
                literal.span(),
                format!("{label} must be an unsuffixed base-10 integer"),
            ),
        );
        return None;
    }
    match digits.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            push(
                errors,
                Error::new(literal.span(), format!("{label} does not fit in usize")),
            );
            None
        }
    }
}
fn normalized_name(name: &Ident) -> String {
    let text = name.to_string();
    text.strip_prefix("r#").unwrap_or(&text).to_owned()
}

fn generated_ident(
    text: &str,
    span: Span,
    role: &str,
    errors: &mut Option<Error>,
) -> Option<Ident> {
    if text.is_empty() {
        push(
            errors,
            Error::new(span, format!("cannot form a nonempty {role}")),
        );
        return None;
    }
    match syn::parse_str::<Ident>(text) {
        Ok(mut name) => {
            name.set_span(span);
            Some(name)
        }
        Err(_) => {
            push(
                errors,
                Error::new(span, format!("cannot form a valid {role}")),
            );
            None
        }
    }
}
fn pascal_case(text: &str) -> String {
    text.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}
fn builtin(name: &Ident) -> Option<Builtin> {
    Some(match name.to_string().as_str() {
        "U8" => Builtin::U8,
        "I8" => Builtin::I8,
        "BeU16" => Builtin::BeU16,
        "LeU16" => Builtin::LeU16,
        "BeI16" => Builtin::BeI16,
        "LeI16" => Builtin::LeI16,
        "BeU24" => Builtin::BeU24,
        "LeU24" => Builtin::LeU24,
        "BeU32" => Builtin::BeU32,
        "LeU32" => Builtin::LeU32,
        "BeI32" => Builtin::BeI32,
        "LeI32" => Builtin::LeI32,
        "BeU64" => Builtin::BeU64,
        "LeU64" => Builtin::LeU64,
        "BeI64" => Builtin::BeI64,
        "LeI64" => Builtin::LeI64,
        "BeU128" => Builtin::BeU128,
        "LeU128" => Builtin::LeU128,
        "BeI128" => Builtin::BeI128,
        "LeI128" => Builtin::LeI128,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;
    fn model(source: &str) -> Result<Invocation> {
        normalize(parse_str(source)?)
    }
    fn error(source: &str, needle: &str) {
        match model(source) {
            Ok(_) => panic!("expected semantic error"),
            Err(value) => assert!(value.to_string().contains(needle), "{value}"),
        }
    }
    #[test]
    fn absolute_keeps_declaration_order_and_sorts_offsets() {
        let value = model("pub absolute layout Header { field tail: BeU16 { offset: 4; } field kind: U8 { offset: 0; } } layout Tail { field end: U8 { position: 1; } }").unwrap();
        let Layout::Absolute(header) = &value.layouts[0] else {
            panic!("expected absolute")
        };
        assert_eq!(header.data.fields[0].placement, 4);
        assert_eq!(header.data.fields[1].placement, 0);
        assert_eq!(header.offset_order, [1, 0]);
        assert!(matches!(&value.layouts[1], Layout::Sequential(_)));
    }
    #[test]
    fn absolute_offsets_allow_gaps_and_reject_duplicates() {
        assert!(
            model("absolute layout H { field a: U8 { offset: 0; } field b: U8 { offset: 4; } }")
                .is_ok()
        );
        error(
            "absolute layout H { field a: U8 { offset: 0; } field b: U8 { offset: 0; } }",
            "duplicate field offset",
        );
        error(
            "absolute layout H { field a: U8 { offset: 1u8; } }",
            "unsuffixed",
        );
        error(
            "absolute layout H { field a: U8 { offset: 0x1; } }",
            "base-10",
        );
    }

    #[test]
    fn retains_sequential_declaration_order_docs_and_custom_paths() {
        let value = model(
            "/// header\npub layout Header { #[doc = \"second\"] field second: codec(crate::Code) { position: 2; } field first: U8 { position: 1; } } layout Tail { field end: LeU16 { position: 1; } }",
        )
        .unwrap();

        assert_eq!(value.layouts.len(), 2);
        let Layout::Sequential(header) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        assert_eq!(header.data.docs.len(), 1);
        assert!(matches!(&header.data.visibility, Visibility::Public(_)));
        assert_eq!(header.data.fields[0].declaration_index, 0);
        assert_eq!(header.data.fields[0].placement, 2);
        assert_eq!(
            header.physical_order,
            [
                PhysicalItem::Field {
                    index: 1,
                    position: 1,
                },
                PhysicalItem::Field {
                    index: 0,
                    position: 2,
                },
            ]
        );
        assert_eq!(header.data.view_name.to_string(), "HeaderView");
        assert_eq!(header.data.error_name.to_string(), "HeaderError");
        assert_eq!(header.data.view_mut_name.to_string(), "HeaderViewMut");
        assert_eq!(header.data.builder_name.to_string(), "HeaderBuilder");
        assert_eq!(
            header.data.mutation_error_name.to_string(),
            "HeaderMutationError"
        );
        assert_eq!(header.data.write_error_name.to_string(), "HeaderWriteError");
        assert_eq!(header.data.fields[0].setter_name.to_string(), "set_second");
        assert_eq!(
            header.data.fields[0].error_variant.to_string(),
            "FieldSecond"
        );
        assert_eq!(header.data.fields[0].docs.len(), 1);
        assert!(matches!(
            header.data.fields[0].codec(),
            Some(Codec::Custom(path)) if path.segments.len() == 2
        ));
    }

    #[test]
    fn normalizes_byte_widths_for_sequential_and_absolute_rendering() {
        let value = model(
            "layout Sequential { field bytes: bytes(16) { position: 1; } } absolute layout Absolute { field bytes: bytes(8) { offset: 0; } }",
        ).unwrap();
        let Layout::Sequential(sequential) = &value.layouts[0] else {
            panic!("expected sequential")
        };
        let Layout::Absolute(absolute) = &value.layouts[1] else {
            panic!("expected absolute")
        };
        assert!(matches!(
            sequential.data.fields[0].codec(),
            Some(Codec::Bytes(16))
        ));
        assert!(matches!(
            absolute.data.fields[0].codec(),
            Some(Codec::Bytes(8))
        ));
    }

    #[test]
    fn raw_field_identifiers_keep_getter_spelling_and_normalize_generated_names() {
        let value = model("layout Header { field r#type: U8 { position: 1; } }").unwrap();
        let Layout::Sequential(header) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        let field = &header.data.fields[0];
        assert_eq!(field.name.to_string(), "r#type");
        assert_eq!(field.error_variant.to_string(), "FieldType");

        error(
            "layout Header { field r#parse_prefix: U8 { position: 1; } }",
            "reserved generated",
        );
    }

    #[test]
    fn fixed_sequential_names_normalize_and_reject_builder_and_setter_collisions() {
        error(
            "layout H { field as_view: U8 { position: 1; } }",
            "reserved generated",
        );
        error(
            "layout H { field new: U8 { position: 1; } }",
            "generated builder",
        );
        error(
            "layout H { field build_into: U8 { position: 1; } }",
            "generated builder",
        );
        error(
            "layout H { field set_value: U8 { position: 1; } field value: U8 { position: 2; } }",
            "generated field setter conflicts",
        );
        error(
            "layout H { field value: U8 { position: 1; projections { bit set_value: 0; } } }",
            "generated field setter conflicts",
        );
        error(
            "layout H { field r#as_view: U8 { position: 1; } }",
            "reserved generated",
        );
        error(
            "layout Header { field value: U8 { position: 1; } } layout HeaderMutation { field value: U8 { position: 1; } }",
            "generated layout name collision",
        );
    }

    #[test]
    fn maps_every_builtin_without_requiring_codec_equality() {
        let names = [
            "U8", "I8", "BeU16", "LeU16", "BeI16", "LeI16", "BeU24", "LeU24", "BeU32", "LeU32",
            "BeI32", "LeI32", "BeU64", "LeU64", "BeI64", "LeI64", "BeU128", "LeU128", "BeI128",
            "LeI128",
        ];
        let fields: String = names
            .iter()
            .enumerate()
            .map(|(index, name)| format!("field f{index}: {name} {{ position: {}; }}", index + 1))
            .collect();
        let value = model(&format!("layout L {{ {fields} }}")).unwrap();
        let Layout::Sequential(layout) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };

        for (field, expected) in layout.data.fields.iter().zip(names) {
            let expected = builtin(&syn::parse_str(expected).unwrap()).unwrap();
            assert!(matches!(field.codec(), Some(Codec::Builtin(actual)) if *actual == expected));
        }
    }

    #[test]
    fn validates_sequential_positions_names_and_empty_models() {
        for (source, needle) in [
            ("", "at least one layout"),
            ("layout L {}", "at least one field"),
            ("layout L { field a: U8 { position: 0; } }", "one-based"),
            (
                "layout L { field a: U8 { position: 1; } field b: U8 { position: 1; } }",
                "duplicate field position",
            ),
            (
                "layout L { field a: U8 { position: 2; } }",
                "missing position 1",
            ),
            ("layout L { field a: U8 { position: 1u8; } }", "unsuffixed"),
            ("layout L { field a: U8 { position: 0x1; } }", "base-10"),
            (
                "layout L { field a: U8 { position: 999999999999999999999999999999999999999; } }",
                "does not fit",
            ),
            (
                "layout L { field a: U8 { position: 1; } field a: U8 { position: 2; } }",
                "duplicate field identifier",
            ),
            (
                "layout L { field foo_bar: U8 { position: 1; } field foo__bar: U8 { position: 2; } }",
                "generated field error variant collision",
            ),
            (
                "layout L { field parse_exact: U8 { position: 1; } }",
                "reserved generated",
            ),
            (
                "layout L { field as_bytes: U8 { position: 1; } }",
                "reserved generated",
            ),
            (
                "layout L { field WIDTH: U8 { position: 1; } }",
                "reserved generated",
            ),
            (
                "layout L { field a: Nope { position: 1; } }",
                "unknown bare codec",
            ),
            (
                "layout r#type { field a: U8 { position: 1; } }",
                "raw layout",
            ),
            (
                "layout L { field a: U8 { position: 1; } } layout L { field b: U8 { position: 1; } }",
                "duplicate layout stem",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn validates_unsigned_projection_storage_ranges_and_namespace() {
        let value = model("layout H { field top_flags: BeU24 { position: 1; projections { bit top: 23; } } field all_flags: BeU24 { position: 2; projections { bits all: 0..=23; } } }").unwrap();
        let Layout::Sequential(layout) = &value.layouts[0] else {
            panic!("expected sequential")
        };
        assert_eq!(
            layout.data.fields[0].projections[0].kind,
            ProjectionKind::Bit
        );
        assert_eq!(
            layout.data.fields[1].projections[0].kind,
            ProjectionKind::Bits
        );
        assert_eq!(layout.data.fields[1].projections[0].end, 23);
        error(
            "layout H { field flags: I8 { position: 1; projections { bit x: 0; } } }",
            "unsigned builtin",
        );
        error(
            "layout H { field flags: codec(crate::C) { position: 1; projections { bit x: 0; } } }",
            "unsigned builtin",
        );
        error(
            "layout H { field bytes: bytes(16) { position: 1; projections { bit x: 0; } } }",
            "unsigned builtin",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bit x: 8; } } }",
            "outside",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bits x: 4..=3; } } }",
            "start must not exceed",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bits x: 1..=3; bit y: 3; } } }",
            "must not overlap",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bit parse_exact: 0; } } }",
            "reserved",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bit r#parse_exact: 0; } } }",
            "reserved",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bit x: 0; bit x: 1; } } }",
            "conflicts",
        );
        error(
            "layout H { field first: U8 { position: 1; projections { bit x: 0; } } field second: U8 { position: 2; projections { bit x: 1; } } }",
            "conflicts",
        );
        error(
            "layout H { field flags: U8 { position: 1; projections { bit r#type: 0; } } field r#type: U8 { position: 2; } }",
            "conflicts",
        );
    }

    #[test]
    fn validates_fixed_absolute_write_namespace() {
        for (source, needle) in [
            (
                "absolute layout H { field parse_prefix_mut: U8 { offset: 0; } }",
                "reserved generated member",
            ),
            (
                "absolute layout H { field r#new: U8 { offset: 0; } }",
                "generated builder member",
            ),
            (
                "absolute layout H { field x: U8 { offset: 0; } field set_x: U8 { offset: 1; } }",
                "generated field setter conflicts",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn every_eligible_endian_builtin_has_its_semantic_width_and_type() {
        for (name, expected_type, width) in [
            ("U8", UnsignedType::U8, 8),
            ("BeU16", UnsignedType::U16, 16),
            ("LeU16", UnsignedType::U16, 16),
            ("BeU24", UnsignedType::U32, 24),
            ("LeU24", UnsignedType::U32, 24),
            ("BeU32", UnsignedType::U32, 32),
            ("LeU32", UnsignedType::U32, 32),
            ("BeU64", UnsignedType::U64, 64),
            ("LeU64", UnsignedType::U64, 64),
            ("BeU128", UnsignedType::U128, 128),
            ("LeU128", UnsignedType::U128, 128),
        ] {
            let codec = Codec::Builtin(builtin(&syn::parse_str(name).unwrap()).unwrap());
            assert_eq!(projection_storage(&codec), Some((expected_type, width)));
        }
        for name in [
            "I8", "BeI16", "LeI16", "BeI32", "LeI32", "BeI64", "LeI64", "BeI128", "LeI128",
        ] {
            let codec = Codec::Builtin(builtin(&syn::parse_str(name).unwrap()).unwrap());
            assert!(projection_storage(&codec).is_none());
        }
    }

    #[test]
    fn pascal_case_keeps_only_identifier_components() {
        assert_eq!(pascal_case("foo_bar"), "FooBar");
        assert_eq!(pascal_case("foo__bar"), "FooBar");
        assert_eq!(pascal_case("_"), "");
    }

    #[test]
    fn normalizes_prefix_layout_boundaries_and_rejects_illegal_owners() {
        let value = model(
            "layout H { field tail: U8 { position: 2; } field r#type: prefix(crate::P) { position: 1; } } layout F { field value: U8 { position: 1; } }",
        )
        .unwrap();
        let Layout::Sequential(dynamic) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        assert!(dynamic.has_dynamic);
        let prefix = &dynamic.data.fields[1];
        assert!(matches!(prefix.codec(), Some(Codec::Prefix(_))));
        assert_eq!(prefix.encoded_getter.to_string(), "type_encoded");
        assert_eq!(prefix.boundary.to_string(), "__wire_end_type");
        let Layout::Sequential(fixed) = &value.layouts[1] else {
            panic!("expected sequential layout")
        };
        assert!(!fixed.has_dynamic);

        for (source, needle) in [
            (
                "absolute layout H { field value: prefix(crate::P) { offset: 0; } }",
                "unsupported in absolute",
            ),
            (
                "layout H { field value: prefix(crate::P) { position: 1; projections { bit x: 0; } } }",
                "cannot own bit projections",
            ),
            (
                "layout H { field r#type: prefix(crate::P) { position: 1; } field type_encoded: U8 { position: 2; } }",
                "generated getter",
            ),
            (
                "layout H { field value: prefix(U8) { position: 1; } }",
                "fixed builtins",
            ),
        ] {
            error(source, needle);
        }
    }
    #[test]
    fn normalizes_regions_and_rejects_invalid_length_sources() {
        let value = model(
            "layout H { field second: region(length) { position: 3; } field length: U8 { position: 1; } field first: region(length) { position: 2; } }",
        )
        .unwrap();
        let Layout::Sequential(layout) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        assert!(layout.has_dynamic);
        assert_eq!(layout.data.fields[0].name, "second");
        assert_eq!(layout.data.fields[1].name, "length");
        assert_eq!(layout.data.fields[2].name, "first");
        assert!(layout.data.fields[1].is_region_length_source);
        assert!(!layout.data.fields[0].is_region_length_source);
        assert!(!layout.data.fields[2].is_region_length_source);
        assert!(matches!(
            layout.data.fields[0].kind,
            FieldKind::Region {
                length_source: 1,
                ..
            }
        ));
        assert!(matches!(
            layout.data.fields[2].kind,
            FieldKind::Region {
                length_source: 1,
                ..
            }
        ));
        assert_eq!(
            layout.physical_order,
            [
                PhysicalItem::Field {
                    index: 1,
                    position: 1,
                },
                PhysicalItem::Field {
                    index: 2,
                    position: 2,
                },
                PhysicalItem::Field {
                    index: 0,
                    position: 3,
                },
            ]
        );

        for (source, needle) in [
            (
                "layout H { field length: U8 { position: 1; } field payload: region(missing) { position: 2; } }",
                "unknown region length field",
            ),
            (
                "layout H { field payload: region(length) { position: 1; } field length: U8 { position: 2; } }",
                "must physically precede",
            ),
            (
                "layout H { field base: U8 { position: 1; } field length: region(base) { position: 2; } field payload: region(length) { position: 3; } }",
                "cannot be region length sources",
            ),
            (
                "absolute layout H { field length: U8 { offset: 0; } field payload: region(length) { offset: 1; } }",
                "regions are unsupported in absolute",
            ),
            (
                "layout H { field length: U8 { position: 1; } field payload: region(length) { position: 2; projections { bit x: 0; } } }",
                "region fields cannot own bit projections",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn dynamic_mutation_namespace_and_region_source_ownership_are_normalized() {
        for source in [
            "layout H { field r#parse_prefix_mut: prefix(crate::P) { position: 1; } }",
            "layout H { field value: prefix(crate::P) { position: 1; } field parse_exact_mut: U8 { position: 2; } }",
            "layout H { field value: prefix(crate::P) { position: 1; } field flags: U8 { position: 2; projections { bit set_flags: 0; } } }",
        ] {
            error(
                source,
                if source.contains("set_flags") {
                    "generated field setter conflicts"
                } else {
                    "reserved generated member"
                },
            );
        }
        model("layout H { field length: U8 { position: 1; } field payload: region(length) { position: 2; } field set_length: U8 { position: 3; } }")
            .expect("a source setter is omitted, so its hypothetical collision is legal");

        let value = model("layout H { field payload: region(length) { position: 3; } field length: prefix(crate::P) { position: 1; } field again: region(length) { position: 2; } field fixed: U8 { position: 4; } }").unwrap();
        let Layout::Sequential(layout) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        assert!(layout.data.fields[1].is_region_length_source);
        assert!(!layout.data.fields[0].is_region_length_source);
        assert!(!layout.data.fields[2].is_region_length_source);
        assert!(!layout.data.fields[3].is_region_length_source);
    }

    #[test]
    fn dynamic_builder_namespace_tracks_only_emitted_fluent_methods() {
        error(
            "layout H { field r#new: U8 { position: 1; } field dynamic: prefix(crate::P) { position: 2; } }",
            "generated builder member",
        );
        error(
            "layout H { field r#build_into: region(length) { position: 2; } field length: U8 { position: 1; } }",
            "generated builder member",
        );
        error(
            "layout H { field foo: U8 { position: 1; } field r#foo: U8 { position: 2; } field dynamic: prefix(crate::P) { position: 3; } }",
            "duplicate field identifier",
        );
        model("layout H { field new: U8 { position: 1; } field payload: region(new) { position: 2; } }")
            .expect("source fields omit builder fluent methods");
        model("layout H { field build_into: U8 { position: 1; } field payload: region(build_into) { position: 2; } }")
            .expect("source fields omit builder fluent methods");
    }

    #[test]
    fn normalizes_padding_alignment_and_shared_physical_positions() {
        let value = model(
            "layout H { align { position: 3; boundary: 1; } field tail: U8 { position: 4; } padding { position: 2; length: 3; } field head: U8 { position: 1; } }",
        )
        .unwrap();
        let Layout::Sequential(layout) = &value.layouts[0] else {
            panic!("expected sequential layout")
        };
        assert_eq!(layout.data.fields[0].name, "tail");
        assert_eq!(layout.data.fields[1].name, "head");
        assert_eq!(
            layout.physical_order,
            [
                PhysicalItem::Field {
                    index: 1,
                    position: 1,
                },
                PhysicalItem::Padding {
                    position: 2,
                    length: 3,
                },
                PhysicalItem::Alignment {
                    position: 3,
                    boundary: 1,
                },
                PhysicalItem::Field {
                    index: 0,
                    position: 4,
                },
            ]
        );
        assert!(!layout.has_dynamic);

        for (source, needle) in [
            (
                "layout H { field a: U8 { position: 1; } padding { position: 2; length: 0; } }",
                "padding length must be nonzero",
            ),
            (
                "layout H { field a: U8 { position: 1; } align { position: 2; boundary: 0; } }",
                "alignment boundary must be nonzero",
            ),
            (
                "layout H { field a: U8 { position: 1; } padding { position: 1; length: 2; } }",
                "duplicate physical position",
            ),
            (
                "layout H { field a: U8 { position: 1; } align { position: 3; boundary: 4; } }",
                "missing position 2",
            ),
            (
                "layout H { field a: U8 { position: 1; } padding { position: 0; length: 2; } }",
                "position must be one-based",
            ),
            (
                "absolute layout H { field a: U8 { offset: 0; } padding { position: 1; length: 2; } }",
                "padding is unsupported in absolute layouts",
            ),
            (
                "absolute layout H { field a: U8 { offset: 0; } align { position: 1; boundary: 4; } }",
                "alignment is unsupported in absolute layouts",
            ),
            (
                "layout H { padding { position: 1; length: 2; } }",
                "layout must contain at least one field",
            ),
        ] {
            error(source, needle);
        }
    }
}
