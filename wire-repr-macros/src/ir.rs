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
    Sequential(Box<SequentialLayout>),
    /// A fixed absolute-offset layout.
    Absolute(Box<AbsoluteLayout>),
}

/// Fields and generated names shared by both layout modes.
pub(crate) struct LayoutData {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    /// Source declaration name, emitted as the immutable layout view.
    pub(crate) layout_name: Ident,
    /// Fluent immutable-view request helper generated for this layout.
    pub(crate) view_request_name: Ident,

    pub(crate) error_name: Ident,
    /// Mutable-view name reserved for sequential rendering.
    pub(crate) view_mut_name: Ident,
    /// Builder name reserved for sequential rendering.
    pub(crate) builder_name: Ident,
    /// Prepared layout-plan name reserved for fixed sequential rendering.
    pub(crate) plan_name: Ident,
    /// Private builder byte-range input representation name.
    pub(crate) range_input_name: Ident,
    /// Mutation-error name reserved for sequential rendering.
    pub(crate) mutation_error_name: Ident,
    /// Builder write-error name reserved for sequential rendering.
    pub(crate) write_error_name: Ident,
    /// Fields in source declaration order.
    pub(crate) fields: Vec<Field>,
    /// Builder-only borrowed inputs used by finalizers.
    pub(crate) contexts: Vec<Context>,
}

/// A sequential layout.
pub(crate) struct SequentialLayout {
    pub(crate) data: LayoutData,
    /// Renderer-ready entries sorted by one-based physical position.
    pub(crate) physical_order: Vec<PhysicalItem>,
    /// Whether at least one field has a runtime-discovered extent.
    pub(crate) has_dynamic: bool,
    /// Explicit derived fields in deterministic dependency order.
    pub(crate) derived_order: Vec<usize>,
    /// Post-write finalizers in deterministic dependency order.
    pub(crate) finalizer_order: Vec<usize>,
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
    /// Builder method selecting an already-existing byte range.
    pub(crate) range_existing_name: Option<Ident>,
    /// Raw-wire getter name used when this is a prefix field.
    pub(crate) raw_getter: Ident,
    /// Stored end-boundary name used when this is a prefix field.
    pub(crate) boundary: Ident,
    /// Range geometry derived from this field, when any range references it.
    pub(crate) range_source: Option<RangeSource>,
    /// Builder-only semantic derivation, normalized after all field kinds are known.
    pub(crate) derivation: Option<Derivation>,
    /// Builder-only post-write finalization, normalized after field geometry is known.
    pub(crate) finalization: Option<Finalization>,
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

    pub(crate) const fn is_derived_range_source(&self) -> bool {
        self.range_source.is_some()
    }
}

/// Fully resolved semantic kind of a named field.
pub(crate) enum FieldKind {
    Codec(Codec),
    ByteRange { end: ByteRangeEnd },
}

/// Validated builder-only derivation for a fixed field.
pub(crate) struct Derivation {
    pub(crate) function: Path,
    pub(crate) error: Path,
    pub(crate) operands: Vec<DeriveOperand>,
}

/// A normalized derivation input.
pub(crate) enum DeriveOperand {
    Value { source: usize, span: Span },
    Len { source: usize, span: Span },
}

/// A builder-only borrowed input retained for future builder rendering.
pub(crate) struct Context {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) name: Ident,
    pub(crate) referent: syn::Type,
    /// Future builder setter name; contexts share the fluent builder namespace.
    pub(crate) setter_name: Ident,
}

/// A normalized infallible post-write finalizer.
pub(crate) struct Finalization {
    pub(crate) function: Path,
    pub(crate) operands: Vec<FinalizeOperand>,
}

/// A renderer-ready finalizer input.
pub(crate) enum FinalizeOperand {
    Bytes {
        start: FinalizeBoundary,
        end: FinalizeBoundary,
    },
    Context {
        source: usize,
        span: Span,
    },
    Value {
        source: usize,
        span: Span,
    },
}

/// Symbolic byte boundary used by a finalizer. `BufEnd` is the final represented extent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FinalizeBoundary {
    BufStart,
    BufEnd,
    FieldStart(usize),
    FieldEnd(usize),
}

enum PendingFinalizeOperand {
    Bytes {
        start: syntax::FinalizeBoundary,
        end: syntax::FinalizeBoundary,
    },
    Context {
        source: Ident,
        span: Span,
    },
    Value {
        source: Ident,
        span: Span,
    },
}

struct PendingFinalization {
    function: Path,
    operands: Vec<PendingFinalizeOperand>,
}

/// Validated byte-range end algebra.
pub(crate) enum ByteRangeEnd {
    Relative { source: usize, source_span: Span },
    Absolute { source: usize, source_span: Span },
    BufEnd,
}

/// How a range source's fixed value becomes byte geometry.
pub(crate) enum RangeSource {
    /// The builtin fixed integer value directly denotes the range geometry.
    Direct,
    /// An adapter converts the builtin fixed integer value to range geometry.
    Transformed { adapter: Path },
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
                    "scalar storage must be a builtin fixed integer codec; `variable(path)` is unsupported",
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
    let layout_name = source.name.clone();
    let view_request_name = generated_ident(
        &(stem_text.clone() + "ViewRequest"),
        source.name.span(),
        "layout view request",
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
    let plan_name = generated_ident(
        &(stem_text.clone() + "Plan"),
        source.name.span(),
        "prepared layout plan",
        errors,
    )?;
    let range_input_name = generated_ident(
        &(stem_text.clone() + "BuilderRangeInput"),
        source.name.span(),
        "builder byte-range input",
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
        &layout_name,
        errors,
        "generated layout name collision",
    );
    register_name(
        generated_layout_names,
        &view_request_name,
        errors,
        "generated layout name collision",
    );
    for name in [
        &error_name,
        &view_mut_name,
        &builder_name,
        &plan_name,
        &range_input_name,
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
    let derive_sources: Vec<_> = source
        .fields
        .iter()
        .map(|field| {
            field.derivation.as_ref().map(|derivation| {
                derivation
                    .operands
                    .iter()
                    .map(|operand| match operand {
                        syntax::DeriveOperand::Value { source, .. }
                        | syntax::DeriveOperand::Len { source, .. } => normalized_name(source),
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let source_contexts = source.contexts;
    let mut pending_finalizations = Vec::with_capacity(source.fields.len());
    let mut fields = Vec::new();
    let mut normalized_field_indices = Vec::new();
    let mut pending_range_sources = Vec::with_capacity(source.fields.len());
    let mut field_names = HashMap::new();
    let mut getter_names = HashMap::new();
    let mut variants = HashMap::new();
    let mut placements = HashMap::new();
    for (declaration_index, field) in source.fields.into_iter().enumerate() {
        normalized_field_indices.push(None);
        let range_source = field.range_source;
        pending_range_sources.push(range_source);
        pending_finalizations.push(field.finalization.map(|finalization| {
            PendingFinalization {
                function: finalization.function,
                operands: finalization
                    .operands
                    .into_iter()
                    .map(|operand| match operand {
                        syntax::FinalizeOperand::Bytes { start, end } => {
                            PendingFinalizeOperand::Bytes { start, end }
                        }
                        syntax::FinalizeOperand::Context { source, span } => {
                            PendingFinalizeOperand::Context { source, span }
                        }
                        syntax::FinalizeOperand::Value { source, span } => {
                            PendingFinalizeOperand::Value { source, span }
                        }
                    })
                    .collect(),
            }
        }));
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
        if matches!(generated_stem.as_str(), "view" | "as_bytes" | "WIDTH") {
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
        if field.derivation.is_some() {
            let derive_variant = generated_ident(
                &format!("Derive{}", error_variant),
                field.name.span(),
                "derived field error variant",
                errors,
            )?;
            register_name(
                &mut variants,
                &derive_variant,
                errors,
                "generated derived field error variant collision",
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
        let range_existing_name = if is_byte_range {
            Some(generated_ident(
                &format!("{generated_stem}_existing"),
                field.name.span(),
                "builder existing byte-range method",
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
            range_existing_name,
            raw_getter,
            boundary,
            range_source: None,
            derivation: field.derivation.map(|derivation| Derivation {
                function: derivation.function,
                error: derivation.error,
                operands: derivation
                    .operands
                    .into_iter()
                    .enumerate()
                    .map(|(operand_index, operand)| match operand {
                        syntax::DeriveOperand::Value { span, .. } => DeriveOperand::Value {
                            source: source_field_indices
                                .get(
                                    &derive_sources[declaration_index]
                                        .as_ref()
                                        .expect("derivation sources")[operand_index],
                                )
                                .copied()
                                .unwrap_or(usize::MAX),
                            span,
                        },
                        syntax::DeriveOperand::Len { span, .. } => DeriveOperand::Len {
                            source: source_field_indices
                                .get(
                                    &derive_sources[declaration_index]
                                        .as_ref()
                                        .expect("derivation sources")[operand_index],
                                )
                                .copied()
                                .unwrap_or(usize::MAX),
                            span,
                        },
                    })
                    .collect(),
            }),
            finalization: None,
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
    for (declaration_index, adapter) in pending_range_sources.iter().enumerate() {
        let Some(adapter) = adapter else {
            continue;
        };
        let span_error = |message| Error::new_spanned(adapter, message);
        let Some(Some(source_index)) = normalized_field_indices.get(declaration_index) else {
            continue;
        };
        let source = &fields[*source_index];
        if kind == syntax::LayoutKind::Absolute {
            push(
                errors,
                span_error("`range_source` is unsupported in absolute layouts"),
            );
        }
        if !matches!(source.codec(), Some(Codec::Builtin(_))) {
            push(
                errors,
                span_error("`range_source` requires a direct builtin integer fixed codec"),
            );
        }
        if source.mapping.is_some() {
            push(
                errors,
                span_error("`range_source` cannot be used with a semantic mapping"),
            );
        }
        if !source.projections.is_empty() {
            push(
                errors,
                span_error("`range_source` cannot be used with bit projections"),
            );
        }
        if source.derivation.is_some() {
            push(
                errors,
                span_error("`range_source` cannot be used with `derive`"),
            );
        }
        if pending_finalizations[declaration_index].is_some() {
            push(
                errors,
                span_error("`range_source` cannot be used with `finalize`"),
            );
        }
    }
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
        if let Some(previous) = source_algebra.insert(source_index, algebra)
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
        if fields[source_index].range_source.is_none() {
            let range_source = match pending_range_sources[source_declaration].take() {
                Some(adapter) => RangeSource::Transformed { adapter },
                None => RangeSource::Direct,
            };
            if matches!(range_source, RangeSource::Transformed { .. }) {
                let variant = generated_ident(
                    &format!("RangeSource{}", fields[source_index].error_variant),
                    fields[source_index].name.span(),
                    "range source error variant",
                    errors,
                )?;
                register_name(
                    &mut variants,
                    &variant,
                    errors,
                    "generated range source error variant collision",
                );
            }
            fields[source_index].range_source = Some(range_source);
        }
        fields[source_index].raw_setter_name = None;
    }
    for adapter in pending_range_sources.iter().flatten() {
        push(
            errors,
            Error::new_spanned(
                adapter,
                "`range_source` requires a `bytes(source)` or `bytes_to(source)` range",
            ),
        );
    }

    // Derivations are semantic builder inputs. Resolve their declared dependencies only
    // after range-source normalization, then order them deterministically by DAG depth.
    for field in &mut fields {
        let Some(derivation) = field.derivation.as_mut() else {
            continue;
        };
        for operand in &mut derivation.operands {
            let source = match operand {
                DeriveOperand::Value { source, .. } | DeriveOperand::Len { source, .. } => source,
            };
            *source = normalized_field_indices
                .get(*source)
                .and_then(|index| *index)
                .unwrap_or(usize::MAX);
        }
    }
    for index in 0..fields.len() {
        let Some(derivation) = fields[index].derivation.as_ref() else {
            continue;
        };
        if kind == syntax::LayoutKind::Absolute
            || fields[index].is_byte_range()
            || fields[index].is_prefix()
            || !fields[index].projections.is_empty()
            || !matches!(
                fields[index].codec(),
                Some(Codec::Builtin(_) | Codec::Custom(_) | Codec::Bytes(_))
            )
        {
            push(
                errors,
                Error::new(
                    fields[index].name.span(),
                    "`derive` is supported only on fixed codec fields in sequential layouts",
                ),
            );
        }
        for operand in &derivation.operands {
            let (source, span, wants_len) = match operand {
                DeriveOperand::Value { source, span } => (*source, *span, false),
                DeriveOperand::Len { source, span } => (*source, *span, true),
            };
            if source == usize::MAX {
                push(errors, Error::new(span, "unknown derived-field dependency"));
                continue;
            }
            if source == index {
                push(
                    errors,
                    Error::new(span, "derived field cannot depend on itself"),
                );
                continue;
            }
            if wants_len && !fields[source].is_byte_range() {
                push(
                    errors,
                    Error::new(span, "`len(...)` requires a byte range field"),
                );
            }
            if !wants_len && fields[source].is_byte_range() {
                push(
                    errors,
                    Error::new(span, "`value(...)` cannot reference a byte range field"),
                );
            }
        }
    }
    for field in &fields {
        if !field.is_derived_range_source() {
            continue;
        }
        if field.derivation.is_some() {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "explicit derived fields cannot be byte range sources",
                ),
            );
        }
    }
    let mut derived_order = Vec::new();
    let mut remaining: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| field.derivation.as_ref().map(|_| index))
        .collect();
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .copied()
            .filter(|candidate| {
                let derivation = fields[*candidate].derivation.as_ref().expect("derived");
                derivation.operands.iter().all(|operand| {
                    let source = match operand {
                        DeriveOperand::Value { source, .. } | DeriveOperand::Len { source, .. } => {
                            *source
                        }
                    };
                    source == usize::MAX
                        || fields
                            .get(source)
                            .is_none_or(|field| field.derivation.is_none())
                        || derived_order.contains(&source)
                })
            })
            .min_by_key(|index| fields[*index].declaration_index);
        let Some(next) = next else {
            for index in remaining {
                push(
                    errors,
                    Error::new(
                        fields[index].name.span(),
                        "cycle in derived field dependencies",
                    ),
                );
            }
            break;
        };
        derived_order.push(next);
        remaining.retain(|index| *index != next);
    }

    let contexts = normalize_contexts(source_contexts, &fields, &pending_finalizations, errors)?;
    let finalizer_order = normalize_finalizers(
        kind,
        pending_finalizations,
        &source_field_indices,
        &normalized_field_indices,
        &contexts,
        &mut fields,
        errors,
    );

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
                    "`remaining_bytes` must be physically terminal",
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

    let has_dynamic = fields.iter().any(|field| {
        field.is_prefix()
            || field.is_byte_range()
            || field.derivation.is_some()
            || field.finalization.is_some()
    }) || !contexts.is_empty();

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
        layout_name,
        view_request_name,

        error_name,
        view_mut_name,
        builder_name,
        plan_name,
        range_input_name,
        mutation_error_name,
        write_error_name,
        fields,
        contexts,
    };
    Some(match kind {
        syntax::LayoutKind::Sequential => Layout::Sequential(Box::new(SequentialLayout {
            data,
            physical_order,
            has_dynamic,
            derived_order,
            finalizer_order,
        })),
        syntax::LayoutKind::Absolute => {
            let mut offset_order: Vec<_> = (0..data.fields.len()).collect();
            offset_order.sort_by_key(|&index| data.fields[index].placement);
            Layout::Absolute(Box::new(AbsoluteLayout { data, offset_order }))
        }
    })
}

fn normalize_contexts(
    source: Vec<syntax::Context>,
    fields: &[Field],
    pending_finalizations: &[Option<PendingFinalization>],
    errors: &mut Option<Error>,
) -> Option<Vec<Context>> {
    let mut names = HashMap::new();
    let mut result = Vec::new();
    for context in source {
        let setter_name = generated_ident(
            &normalized_name(&context.name),
            context.name.span(),
            "context builder setter",
            errors,
        )?;
        if names
            .insert(normalized_name(&context.name), context.name.span())
            .is_some()
        {
            push(
                errors,
                Error::new(context.name.span(), "duplicate context identifier"),
            );
        }
        result.push(Context {
            docs: context.docs,
            name: context.name,
            referent: context.referent,
            setter_name,
        });
    }
    let mut occupied = HashMap::new();
    for field in fields {
        // A finalizer target has no builder setter or fluent input, so it must not
        // reserve those names before the renderer gets a chance to omit them.
        if pending_finalizations
            .get(field.declaration_index)
            .is_some_and(|finalization| finalization.is_some())
        {
            continue;
        }
        occupied.insert(normalized_name(&field.name), field.name.span());
        occupied.insert(normalized_name(&field.setter_name), field.name.span());
        if let Some(raw) = &field.raw_name {
            occupied.insert(normalized_name(raw), raw.span());
        }
        if let Some(raw) = &field.raw_setter_name {
            occupied.insert(normalized_name(raw), raw.span());
        }
        if let Some(existing) = &field.range_existing_name {
            occupied.insert(normalized_name(existing), existing.span());
        }
        if let Some(accessor) = &field.range_mut_name {
            occupied.insert(normalized_name(accessor), accessor.span());
        }
        if field.is_prefix() {
            occupied.insert(normalized_name(&field.raw_getter), field.raw_getter.span());
        }
        for projection in &field.projections {
            occupied.insert(normalized_name(&projection.name), projection.name.span());
        }
    }
    for context in &result {
        let name = normalized_name(&context.setter_name);
        if matches!(name.as_str(), "new" | "prepare" | "build_into") || occupied.contains_key(&name)
        {
            push(
                errors,
                Error::new(
                    context.name.span(),
                    "context name conflicts with a generated builder member",
                ),
            );
        }
    }
    Some(result)
}

fn normalize_finalizers(
    kind: syntax::LayoutKind,
    pending: Vec<Option<PendingFinalization>>,
    source_field_indices: &HashMap<String, usize>,
    normalized_field_indices: &[Option<usize>],
    contexts: &[Context],
    fields: &mut [Field],
    errors: &mut Option<Error>,
) -> Vec<usize> {
    if kind == syntax::LayoutKind::Absolute {
        for context in contexts {
            push(
                errors,
                Error::new(
                    context.name.span(),
                    "`context` is supported only in sequential layouts",
                ),
            );
        }
        for (declaration, finalization) in pending.iter().enumerate() {
            if finalization.is_some() {
                let span = normalized_field_indices
                    .get(declaration)
                    .and_then(|index| *index)
                    .and_then(|index| fields.get(index))
                    .map_or(Span::call_site(), |field| field.name.span());
                push(
                    errors,
                    Error::new(span, "`finalize` is supported only in sequential layouts"),
                );
            }
        }
        return Vec::new();
    }
    let context_indices: HashMap<_, _> = contexts
        .iter()
        .enumerate()
        .map(|(index, context)| (normalized_name(&context.name), index))
        .collect();
    let mut targets = Vec::new();
    for (declaration, finalization) in pending.into_iter().enumerate() {
        let Some(finalization) = finalization else {
            continue;
        };
        let Some(Some(index)) = normalized_field_indices.get(declaration) else {
            continue;
        };
        let index = *index;
        let field = &fields[index];
        if matches!(
            field.codec(),
            Some(Codec::Builtin(Builtin::BeU24 | Builtin::LeU24))
        ) {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "`finalize` requires an infallibly encodable builtin target; U24 requires range validation",
                ),
            );
            continue;
        }
        if field.mapping.is_some()
            || !field.projections.is_empty()
            || field.derivation.is_some()
            || field.is_derived_range_source()
            || !matches!(field.codec(), Some(Codec::Builtin(_)))
        {
            push(
                errors,
                Error::new(
                    field.name.span(),
                    "`finalize` target must be an unmapped direct builtin fixed integer that is not derived, projected, or a byte range source",
                ),
            );
            continue;
        }
        let mut operands = Vec::new();
        for operand in &finalization.operands {
            match operand {
                PendingFinalizeOperand::Context { source, span } => {
                    match context_indices.get(&normalized_name(source)) {
                        Some(source) => operands.push(FinalizeOperand::Context {
                            source: *source,
                            span: *span,
                        }),
                        None => push(errors, Error::new(*span, "unknown finalizer context")),
                    }
                }
                PendingFinalizeOperand::Value { source, span } => {
                    let Some(&source_declaration) =
                        source_field_indices.get(&normalized_name(source))
                    else {
                        push(errors, Error::new(*span, "unknown finalizer value field"));
                        continue;
                    };
                    let Some(Some(source)) = normalized_field_indices.get(source_declaration)
                    else {
                        continue;
                    };
                    if *source == index {
                        push(
                            errors,
                            Error::new(*span, "finalizer cannot consume its own value"),
                        );
                        continue;
                    }
                    if fields[*source].is_byte_range() {
                        push(
                            errors,
                            Error::new(*span, "`value(...)` cannot reference a byte range field"),
                        );
                        continue;
                    }
                    if fields[*source].is_derived_range_source() {
                        push(
                            errors,
                            Error::new(
                                *span,
                                "`value(...)` cannot reference an automatic byte range source",
                            ),
                        );
                        continue;
                    }
                    operands.push(FinalizeOperand::Value {
                        source: *source,
                        span: *span,
                    });
                }
                PendingFinalizeOperand::Bytes { start, end } => {
                    let end_span = span_of_boundary(end);
                    let Some(start) = normalize_finalize_boundary(
                        start,
                        source_field_indices,
                        normalized_field_indices,
                        span_of_boundary(start),
                        errors,
                    ) else {
                        continue;
                    };
                    let Some(end) = normalize_finalize_boundary(
                        end,
                        source_field_indices,
                        normalized_field_indices,
                        span_of_boundary(end),
                        errors,
                    ) else {
                        continue;
                    };
                    if boundary_rank(start, fields) > boundary_rank(end, fields) {
                        push(
                            errors,
                            Error::new(
                                end_span,
                                "finalizer bytes range start must not be after end",
                            ),
                        );
                    }
                    operands.push(FinalizeOperand::Bytes { start, end });
                }
            }
        }
        fields[index].finalization = Some(Finalization {
            function: finalization.function,
            operands,
        });
        targets.push(index);
    }
    for context in contexts {
        let used = fields.iter().filter_map(|field| field.finalization.as_ref()).any(|finalization| {
            finalization.operands.iter().any(|operand| matches!(operand, FinalizeOperand::Context { source, .. } if *source == context_indices[&normalized_name(&context.name)]))
        });
        if !used {
            push(
                errors,
                Error::new(
                    context.name.span(),
                    "context must be referenced by at least one finalizer",
                ),
            );
        }
    }
    let mut order = Vec::new();
    let mut remaining = targets;
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .copied()
            .filter(|candidate| {
                finalizer_dependencies(*candidate, fields)
                    .iter()
                    .all(|source| !remaining.contains(source))
            })
            .min_by_key(|index| fields[*index].declaration_index);
        let Some(next) = next else {
            for index in remaining {
                push(
                    errors,
                    Error::new(fields[index].name.span(), "cycle in finalizer dependencies"),
                );
            }
            break;
        };
        order.push(next);
        remaining.retain(|index| *index != next);
    }
    order
}

fn span_of_boundary(boundary: &syntax::FinalizeBoundary) -> Span {
    match boundary {
        syntax::FinalizeBoundary::BufStart(span) | syntax::FinalizeBoundary::BufEnd(span) => *span,
        syntax::FinalizeBoundary::FieldStart { span, .. }
        | syntax::FinalizeBoundary::FieldEnd { span, .. } => *span,
    }
}

fn normalize_finalize_boundary(
    boundary: &syntax::FinalizeBoundary,
    source_field_indices: &HashMap<String, usize>,
    normalized_field_indices: &[Option<usize>],
    span: Span,
    errors: &mut Option<Error>,
) -> Option<FinalizeBoundary> {
    match boundary {
        syntax::FinalizeBoundary::BufStart(_) => Some(FinalizeBoundary::BufStart),
        syntax::FinalizeBoundary::BufEnd(_) => Some(FinalizeBoundary::BufEnd),
        syntax::FinalizeBoundary::FieldStart { source, .. } => {
            let Some(&declaration) = source_field_indices.get(&normalized_name(source)) else {
                push(
                    errors,
                    Error::new(span, "unknown finalizer bytes boundary field"),
                );
                return None;
            };
            let Some(Some(index)) = normalized_field_indices.get(declaration) else {
                return None;
            };
            Some(FinalizeBoundary::FieldStart(*index))
        }
        syntax::FinalizeBoundary::FieldEnd { source, .. } => {
            let Some(&declaration) = source_field_indices.get(&normalized_name(source)) else {
                push(
                    errors,
                    Error::new(span, "unknown finalizer bytes boundary field"),
                );
                return None;
            };
            let Some(Some(index)) = normalized_field_indices.get(declaration) else {
                return None;
            };
            Some(FinalizeBoundary::FieldEnd(*index))
        }
    }
}

/// Orders finalizer boundaries by physical layout position, with a field's start before its end.
fn boundary_rank(boundary: FinalizeBoundary, fields: &[Field]) -> (usize, u8) {
    match boundary {
        FinalizeBoundary::BufStart => (0, 0),
        FinalizeBoundary::FieldStart(index) => (fields[index].placement, 0),
        FinalizeBoundary::FieldEnd(index) => (fields[index].placement, 1),
        FinalizeBoundary::BufEnd => (usize::MAX, 1),
    }
}

fn finalizer_dependencies(target: usize, fields: &[Field]) -> Vec<usize> {
    let Some(finalization) = fields[target].finalization.as_ref() else {
        return Vec::new();
    };
    let mut dependencies: Vec<_> = finalization
        .operands
        .iter()
        .filter_map(|operand| match operand {
            FinalizeOperand::Value { source, .. }
                if *source != target && fields[*source].finalization.is_some() =>
            {
                Some(*source)
            }
            FinalizeOperand::Bytes { .. }
            | FinalizeOperand::Context { .. }
            | FinalizeOperand::Value { .. } => None,
        })
        .collect();
    dependencies.sort_unstable();
    dependencies.dedup();
    dependencies
}

fn validate_dynamic_layout_namespace(fields: &[Field], errors: &mut Option<Error>) {
    let reserved = [
        "view",
        "as_bytes",
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
        matches!(field.codec(), Some(codec) if !codec.is_prefix())
            && !field.is_derived_range_source()
            && field.derivation.is_none()
            && field.finalization.is_none()
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
        .filter(|field| field.derivation.is_none() && field.finalization.is_none())
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
    for field in fields.iter().filter(|field| {
        !field.is_derived_range_source()
            && field.derivation.is_none()
            && field.finalization.is_none()
    }) {
        let name = normalized_name(&field.name);
        if matches!(name.as_str(), "new" | "prepare" | "build_into") {
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
    for field in fields.iter().filter(|field| {
        !field.is_derived_range_source()
            && field.derivation.is_none()
            && field.finalization.is_none()
    }) {
        if let Some(existing) = &field.range_existing_name {
            let name = normalized_name(existing);
            if fluent.insert(name, field.name.span()).is_some() {
                push(
                    errors,
                    Error::new(
                        field.name.span(),
                        "generated builder existing range method collision",
                    ),
                );
            }
        }
    }
    for field in fields
        .iter()
        .filter(|field| {
            !field.is_derived_range_source()
                && field.derivation.is_none()
                && field.finalization.is_none()
        })
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
        "view",
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
        if matches!(name.as_str(), "new" | "prepare" | "build_into") {
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
            "view" | "as_bytes" | "WIDTH"
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
            .map(|(index, codec)| format!("f{index}: {codec} as crate::Semantic;"))
            .collect();
        let value = model(&format!(
            "layout H {{ {fields} bytes: bytes(16) as crate::Address; }}"
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
                "layout H { value: crate::Codec as crate::Semantic; }",
                "custom codecs",
            ),
            (
                "scalar Nominal: U8; layout H { value: Nominal as crate::Semantic; }",
                "declared scalar codecs",
            ),
            (
                "layout H { value: variable(crate::Codec) as crate::Semantic; }",
                "prefix codecs",
            ),
            (
                "layout H { length: U8; value: bytes(length) as crate::Semantic; }",
                "byte range fields",
            ),
        ] {
            error(source, needle);
        }

        let value =
            model("layout H { flags: U8 as crate::Flags { projections { bit enabled: 0; } }; }")
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
            "layout H { kind: U8 as crate::Kind; kind_raw: U8; }",
            "mapped raw getter conflicts",
        );
        error(
            "layout H { flags: U8 { projections { bit kind_raw: 0; } }; kind: U8 as crate::Kind; }",
            "mapped raw getter conflicts",
        );
        error(
            "layout H { kind: U8 as crate::Kind; set_kind_raw: U8; }",
            "mapped raw setter conflicts",
        );
        error(
            "layout H { r#kind: U8 as crate::Kind; kind_raw: U8; }",
            "mapped raw getter conflicts",
        );
        let builder_error = match model("layout H { kind: U8 as crate::Kind; kind_raw: U8; }") {
            Ok(_) => panic!("expected builder namespace error"),
            Err(error) => error.into_compile_error().to_string(),
        };
        assert!(builder_error.contains("generated builder fluent method collision"));
    }

    #[test]
    fn unmapped_fields_keep_no_mapping_or_raw_generated_names() {
        let value = model("layout H { value: U8; }").unwrap();
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
        let value = model("layout Frame { hardware: Hardware; external: crate::External; } pub scalar Hardware: BeU16; pub scalar Size: LeU24;").unwrap();
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
            ("scalar S: variable(crate::P);", "variable(path)"),
            ("scalar S: remaining_bytes;", "byte ranges"),
            ("scalar S: crate::Other;", "custom path"),
            ("scalar S: Unknown;", "supported builtin"),
            (
                "layout Header { value: U8; } scalar Header: U8;",
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
        let value = model("pub absolute layout Header { tail @ 4: BeU16; kind @ 0: U8; } layout Tail { end @ 1: U8; }").unwrap();
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
        assert!(model("absolute layout H { a @ 0: U8; b @ 4: U8; }").is_ok());
        error(
            "absolute layout H { a @ 0: U8; b @ 0: U8; }",
            "duplicate field offset",
        );
        error("absolute layout H { a @ 1u8: U8; }", "unsuffixed");
        error("absolute layout H { a @ 0x1: U8; }", "base-10");
    }

    #[test]
    fn retains_sequential_declaration_order_docs_and_custom_paths() {
        let value = model(
            "/// header\npub layout Header { #[doc = \"second\"] second @ 2: crate::Code; first @ 1: U8; } layout Tail { end @ 1: LeU16; }",
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
        assert_eq!(header.data.layout_name.to_string(), "Header");
        assert_eq!(
            header.data.view_request_name.to_string(),
            "HeaderViewRequest"
        );
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
            "layout Sequential { bytes @ 1: bytes(16); } absolute layout Absolute { bytes @ 0: bytes(8); }",
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
        let value = model("layout Header { r#type @ 1: U8; }").unwrap();
        let Item::Layout(Layout::Sequential(header)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        let field = &header.data.fields[0];
        assert_eq!(field.name.to_string(), "r#type");
        assert_eq!(field.error_variant.to_string(), "FieldType");

        assert!(model("layout Header { r#parse_prefix @ 1: U8; }").is_ok());
        assert!(model("layout Header { parse_exact @ 1: U8; }").is_ok());
        error("layout Header { view @ 1: U8; }", "reserved generated");
    }

    #[test]
    fn fixed_sequential_names_normalize_and_reject_builder_and_setter_collisions() {
        error("layout H { as_view @ 1: U8; }", "reserved generated");
        error("layout H { new @ 1: U8; }", "generated builder");
        error("layout H { prepare @ 1: U8; }", "generated builder");
        error("layout H { build_into @ 1: U8; }", "generated builder");
        error(
            "layout H { set_value @ 1: U8; value @ 2: U8; }",
            "generated field setter conflicts",
        );
        error(
            "layout H { value @ 1: U8 {  projections { bit set_value: 0; } }; }",
            "generated field setter conflicts",
        );
        error("layout H { r#as_view @ 1: U8; }", "reserved generated");
        error(
            "layout Header { value @ 1: U8; } layout HeaderMutation { value @ 1: U8; }",
            "generated layout name collision",
        );
        error(
            "layout Header { value @ 1: U8; } layout HeaderViewRequest { value @ 1: U8; }",
            "generated layout name collision",
        );
        error(
            "layout Header { value @ 1: U8; } layout HeaderPlan { value @ 1: U8; }",
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
            .map(|(index, name)| format!("f{index} @ {}: {name};", index + 1))
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
            ("layout L { a @ 0: U8; }", "one-based"),
            (
                "layout L { a @ 1: U8; b @ 1: U8; }",
                "duplicate field position",
            ),
            ("layout L { a @ 2: U8; }", "missing position 1"),
            ("layout L { a @ 1u8: U8; }", "unsuffixed"),
            ("layout L { a @ 0x1: U8; }", "base-10"),
            (
                "layout L { a @ 999999999999999999999999999999999999999: U8; }",
                "does not fit",
            ),
            (
                "layout L { a @ 1: U8; a @ 2: U8; }",
                "duplicate field identifier",
            ),
            (
                "layout L { foo_bar @ 1: U8; foo__bar @ 2: U8; }",
                "generated field error variant collision",
            ),
            ("layout L { as_bytes @ 1: U8; }", "reserved generated"),
            ("layout L { WIDTH @ 1: U8; }", "reserved generated"),
            ("layout L { a @ 1: Nope; }", "unknown bare codec"),
            ("layout r#type { a @ 1: U8; }", "raw layout"),
            (
                "layout L { a @ 1: U8; } layout L { b @ 1: U8; }",
                "duplicate layout stem",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn validates_unsigned_projection_storage_ranges_and_namespace() {
        let value = model("layout H { top_flags @ 1: BeU24 {  projections { bit top: 23; } }; all_flags @ 2: BeU24 {  projections { bits all: 0..=23; } }; }").unwrap();
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
            "layout H { flags @ 1: I8 {  projections { bit x: 0; } }; }",
            "unsigned builtin",
        );
        error(
            "layout H { flags @ 1: crate::C {  projections { bit x: 0; } }; }",
            "unsigned builtin",
        );
        error(
            "layout H { bytes @ 1: bytes(16) {  projections { bit x: 0; } }; }",
            "unsigned builtin",
        );
        error(
            "layout H { flags @ 1: U8 {  projections { bit x: 8; } }; }",
            "outside",
        );
        error(
            "layout H { flags @ 1: U8 {  projections { bits x: 4..=3; } }; }",
            "start must not exceed",
        );
        error(
            "layout H { flags @ 1: U8 {  projections { bits x: 1..=3; bit y: 3; } }; }",
            "must not overlap",
        );
        assert!(
            model("layout H { flags @ 1: U8 {  projections { bit parse_exact: 0; } }; }").is_ok()
        );
        assert!(
            model("layout H { flags @ 1: U8 {  projections { bit r#parse_exact: 0; } }; }").is_ok()
        );
        error(
            "layout H { flags @ 1: U8 {  projections { bit x: 0; bit x: 1; } }; }",
            "conflicts",
        );
        error(
            "layout H { first @ 1: U8 {  projections { bit x: 0; } }; second @ 2: U8 {  projections { bit x: 1; } }; }",
            "conflicts",
        );
        error(
            "layout H { flags @ 1: U8 {  projections { bit r#type: 0; } }; r#type @ 2: U8; }",
            "conflicts",
        );
    }

    #[test]
    fn validates_fixed_absolute_write_namespace() {
        for (source, needle) in [
            (
                "absolute layout H { parse_prefix_mut @ 0: U8; }",
                "reserved generated member",
            ),
            (
                "absolute layout H { r#new @ 0: U8; }",
                "generated builder member",
            ),
            (
                "absolute layout H { x @ 0: U8; set_x @ 1: U8; }",
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
            "layout H { tail @ 2: U8; r#type @ 1: variable(crate::P); } layout F { value @ 1: U8; }",
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
                "absolute layout H { value @ 0: variable(crate::P); }",
                "unsupported in absolute",
            ),
            (
                "layout H { value @ 1: variable(crate::P) {  projections { bit x: 0; } }; }",
                "cannot own bit projections",
            ),
            (
                "layout H { r#type @ 1: variable(crate::P); type_raw @ 2: U8; }",
                "generated getter",
            ),
            ("layout H { value @ 1: variable(U8); }", "fixed builtins"),
        ] {
            error(source, needle);
        }
    }
    #[test]
    fn normalizes_padding_alignment_and_shared_physical_positions() {
        let value = model("layout H { align(1) @ 3; tail @ 4: U8; padding(3) @ 2; head @ 1: U8; }")
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
                "layout H { a @ 1: U8; padding(0) @ 2; }",
                "padding length must be nonzero",
            ),
            (
                "layout H { a @ 1: U8; align(0) @ 2; }",
                "alignment boundary must be nonzero",
            ),
            (
                "layout H { a @ 1: U8; padding(2) @ 1; }",
                "duplicate physical position",
            ),
            (
                "layout H { a @ 1: U8; align(4) @ 3; }",
                "missing position 2",
            ),
            (
                "layout H { a @ 1: U8; padding(2) @ 0; }",
                "position must be one-based",
            ),
            (
                "absolute layout H { a @ 0: U8; padding(2) @ 1; }",
                "padding is unsupported in absolute layouts",
            ),
            (
                "absolute layout H { a @ 0: U8; align(4) @ 1; }",
                "alignment is unsupported in absolute layouts",
            ),
            (
                "layout H { padding(2) @ 1; }",
                "layout must contain at least one field",
            ),
        ] {
            error(source, needle);
        }
    }
    #[test]
    fn normalizes_implicit_sequential_entries_in_source_order() {
        let value = model("layout H { head: U8; padding(3); align(8); tail: U8; }").unwrap();
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
            "layout H { head @ 1: U8; padding(3); }",
            "layout H { head: U8; align(8) @ 2; }",
        ] {
            error(
                source,
                "`position` is required for every sequential entry when any position is explicit",
            );
        }
    }

    #[test]
    fn normalizes_byte_ranges_and_validates_sources() {
        let value = model("layout H { payload @ 2: bytes(length); length @ 1: U8 as crate::Length; end @ 3: BeI16; absolute @ 4: bytes_to(end); }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential layout")
        };
        assert!(matches!(
            layout.data.fields[1].range_source,
            Some(RangeSource::Direct)
        ));
        assert!(matches!(
            layout.data.fields[2].range_source,
            Some(RangeSource::Direct)
        ));
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
                "layout H { length @ 1: variable(crate::P); payload @ 2: bytes(length); }",
                "direct builtin",
            ),
            (
                "layout H { length @ 1: bytes(2); payload @ 2: bytes(length); }",
                "direct builtin",
            ),
            (
                "layout H { payload @ 1: bytes(length); length @ 2: U8; }",
                "must physically precede",
            ),
            (
                "layout H { first @ 1: remaining_bytes; tail @ 2: U8; }",
                "`remaining_bytes` must be physically terminal",
            ),
            (
                "layout H { length @ 1: U8; relative @ 2: bytes(length); absolute @ 3: bytes_to(length); }",
                "cannot mix relative and absolute",
            ),
        ] {
            error(source, needle);
        }

        model("layout H { length @ 1: U8; first @ 2: bytes(length); second @ 3: bytes(length); }")
            .unwrap();
        error(
            "layout H { body_existing @ 1: U8; body @ 2: remaining_bytes; }",
            "generated builder existing range method collision",
        );
        error(
            "scalar HBuilderRangeInput: U8; layout H { body: remaining_bytes; }",
            "generated layout name collision",
        );
        error("layout H { payload: remainder; }", "unknown bare codec");
        error(
            "layout H { payload: region(length); length: U8; }",
            "expected curly braces",
        );
    }

    #[test]
    fn normalizes_transformed_range_sources_and_rejects_invalid_owners() {
        let value = model(
            "layout Relative { length: BeU16 { range_source: crate::Length; }; payload: bytes(length); } layout Absolute { end: BeU16 { range_source: crate::Endpoint; }; payload: bytes_to(end); }",
        )
        .unwrap();
        for (item, expected) in value.items.iter().zip(["Length", "Endpoint"]) {
            let Item::Layout(Layout::Sequential(layout)) = item else {
                panic!("expected sequential layout")
            };
            assert!(matches!(
                &layout.data.fields[0].range_source,
                Some(RangeSource::Transformed { adapter }) if adapter.segments.last().is_some_and(|segment| segment.ident == expected)
            ));
        }
        for (source, needle) in [
            (
                "layout H { length: U8 { range_source: crate::Length; }; }",
                "requires a `bytes(source)`",
            ),
            (
                "layout H { length: U8 as crate::Length { range_source: crate::Length; }; payload: bytes(length); }",
                "semantic mapping",
            ),
            (
                "layout H { length: U8 { projections { bit flag: 0; } range_source: crate::Length; }; payload: bytes(length); }",
                "bit projections",
            ),
            (
                "layout H { length: U8 { derive: crate::f(); derive_error: crate::E; range_source: crate::Length; }; payload: bytes(length); }",
                "with `derive`",
            ),
            (
                "layout H { length: U8 { finalize: crate::f(value(length)); range_source: crate::Length; }; payload: bytes(length); }",
                "with `finalize`",
            ),
            (
                "layout H { length: crate::Length { range_source: crate::Length; }; payload: bytes(length); }",
                "direct builtin integer",
            ),
            (
                "absolute layout H { length @ 0: U8 { range_source: crate::Length; }; }",
                "unsupported in absolute",
            ),
            (
                "layout H { length @ 1: U8 { range_source: crate::Length; }; relative @ 2: bytes(length); absolute @ 3: bytes_to(length); }",
                "cannot mix relative and absolute",
            ),
            (
                "layout H { payload @ 1: bytes(length); length @ 2: U8 { range_source: crate::Length; }; }",
                "must physically precede",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn normalizes_explicit_derived_fields_and_rejects_bad_dependencies() {
        let value = model("layout H { later @ 4: U8 {  derive: crate::later(value(total)); derive_error: crate::E; }; count @ 1: U8; bytes @ 2: bytes(count); total @ 3: U8 {  derive: crate::total(len(bytes)); derive_error: crate::E; }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        assert_eq!(layout.derived_order, vec![3, 0]);
        for (source, needle) in [
            (
                "layout H { total @ 1: U8 {  derive: crate::f(value(missing)); derive_error: crate::E; }; }",
                "unknown derived-field dependency",
            ),
            (
                "layout H { total @ 1: U8 {  derive: crate::f(value(total)); derive_error: crate::E; }; }",
                "cannot depend on itself",
            ),
            (
                "layout H { n @ 1: U8; body @ 2: bytes(n); total @ 3: U8 {  derive: crate::f(value(body)); derive_error: crate::E; }; }",
                "cannot reference a byte range",
            ),
            (
                "layout H { n @ 1: U8; total @ 2: U8 {  derive: crate::f(len(n)); derive_error: crate::E; }; }",
                "requires a byte range",
            ),
            (
                "layout H { a @ 1: U8 {  derive: crate::a(value(b)); derive_error: crate::E; }; b @ 2: U8 {  derive: crate::b(value(a)); derive_error: crate::E; }; }",
                "cycle in derived field dependencies",
            ),
            (
                "layout H { total @ 1: U8 {  derive: crate::f(); }; }",
                "requires `derive_error`",
            ),
            (
                "layout H { total @ 1: U8 {  derive_error: crate::E; }; }",
                "requires `derive`",
            ),
        ] {
            error(source, needle);
        }
    }

    #[test]
    fn normalizes_semantic_finalizer_value_sources() {
        let value = model("layout H { target @ 1: U8 {  finalize: crate::finish(value(plain), value(mapped), value(derived), value(finalized)); }; plain @ 2: U8; mapped @ 3: U8 as crate::Mapped; derived @ 4: U8 {  derive: crate::derive(value(plain)); derive_error: crate::E; }; finalized @ 5: U8 {  finalize: crate::finalize(bytes(finalized.start..finalized.end)); }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        let operands = &layout.data.fields[0]
            .finalization
            .as_ref()
            .unwrap()
            .operands;
        assert!(matches!(
            operands[0],
            FinalizeOperand::Value { source: 1, .. }
        ));
        assert!(matches!(
            operands[1],
            FinalizeOperand::Value { source: 2, .. }
        ));
        assert!(matches!(
            operands[2],
            FinalizeOperand::Value { source: 3, .. }
        ));
        assert!(matches!(
            operands[3],
            FinalizeOperand::Value { source: 4, .. }
        ));
        assert_eq!(layout.finalizer_order, vec![4, 0]);
    }

    #[test]
    fn normalizes_contexts_finalizers_and_operands() {
        let value = model("layout H { /// borrowed input\n context seed: crate::Seed; sum @ 2: BeU16 {  finalize: crate::finish(context(seed), value(check), bytes(buf_start..check.end)); }; check @ 1: U8 {  finalize: crate::check(bytes(check.start..check.end)); }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        assert!(layout.has_dynamic);
        assert_eq!(layout.data.contexts.len(), 1);
        assert_eq!(layout.data.contexts[0].name, "seed");
        assert_eq!(layout.data.contexts[0].setter_name, "seed");
        assert_eq!(layout.finalizer_order, vec![1, 0]);
        let finalization = layout.data.fields[0].finalization.as_ref().unwrap();
        assert_eq!(finalization.function.segments.len(), 2);
        assert!(matches!(
            finalization.operands[0],
            FinalizeOperand::Context { source: 0, .. }
        ));
        assert!(matches!(
            finalization.operands[1],
            FinalizeOperand::Value { source: 1, .. }
        ));
        assert!(matches!(
            finalization.operands[2],
            FinalizeOperand::Bytes {
                start: FinalizeBoundary::BufStart,
                end: FinalizeBoundary::FieldEnd(1)
            }
        ));
    }

    #[test]
    fn validates_context_finalizer_and_target_rules() {
        for (source, needle) in [
            (
                "layout H { context unused: crate::U; sum: U8 { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "context must be referenced",
            ),
            (
                "layout H { context x: crate::X; context x: crate::Y; sum: U8 { finalize: crate::f(context(x)); }; }",
                "duplicate context",
            ),
            (
                "layout H { context sum: crate::S; plain: U8; total: U8 { finalize: crate::f(context(sum)); }; }",
                "",
            ),
            (
                "layout H { context plain: crate::S; plain: U8; total: U8 { finalize: crate::f(context(plain)); }; }",
                "context name conflicts",
            ),
            (
                "layout H { context body_existing: crate::S; body: remaining_bytes; total: U8 { finalize: crate::f(context(body_existing)); }; }",
                "context name conflicts",
            ),
            (
                "layout H { sum: U8 { finalize: crate::f(context(missing)); }; }",
                "unknown finalizer context",
            ),
            (
                "layout H { context x: crate::X; sum: U8 { finalize: crate::f(value(x)); }; }",
                "unknown finalizer value field",
            ),
            (
                "layout H { sum: U8 { finalize: crate::f(value(sum)); }; }",
                "cannot consume its own value",
            ),
            (
                "absolute layout H { context x: crate::X; sum @ 0: U8 {  finalize: crate::f(context(x)); }; }",
                "only in sequential",
            ),
            (
                "layout H { sum: BeU24 { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "U24 requires range validation",
            ),
            (
                "layout H { sum: LeU24 { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "U24 requires range validation",
            ),
            (
                "layout H { sum: crate::C { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
            (
                "scalar N: U8; layout H { sum: N { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
            (
                "layout H { sum: bytes(1) { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
            (
                "layout H { sum: variable(crate::P) { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
            (
                "layout H { sum: U8 as crate::S { finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
            (
                "layout H { sum: U8 { projections { bit x: 0; } finalize: crate::f(bytes(buf_start..buf_end)); }; }",
                "unmapped direct builtin",
            ),
        ] {
            if needle.is_empty() {
                assert!(model(source).is_ok(), "{source}");
            } else {
                error(source, needle);
            }
        }
        error(
            "layout H { length: U8 { finalize: crate::f(bytes(buf_start..buf_end)); }; body: bytes(length); }",
            "unmapped direct builtin",
        );
        error(
            "layout H { length: U8; body: bytes(length); sum: U8 { finalize: crate::f(value(body)); }; }",
            "cannot reference a byte range field",
        );
        error(
            "layout H { length: U8; body: bytes(length); sum: U8 { finalize: crate::f(value(length)); }; }",
            "cannot reference an automatic byte range source",
        );
    }

    #[test]
    fn orders_finalizers_only_by_explicit_value_dependencies() {
        let value = model("layout H { b @ 1: U8 {  finalize: crate::b(value(a)); }; a @ 2: U8 {  finalize: crate::a(bytes(a.start..a.end)); }; c @ 3: U8 {  finalize: crate::c(bytes(a.start..a.end)); }; d @ 4: U8 {  finalize: crate::d(bytes(d.start..d.end)); }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        // Only value(a) orders a before b. Byte observation, including a target's
        // own bytes, does not infer dependencies.
        assert_eq!(layout.finalizer_order, vec![1, 0, 2, 3]);

        let value = model("layout H { a: U8 { finalize: crate::a(bytes(b.start..b.end)); }; b: U8 { finalize: crate::b(bytes(a.start..a.end)); }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        assert_eq!(layout.finalizer_order, vec![0, 1]);

        let value = model("layout H { first: U8 { finalize: crate::first(value(plain), value(derived)); }; plain: U8; derived: U8 { derive: crate::derive(value(plain)); derive_error: crate::E; }; second: U8 { finalize: crate::second(bytes(second.start..second.end)); }; }").unwrap();
        let Item::Layout(Layout::Sequential(layout)) = &value.items[0] else {
            panic!("expected sequential");
        };
        assert_eq!(layout.finalizer_order, vec![0, 3]);

        error(
            "layout H { a: U8 { finalize: crate::a(value(b)); }; b: U8 { finalize: crate::b(value(a)); }; }",
            "cycle in finalizer dependencies",
        );

        error(
            "layout H { a: U8 { finalize: crate::a(bytes(a.end..a.start)); }; }",
            "start must not be after end",
        );
    }
}
