//! Validated, renderer-ready layout model.

use std::collections::HashMap;

use proc_macro2::Span;
use syn::{Attribute, Error, Ident, Path, Result, Visibility};

use crate::syntax;

/// Fully validated invocation in source declaration order.
pub(crate) struct Invocation {
    pub(crate) items: Vec<Item>,
}

/// A renderer-ready top-level declaration.
pub(crate) enum Item {
    /// A byte-backed layout declaration.
    Layout(Layout),
    /// A transparent nominal fixed integer scalar.
    Scalar(Scalar),
}

/// A normalized scalar whose storage has already been resolved to a builtin integer codec.
pub(crate) struct Scalar {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) name: Ident,
    pub(crate) storage: Builtin,
    pub(crate) raw_type: IntegerType,
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
    /// Optional semantic representation layered over the physical codec.
    pub(crate) mapping: Option<Mapping>,
    /// Generated physical-representation getter for a mapped field.
    pub(crate) raw_name: Option<Ident>,
    /// Generated physical-representation setter for a mapped field.
    pub(crate) raw_setter_name: Option<Ident>,
    pub(crate) declaration_index: usize,
    pub(crate) placement: usize,
    pub(crate) placement_span: Span,
    pub(crate) error_variant: Ident,
    /// Fixed-sequential setter name, normalized with the source identifier.
    pub(crate) setter_name: Ident,
    /// Dynamic-range mutable accessor name, normalized with the source identifier.
    pub(crate) range_mut_name: Option<Ident>,
    /// Raw-wire getter name used when this is a prefix field.
    pub(crate) raw_getter: Ident,
    /// Stored end-boundary name used when this is a prefix field.
    pub(crate) boundary: Ident,
    /// Whether any byte range is derived from this field's raw value.
    pub(crate) is_derived_range_source: bool,
    pub(crate) projections: Vec<Projection>,
}

impl Field {
    pub(crate) fn codec(&self) -> Option<&Codec> {
        match &self.kind {
            FieldKind::Codec(codec) => Some(codec),
            FieldKind::ByteRange { .. } => None,
        }
    }

    pub(crate) fn is_prefix(&self) -> bool {
        self.codec().is_some_and(Codec::is_prefix)
    }

    pub(crate) const fn is_byte_range(&self) -> bool {
        matches!(self.kind, FieldKind::ByteRange { .. })
    }

    pub(crate) const fn is_buf_end_range(&self) -> bool {
        matches!(
            self.kind,
            FieldKind::ByteRange {
                end: ByteRangeEnd::BufEnd
            }
        )
    }
}

/// Fully resolved semantic kind of a named field.
pub(crate) enum FieldKind {
    Codec(Codec),
    ByteRange { end: ByteRangeEnd },
}

/// Validated byte-range end algebra.
pub(crate) enum ByteRangeEnd {
    Relative { source: usize, source_span: Span },
    Absolute { source: usize, source_span: Span },
    BufEnd,
}

/// A semantic fixed mapping and its physical raw representation.
pub(crate) struct Mapping {
    pub(crate) semantic: Path,
    pub(crate) raw: MappingRaw,
}

/// Physical raw representation preserved by a semantic fixed mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MappingRaw {
    Builtin(IntegerType),
    Bytes(usize),
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

/// Semantic integer type owned by a declared scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegerType {
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
}

/// Normalizes parsed syntax and rejects semantic violations before rendering.
pub(crate) fn normalize(parsed: syntax::Invocation) -> Result<Invocation> {
    let mut errors = None;
    if parsed.items.is_empty() {
        push(
            &mut errors,
            Error::new(
                Span::call_site(),
                "wire_repr requires at least one declaration",
            ),
        );
    }
    let mut stems = HashMap::new();
    let mut generated_names = HashMap::new();
    let scalar_names: HashMap<_, _> = parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syntax::Item::Scalar(scalar) => {
                Some((normalized_name(&scalar.name), scalar.name.span()))
            }
            syntax::Item::Layout(_) => None,
        })
        .collect();
    let mut items = Vec::new();
    for source in parsed.items {
        match source {
            syntax::Item::Layout(source) => {
                if stems
                    .insert(normalized_name(&source.name), source.name.span())
                    .is_some()
                {
                    push(
                        &mut errors,
                        Error::new(source.name.span(), "duplicate layout stem"),
                    );
                }
                if let Some(layout) =
                    normalize_layout(source, &scalar_names, &mut generated_names, &mut errors)
                {
                    items.push(Item::Layout(layout));
                }
            }
            syntax::Item::Scalar(source) => {
                if let Some(scalar) = normalize_scalar(source, &mut generated_names, &mut errors) {
                    items.push(Item::Scalar(scalar));
                }
            }
        }
    }
    match errors {
        Some(error) => Err(error),
        None => Ok(Invocation { items }),
    }
}

fn normalize_scalar(
    source: syntax::Scalar,
    generated_names: &mut HashMap<String, Span>,
    errors: &mut Option<Error>,
) -> Option<Scalar> {
    let storage = match source.storage {
        syntax::Codec::Bare(name) => match builtin(&name) {
            Some(storage) => storage,
            None => {
                push(
                    errors,
                    Error::new(
                        name.span(),
                        "scalar storage must be a supported builtin fixed integer codec",
                    ),
                );
                return None;
            }
        },
        syntax::Codec::Custom(path) => {
            push(
                errors,
                Error::new_spanned(
                    path,
                    "scalar storage must be a builtin fixed integer codec, not a custom path",
                ),
            );
            return None;
        }
        syntax::Codec::Bytes(_) => {
            push(
                errors,
                Error::new(
                    source.name.span(),
                    "scalar storage must be a builtin fixed integer codec; `bytes(N)` is unsupported",
                ),
            );
            return None;
        }
        syntax::Codec::Prefix(_) => {
            push(
                errors,
                Error::new(
                    source.name.span(),
                    "scalar storage must be a builtin fixed integer codec; `prefix(path)` is unsupported",
                ),
            );
            return None;
        }
        syntax::Codec::Range(_) => {
            push(
                errors,
                Error::new(
                    source.name.span(),
                    "scalar storage must be a builtin fixed integer codec; byte ranges are unsupported",
                ),
            );
            return None;
        }
    };
    let raw_type = scalar_raw_type(storage);
    register_name(
        generated_names,
        &source.name,
        errors,
        "generated top-level name collision",
    );
    Some(Scalar {
        docs: source.docs,
        visibility: source.visibility,
        name: source.name,
        storage,
        raw_type,
    })
}

fn normalize_layout(
    source: syntax::Layout,
    scalar_names: &HashMap<String, Span>,
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
    let mut implicit_field_positions = vec![None; source.fields.len()];
    if kind == syntax::LayoutKind::Sequential {
        let mut has_explicit = false;
        let mut has_implicit = false;
        for (source_position, entry) in source_physical.iter().enumerate() {
            let placement = match entry {
                syntax::PhysicalEntry::Field(index) => &source.fields[*index].placement,
                syntax::PhysicalEntry::Padding { placement, .. }
                | syntax::PhysicalEntry::Alignment { placement, .. } => placement,
            };
            has_explicit |= placement.is_explicit();
            has_implicit |= !placement.is_explicit();
            if let syntax::PhysicalEntry::Field(index) = entry
                && !placement.is_explicit()
            {
                implicit_field_positions[*index] = source_position.checked_add(1);
                if implicit_field_positions[*index].is_none() {
                    push(
                        errors,
                        Error::new(placement.span(), "implicit position does not fit in usize"),
                    );
                }
            }
        }
        if has_explicit && has_implicit {
            for entry in &source_physical {
                let placement = match entry {
                    syntax::PhysicalEntry::Field(index) => &source.fields[*index].placement,
                    syntax::PhysicalEntry::Padding { placement, .. }
                    | syntax::PhysicalEntry::Alignment { placement, .. } => placement,
                };
                if !placement.is_explicit() {
                    push(
                        errors,
                        Error::new(
                            placement.span(),
                            "`position` is required for every sequential entry when any position is explicit",
                        ),
                    );
                }
            }
        }
    }
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
        let placement = match &field.placement {
            syntax::Placement::Explicit(value) => parse_placement(value, placement_label, errors)?,
            syntax::Placement::Implicit(_) => {
                let Some(position) = implicit_field_positions[declaration_index] else {
                    push(errors, Error::new(placement_span, "expected `offset`"));
                    continue;
                };
                position
            }
        };
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
        let mapping = match field.mapping {
            Some(semantic) => match &field.codec {
                syntax::Codec::Bare(name) => match builtin(name) {
                    Some(storage) => Some(Mapping {
                        semantic,
                        raw: MappingRaw::Builtin(scalar_raw_type(storage)),
                    }),
                    None if scalar_names.contains_key(&normalized_name(name)) => {
                        push(
                            errors,
                            Error::new_spanned(
                                semantic,
                                "mappings are unsupported on declared scalar codecs",
                            ),
                        );
                        None
                    }
                    None => None,
                },
                syntax::Codec::Bytes(width) => Some(Mapping {
                    semantic,
                    raw: MappingRaw::Bytes(*width),
                }),
                syntax::Codec::Custom(_) => {
                    push(
                        errors,
                        Error::new_spanned(semantic, "mappings are unsupported on custom codecs"),
                    );
                    None
                }
                syntax::Codec::Prefix(_) => {
                    push(
                        errors,
                        Error::new_spanned(semantic, "mappings are unsupported on prefix codecs"),
                    );
                    None
                }
                syntax::Codec::Range(_) => {
                    push(
                        errors,
                        Error::new_spanned(
                            semantic,
                            "mappings are unsupported on byte range fields",
                        ),
                    );
                    None
                }
            },
            None => None,
        };
        let field_kind = match field.codec {
            syntax::Codec::Bare(name) => match builtin(&name) {
                Some(value) => FieldKind::Codec(Codec::Builtin(value)),
                None if scalar_names.contains_key(&normalized_name(&name)) => {
                    let path: Path = syn::parse_quote!(#name);
                    FieldKind::Codec(Codec::Custom(path))
                }
                None => {
                    push(
                        errors,
                        Error::new(
                            name.span(),
                            "unknown bare codec; use a supported builtin, a declared scalar, or a direct codec path",
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
            syntax::Codec::Range(range) => {
                if kind == syntax::LayoutKind::Absolute {
                    push(
                        errors,
                        Error::new(
                            field.name.span(),
                            "byte ranges are unsupported in absolute layouts",
                        ),
                    );
                }
                let start_span = match range.start {
                    syntax::ByteRangeStart::CurrentPos => None,
                    syntax::ByteRangeStart::BufStart(span)
                    | syntax::ByteRangeStart::BufEnd(span)
                    | syntax::ByteRangeStart::FieldStart(span)
                    | syntax::ByteRangeStart::FieldEnd(span) => Some(span),
                };
                if let Some(span) = start_span {
                    push(
                        errors,
                        Error::new(span, "byte range start must be `current_pos`"),
                    );
                }
                let end = match range.end {
                    syntax::ByteRangeEnd::BufEnd => ByteRangeEnd::BufEnd,
                    syntax::ByteRangeEnd::Relative { source, span } => {
                        let source_name = normalized_name(&source);
                        let Some(&source) = source_field_indices.get(&source_name) else {
                            push(errors, Error::new(span, "unknown byte range source field"));
                            continue;
                        };
                        ByteRangeEnd::Relative {
                            source,
                            source_span: span,
                        }
                    }
                    syntax::ByteRangeEnd::Absolute { source, span } => {
                        let source_name = normalized_name(&source);
                        let Some(&source) = source_field_indices.get(&source_name) else {
                            push(errors, Error::new(span, "unknown byte range source field"));
                            continue;
                        };
                        ByteRangeEnd::Absolute {
                            source,
                            source_span: span,
                        }
                    }
                };
                FieldKind::ByteRange { end }
            }
        };
        let raw_name = if mapping.is_some() {
            Some(generated_ident(
                &format!("{generated_stem}_raw"),
                field.name.span(),
                "mapped field raw getter",
                errors,
            )?)
        } else {
            None
        };
        let raw_setter_name = if mapping.is_some() {
            Some(generated_ident(
                &format!("set_{generated_stem}_raw"),
                field.name.span(),
                "mapped field raw setter",
                errors,
            )?)
        } else {
            None
        };
        let is_prefix = matches!(
            &field_kind,
            FieldKind::Codec(codec) if codec.is_prefix()
        );
        let is_byte_range = matches!(&field_kind, FieldKind::ByteRange { .. });
        if kind == syntax::LayoutKind::Absolute && is_prefix {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "prefix codecs are unsupported in absolute layouts",
                ),
            );
        }
        if !is_byte_range {
            register_name(
                &mut variants,
                &error_variant,
                errors,
                "generated field error variant collision",
            );
        }
        let raw_getter = generated_ident(
            &format!("{generated_stem}_raw"),
            field.name.span(),
            "prefix representation getter",
            errors,
        )?;
        if is_prefix {
            register_name(
                &mut getter_names,
                &raw_getter,
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
        let range_mut_name = if is_byte_range {
            Some(generated_ident(
                &format!("{generated_stem}_mut"),
                field.name.span(),
                "range mutable accessor",
                errors,
            )?)
        } else {
            None
        };
        let projections = if is_prefix || is_byte_range {
            if !field.projections.is_empty() {
                let message = if is_byte_range {
                    "byte range fields cannot own bit projections"
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
            mapping,
            raw_name,
            raw_setter_name,
            declaration_index,
            placement,
            placement_span,
            error_variant,
            setter_name,
            range_mut_name,
            raw_getter,
            boundary,
            is_derived_range_source: false,
            projections,
        });
        normalized_field_indices[declaration_index] = Some(normalized_index);
    }

    let pending_ranges: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(range_index, field)| match &field.kind {
            FieldKind::ByteRange {
                end:
                    ByteRangeEnd::Relative {
                        source,
                        source_span,
                    }
                    | ByteRangeEnd::Absolute {
                        source,
                        source_span,
                    },
            } => Some((range_index, *source, *source_span)),
            FieldKind::Codec(_)
            | FieldKind::ByteRange {
                end: ByteRangeEnd::BufEnd,
            } => None,
        })
        .collect();
    let mut source_algebra = HashMap::new();
    for (range_index, source_declaration, source_span) in pending_ranges {
        let Some(Some(source_index)) = normalized_field_indices.get(source_declaration) else {
            continue;
        };
        let source_index = *source_index;
        let source = &fields[source_index];
        if !matches!(source.codec(), Some(Codec::Builtin(_))) {
            push(
                errors,
                Error::new(
                    source_span,
                    "byte range source must be a direct builtin fixed-width integer or a semantic mapping over one",
                ),
            );
            continue;
        }
        if source.placement >= fields[range_index].placement {
            push(
                errors,
                Error::new(
                    source_span,
                    "byte range source field must physically precede the range",
                ),
            );
            continue;
        }
        let algebra = match &fields[range_index].kind {
            FieldKind::ByteRange {
                end: ByteRangeEnd::Relative { .. },
            } => "relative",
            FieldKind::ByteRange {
                end: ByteRangeEnd::Absolute { .. },
            } => "absolute",
            _ => continue,
        };
        let key = source_index;
        if let Some(previous) = source_algebra.insert(key, algebra)
            && previous != algebra
        {
            push(
                errors,
                Error::new(
                    source_span,
                    "a byte range source cannot mix relative and absolute end algebra",
                ),
            );
        }
        if let FieldKind::ByteRange {
            end: ByteRangeEnd::Relative { source, .. } | ByteRangeEnd::Absolute { source, .. },
        } = &mut fields[range_index].kind
        {
            *source = source_index;
        }
        fields[source_index].is_derived_range_source = true;
        fields[source_index].raw_setter_name = None;
    }

    let mut physical_order = Vec::new();
    if kind == syntax::LayoutKind::Sequential {
        let mut physical_placements = HashMap::new();
        for (source_position, entry) in source_physical.into_iter().enumerate() {
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
                syntax::PhysicalEntry::Padding { placement, length } => {
                    let span = placement.span();
                    let Some(position_value) =
                        sequential_position(&placement, source_position, errors)
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
                            Error::new(placement.span(), "position must be one-based (at least 1)"),
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
                syntax::PhysicalEntry::Alignment {
                    placement,
                    boundary,
                } => {
                    let span = placement.span();
                    let Some(position_value) =
                        sequential_position(&placement, source_position, errors)
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
                            Error::new(placement.span(), "position must be one-based (at least 1)"),
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
        let buf_end_ranges: Vec<_> = physical_order
            .iter()
            .filter_map(|item| match item {
                PhysicalItem::Field { index, .. } if fields[*index].is_buf_end_range() => {
                    Some(*index)
                }
                _ => None,
            })
            .collect();
        if buf_end_ranges.len() > 1 {
            for index in buf_end_ranges.iter().skip(1) {
                push(
                    errors,
                    Error::new(
                        fields[*index].name.span(),
                        "at most one `buf_end` byte range is supported",
                    ),
                );
            }
        }
        if let Some(&index) = buf_end_ranges.first()
            && !matches!(physical_order.last(), Some(PhysicalItem::Field { index: last, .. }) if *last == index)
        {
            push(
                errors,
                Error::new(
                    fields[index].placement_span,
                    "byte range fields must be physically terminal",
                ),
            );
        }
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
                syntax::PhysicalEntry::Padding { placement, .. } => push(
                    errors,
                    Error::new(
                        placement.span(),
                        "padding is unsupported in absolute layouts",
                    ),
                ),
                syntax::PhysicalEntry::Alignment { placement, .. } => push(
                    errors,
                    Error::new(
                        placement.span(),
                        "alignment is unsupported in absolute layouts",
                    ),
                ),
            }
        }
    }

    let has_dynamic = fields
        .iter()
        .any(|field| field.is_prefix() || field.is_byte_range());
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
        if field.is_prefix() {
            getters.insert(normalized_name(&field.raw_getter), field.raw_getter.span());
        }
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
    for field in fields.iter().filter_map(|field| field.raw_name.as_ref()) {
        let name = normalized_name(field);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.span(),
                    "mapped raw getter conflicts with an existing getter",
                ),
            );
        }
        getters.insert(name, field.span());
    }
    let mut setters = HashMap::new();
    for field in fields.iter().filter(|field| {
        matches!(field.codec(), Some(codec) if !codec.is_prefix()) && !field.is_derived_range_source
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
    for field in fields
        .iter()
        .filter_map(|field| field.raw_setter_name.as_ref())
    {
        let name = normalized_name(field);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.span(),
                    "mapped raw setter conflicts with an existing getter",
                ),
            );
        }
        if setters.insert(name, field.span()).is_some() {
            push(
                errors,
                Error::new(field.span(), "mapped raw setter collision"),
            );
        }
    }
    let mut range_accessors = HashMap::new();
    for field in fields.iter().filter(|field| field.is_byte_range()) {
        let Some(accessor) = &field.range_mut_name else {
            continue;
        };
        let name = normalized_name(accessor);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated range mutable accessor conflicts with an existing getter",
                ),
            );
        }
        if setters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated range mutable accessor conflicts with an existing setter",
                ),
            );
        }
        if range_accessors.insert(name, field.name.span()).is_some() {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "generated range mutable accessor collision",
                ),
            );
        }
    }
    let mut fluent = HashMap::new();
    for field in fields.iter().filter(|field| !field.is_derived_range_source) {
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
    for field in fields
        .iter()
        .filter(|field| !field.is_derived_range_source)
        .filter_map(|field| field.raw_name.as_ref())
    {
        let name = normalized_name(field);
        if fluent.insert(name, field.span()).is_some() {
            push(
                errors,
                Error::new(field.span(), "generated builder fluent method collision"),
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
    for field in fields.iter().filter_map(|field| field.raw_name.as_ref()) {
        let name = normalized_name(field);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.span(),
                    "mapped raw getter conflicts with an existing getter",
                ),
            );
        }
        getters.insert(name, field.span());
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
    for field in fields
        .iter()
        .filter_map(|field| field.raw_setter_name.as_ref())
    {
        let name = normalized_name(field);
        if getters.contains_key(&name) {
            push(
                errors,
                Error::new(
                    field.span(),
                    "mapped raw setter conflicts with an existing getter",
                ),
            );
        }
        if setters.insert(name, field.span()).is_some() {
            push(
                errors,
                Error::new(field.span(), "mapped raw setter collision"),
            );
        }
    }
    let mut fluent: HashMap<_, _> = fields
        .iter()
        .map(|field| (normalized_name(&field.name), field.name.span()))
        .collect();
    for field in fields.iter().filter_map(|field| field.raw_name.as_ref()) {
        let name = normalized_name(field);
        if fluent.insert(name, field.span()).is_some() {
            push(
                errors,
                Error::new(field.span(), "generated builder fluent method collision"),
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

fn scalar_raw_type(storage: Builtin) -> IntegerType {
    match storage {
        Builtin::U8 => IntegerType::U8,
        Builtin::I8 => IntegerType::I8,
        Builtin::BeU16 | Builtin::LeU16 => IntegerType::U16,
        Builtin::BeI16 | Builtin::LeI16 => IntegerType::I16,
        Builtin::BeU24 | Builtin::LeU24 | Builtin::BeU32 | Builtin::LeU32 => IntegerType::U32,
        Builtin::BeI32 | Builtin::LeI32 => IntegerType::I32,
        Builtin::BeU64 | Builtin::LeU64 => IntegerType::U64,
        Builtin::BeI64 | Builtin::LeI64 => IntegerType::I64,
        Builtin::BeU128 | Builtin::LeU128 => IntegerType::U128,
        Builtin::BeI128 | Builtin::LeI128 => IntegerType::I128,
    }
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
    if names.insert(normalized_name(name), name.span()).is_some() {
        push(errors, Error::new(name.span(), message));
    }
}
fn sequential_position(
    placement: &syntax::Placement,
    source_position: usize,
    errors: &mut Option<Error>,
) -> Option<usize> {
    match placement {
        syntax::Placement::Explicit(value) => parse_placement(value, "position", errors),
        syntax::Placement::Implicit(span) => match source_position.checked_add(1) {
            Some(position) => Some(position),
            None => {
                push(
                    errors,
                    Error::new(*span, "implicit position does not fit in usize"),
                );
                None
            }
        },
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
    fn normalizes_fixed_mappings_with_physical_raw_types() {
        let names = [
            "U8", "I8", "BeU16", "LeU16", "BeI16", "LeI16", "BeU24", "LeU24", "BeU32", "LeU32",
            "BeI32", "LeI32", "BeU64", "LeU64", "BeI64", "LeI64", "BeU128", "LeU128", "BeI128",
            "LeI128",
        ];
        let fields: String = names
            .iter()
            .enumerate()
            .map(|(index, codec)| format!("field f{index}: {codec} as crate::Semantic;"))
            .collect();
        let value = model(&format!(
            "layout H {{ {fields} field bytes: bytes(16) as crate::Address; }}"
        ))
        .unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        let expected = [
            IntegerType::U8,
            IntegerType::I8,
            IntegerType::U16,
            IntegerType::U16,
            IntegerType::I16,
            IntegerType::I16,
            IntegerType::U32,
            IntegerType::U32,
            IntegerType::U32,
            IntegerType::U32,
            IntegerType::I32,
            IntegerType::I32,
            IntegerType::U64,
            IntegerType::U64,
            IntegerType::I64,
            IntegerType::I64,
            IntegerType::U128,
            IntegerType::U128,
            IntegerType::I128,
            IntegerType::I128,
        ];
        for (field, raw) in layout.data.fields.iter().zip(expected) {
            assert!(
                matches!(&field.mapping, Some(Mapping { raw: MappingRaw::Builtin(actual), .. }) if *actual == raw)
            );
            assert!(field.raw_name.is_some());
            assert!(field.raw_setter_name.is_some());
        }
        assert!(matches!(
            layout.data.fields.last().and_then(|field| field.mapping.as_ref()),
            Some(Mapping { raw: MappingRaw::Bytes(16), semantic }) if semantic.segments.len() == 2
        ));
    }

    #[test]
    fn mapping_rejects_nonphysical_fixed_codecs_and_preserves_projections() {
        for (source, needle) in [
            (
                "layout H { field value: crate::Codec as crate::Semantic; }",
                "custom codecs",
            ),
            (
                "scalar Nominal: U8; layout H { field value: Nominal as crate::Semantic; }",
                "declared scalar codecs",
            ),
            (
                "layout H { field value: prefix(crate::Codec) as crate::Semantic; }",
                "prefix codecs",
            ),
            (
                "layout H { field length: U8; field value: bytes(current_pos..current_pos + length) as crate::Semantic; }",
                "byte range fields",
            ),
        ] {
            error(source, needle);
        }

        let value = model(
            "layout H { field flags: U8 as crate::Flags { projections { bit enabled: 0; } } }",
        )
        .unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        assert!(matches!(
            layout.data.fields[0].codec(),
            Some(Codec::Builtin(Builtin::U8))
        ));
        assert_eq!(layout.data.fields[0].projections.len(), 1);
    }

    #[test]
    fn mapped_raw_names_share_getter_setter_and_builder_namespaces() {
        error(
            "layout H { field kind: U8 as crate::Kind; field kind_raw: U8; }",
            "mapped raw getter conflicts",
        );
        error(
            "layout H { field flags: U8 { projections { bit kind_raw: 0; } } field kind: U8 as crate::Kind; }",
            "mapped raw getter conflicts",
        );
        error(
            "layout H { field kind: U8 as crate::Kind; field set_kind_raw: U8; }",
            "mapped raw setter conflicts",
        );
        error(
            "layout H { field r#kind: U8 as crate::Kind; field kind_raw: U8; }",
            "mapped raw getter conflicts",
        );
        let builder_error =
            match model("layout H { field kind: U8 as crate::Kind; field kind_raw: U8; }") {
                Ok(_) => panic!("expected builder namespace error"),
                Err(error) => error.into_compile_error().to_string(),
            };
        assert!(builder_error.contains("generated builder fluent method collision"));
    }

    #[test]
    fn unmapped_fields_keep_no_mapping_or_raw_generated_names() {
        let value = model("layout H { field value: U8; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        let field = &layout.data.fields[0];
        assert!(field.mapping.is_none());
        assert!(field.raw_name.is_none());
        assert!(field.raw_setter_name.is_none());
    }

    #[test]
    fn normalizes_builtin_scalars_direct_paths_and_top_level_collisions() {
        let value = model("layout Frame { field hardware: Hardware; field external: crate::External; } pub scalar Hardware: BeU16; pub scalar Size: LeU24;").unwrap();
        assert!(
            matches!(&value.items[0], Item::Layout(Layout::Sequential(layout)) if
                matches!(layout.data.fields[0].codec(), Some(Codec::Custom(_)))
                    && matches!(layout.data.fields[1].codec(), Some(Codec::Custom(_))))
        );
        assert!(matches!(
            &value.items[1],
            Item::Scalar(Scalar {
                storage: Builtin::BeU16,
                ..
            })
        ));
        assert!(matches!(
            &value.items[2],
            Item::Scalar(Scalar {
                storage: Builtin::LeU24,
                ..
            })
        ));
        for (source, needle) in [
            ("scalar S: bytes(2);", "bytes(N)"),
            ("scalar S: prefix(crate::P);", "prefix(path)"),
            ("scalar S: bytes(current_pos..buf_end);", "byte ranges"),
            ("scalar S: crate::Other;", "custom path"),
            ("scalar S: Unknown;", "supported builtin"),
            (
                "layout Header { field value: U8; } scalar HeaderView: U8;",
                "top-level name collision",
            ),
            (
                "scalar r#Thing: U8; scalar Thing: U8;",
                "top-level name collision",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn absolute_keeps_declaration_order_and_sorts_offsets() {
        let value = model("pub absolute layout Header { field tail: BeU16 { offset: 4; } field kind: U8 { offset: 0; } } layout Tail { field end: U8 { position: 1; } }").unwrap();
        let Item::Layout(Layout::Absolute(header)) = &value.items[0] else {
            panic!("expected absolute")
        };
        assert_eq!(header.data.fields[0].placement, 4);
        assert_eq!(header.data.fields[1].placement, 0);
        assert_eq!(header.offset_order, [1, 0]);
        assert!(matches!(
            &value.items[1],
            Item::Layout(Layout::Sequential(_))
        ));
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

        assert_eq!(value.items.len(), 2);
        let Item::Layout(Layout::Sequential(header)) = &value.items[0] else {
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
        let Item::Layout(Layout::Sequential(sequential)) = &value.items[0] else {
            panic!("expected sequential")
        };
        let Item::Layout(Layout::Absolute(absolute)) = &value.items[1] else {
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
        let Item::Layout(Layout::Sequential(header)) = &value.items[0] else {
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
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
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
            ("", "at least one declaration"),
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
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
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
        let Item::Layout(Layout::Sequential(dynamic)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        assert!(dynamic.has_dynamic);
        let prefix = &dynamic.data.fields[1];
        assert!(matches!(prefix.codec(), Some(Codec::Prefix(_))));
        assert_eq!(prefix.raw_getter.to_string(), "type_raw");
        assert_eq!(prefix.boundary.to_string(), "__wire_end_type");
        let Item::Layout(Layout::Sequential(fixed)) = &value.items[1] else {
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
                "layout H { field r#type: prefix(crate::P) { position: 1; } field type_raw: U8 { position: 2; } }",
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
    fn normalizes_padding_alignment_and_shared_physical_positions() {
        let value = model(
            "layout H { align { position: 3; boundary: 1; } field tail: U8 { position: 4; } padding { position: 2; length: 3; } field head: U8 { position: 1; } }",
        )
        .unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
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
    #[test]
    fn normalizes_implicit_sequential_entries_in_source_order() {
        let value = model(
            "layout H { field head: U8; padding { length: 3; } align { boundary: 8; } field tail: U8; }",
        )
        .unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        assert_eq!(layout.data.fields[0].name, "head");
        assert_eq!(layout.data.fields[0].declaration_index, 0);
        assert_eq!(layout.data.fields[0].placement, 1);
        assert_eq!(layout.data.fields[1].name, "tail");
        assert_eq!(layout.data.fields[1].declaration_index, 1);
        assert_eq!(layout.data.fields[1].placement, 4);
        assert_eq!(
            layout.physical_order,
            [
                PhysicalItem::Field {
                    index: 0,
                    position: 1,
                },
                PhysicalItem::Padding {
                    position: 2,
                    length: 3,
                },
                PhysicalItem::Alignment {
                    position: 3,
                    boundary: 8,
                },
                PhysicalItem::Field {
                    index: 1,
                    position: 4,
                },
            ]
        );
    }

    #[test]
    fn rejects_mixed_sequential_placement_including_spacing() {
        for source in [
            "layout H { field head: U8 { position: 1; } padding { length: 3; } }",
            "layout H { field head: U8; align { position: 2; boundary: 8; } }",
        ] {
            error(
                source,
                "`position` is required for every sequential entry when any position is explicit",
            );
        }
    }

    #[test]
    fn normalizes_byte_ranges_and_validates_sources() {
        let value = model("layout H { field payload: bytes(current_pos..current_pos + length) { position: 2; } field length: U8 as crate::Length { position: 1; } field end: BeI16 { position: 3; } field absolute: bytes(current_pos..end) { position: 4; } }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        assert!(layout.data.fields[1].is_derived_range_source);
        assert!(layout.data.fields[2].is_derived_range_source);
        assert!(matches!(
            layout.data.fields[0].kind,
            FieldKind::ByteRange {
                end: ByteRangeEnd::Relative { source: 1, .. }
            }
        ));
        assert!(matches!(
            layout.data.fields[3].kind,
            FieldKind::ByteRange {
                end: ByteRangeEnd::Absolute { source: 2, .. }
            }
        ));

        for (source, needle) in [
            (
                "layout H { field length: prefix(crate::P) { position: 1; } field payload: bytes(current_pos..current_pos + length) { position: 2; } }",
                "direct builtin",
            ),
            (
                "layout H { field length: bytes(2) { position: 1; } field payload: bytes(current_pos..current_pos + length) { position: 2; } }",
                "direct builtin",
            ),
            (
                "layout H { field payload: bytes(current_pos..current_pos + length) { position: 1; } field length: U8 { position: 2; } }",
                "must physically precede",
            ),
            (
                "layout H { field payload: bytes(buf_start..buf_end); }",
                "start must be `current_pos`",
            ),
            (
                "layout H { field first: bytes(current_pos..buf_end) { position: 1; } field tail: U8 { position: 2; } }",
                "physically terminal",
            ),
            (
                "layout H { field length: U8 { position: 1; } field relative: bytes(current_pos..current_pos + length) { position: 2; } field absolute: bytes(current_pos..length) { position: 3; } }",
                "cannot mix relative and absolute",
            ),
        ] {
            error(source, needle);
        }

        model("layout H { field length: U8 { position: 1; } field first: bytes(current_pos..current_pos + length) { position: 2; } field second: bytes(current_pos..current_pos + length) { position: 3; } }").unwrap();
        error(
            "layout H { field payload: remainder; }",
            "unknown bare codec",
        );
        error(
            "layout H { field payload: region(length); field length: U8; }",
            "expected curly braces",
        );
    }
}
