//! Resolved struct preparation policy.

use syn::{Ident, Lifetime, Type};

use super::codec::{is_borrowed_byte_slice, is_unsigned_integer};
use super::{Codec, Field, FieldKind, FieldPosition};

pub(in crate::derive) struct Computation {
    /// Raw semantic value encoded for this computed field.
    pub(in crate::derive) value_ty: Type,
    pub(in crate::derive) callback: syn::Path,
    /// Ordered callback arguments and their physical read sets.
    pub(in crate::derive) arguments: Vec<ComputationArgument>,
    /// Whether preparing this computation requires the representation's physical geometry.
    pub(in crate::derive) requires_geometry: bool,
}

pub(in crate::derive) enum ComputationArgument {
    Semantic { name: Ident, index: usize },
    Bytes(ComputationByteSelection),
}

pub(in crate::derive) enum ComputationByteSelection {
    Include(Vec<ComputationFieldPath>),
    Exclude(Vec<ComputationFieldPath>),
}

pub(in crate::derive) struct ComputationFieldPath {
    pub(in crate::derive) top_level: Ident,
    pub(in crate::derive) top_level_index: usize,
    pub(in crate::derive) nested: Vec<Ident>,
}

pub(in crate::derive) struct StructPreparation {
    pub(in crate::derive) computation_order: Vec<usize>,
    pub(in crate::derive) controlled_by: Vec<Option<usize>>,
    pub(in crate::derive) position_sources: Vec<bool>,
}

pub(in crate::derive) fn validate_computations(fields: &mut [Field]) -> syn::Result<Vec<usize>> {
    for index in 0..fields.len() {
        let Some(mut computation) = fields[index].computation.take() else {
            continue;
        };
        for argument in &mut computation.arguments {
            match argument {
                ComputationArgument::Semantic {
                    name,
                    index: argument_index,
                } => {
                    let resolved = fields
                        .iter()
                        .position(|field| field.name == *name)
                        .ok_or_else(|| {
                            syn::Error::new_spanned(
                                &*name,
                                "computed semantic argument must name a field in the same struct",
                            )
                        })?;
                    if resolved == index || fields[resolved].computation.is_some() {
                        return Err(syn::Error::new_spanned(
                            name,
                            "computed semantic arguments cannot name computed fields",
                        ));
                    }
                    if fields[resolved].operation_input.is_some() {
                        return Err(syn::Error::new_spanned(
                            name,
                            "computed semantic arguments cannot name operation-table fields",
                        ));
                    }
                    *argument_index = resolved;
                }
                ComputationArgument::Bytes(selection) => {
                    validate_computation_selection(selection, index, fields)?;
                }
            }
        }
        fields[index].computation = Some(computation);
    }

    let order = computation_order(fields)?;
    let has_geometry = fields.iter().any(|field| {
        field.position.is_some() || field.padding_before != 0 || field.alignment_before.is_some()
    });
    for &index in &order {
        let computation = fields[index].computation.as_ref().expect("computed index");
        let requires_geometry =
            computation.arguments.iter().any(|argument| {
                matches!(
                    argument,
                    ComputationArgument::Bytes(ComputationByteSelection::Exclude(_))
                ) && has_geometry
            }) || computation_dependencies(computation, &order, index).any(|dependency| {
                fields[dependency]
                    .computation
                    .as_ref()
                    .expect("computed dependency")
                    .requires_geometry
            });
        fields[index]
            .computation
            .as_mut()
            .expect("computed index")
            .requires_geometry = requires_geometry;
    }
    Ok(order)
}

fn validate_computation_selection(
    selection: &mut ComputationByteSelection,
    own_index: usize,
    fields: &[Field],
) -> syn::Result<()> {
    let is_include = matches!(selection, ComputationByteSelection::Include(_));
    let paths = match selection {
        ComputationByteSelection::Include(paths) | ComputationByteSelection::Exclude(paths) => {
            paths
        }
    };
    for path in paths.iter_mut() {
        let top_level_index = if path.top_level == "self" {
            own_index
        } else {
            fields
                .iter()
                .position(|field| field.name == path.top_level)
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        &path.top_level,
                        "computed byte selection must name a field in the same struct",
                    )
                })?
        };
        if is_include && top_level_index == own_index {
            return Err(syn::Error::new_spanned(
                &path.top_level,
                "computed byte selections cannot include the computed field itself",
            ));
        }
        if !path.nested.is_empty() && !matches!(fields[top_level_index].kind, FieldKind::Nested) {
            return Err(syn::Error::new_spanned(
                &path.top_level,
                "nested computed byte paths require a nested wire field",
            ));
        }
        path.top_level_index = top_level_index;
    }
    Ok(())
}

fn computation_order(fields: &[Field]) -> syn::Result<Vec<usize>> {
    let computed: Vec<_> = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| field.computation.as_ref().map(|_| index))
        .collect();
    let mut remaining = computed.clone();
    let mut ordered = Vec::with_capacity(computed.len());

    while !remaining.is_empty() {
        let before = remaining.len();
        remaining.retain(|&index| {
            let computation = fields[index].computation.as_ref().expect("computed index");
            let ready = computation_dependencies(computation, &computed, index)
                .all(|dependency| ordered.contains(&dependency));
            if ready {
                ordered.push(index);
                false
            } else {
                true
            }
        });
        if remaining.len() == before {
            return Err(syn::Error::new_spanned(
                &fields[remaining[0]].name,
                "computed byte selections form a dependency cycle",
            ));
        }
    }
    Ok(ordered)
}

fn computation_dependencies<'a>(
    computation: &'a Computation,
    computed: &'a [usize],
    own_index: usize,
) -> impl Iterator<Item = usize> + 'a {
    computed.iter().copied().filter(move |&candidate| {
        candidate != own_index
            && computation.arguments.iter().any(|argument| match argument {
                ComputationArgument::Semantic { .. } => false,
                ComputationArgument::Bytes(ComputationByteSelection::Include(paths)) => {
                    paths.iter().any(|path| path.top_level_index == candidate)
                }
                ComputationArgument::Bytes(ComputationByteSelection::Exclude(paths)) => !paths
                    .iter()
                    .any(|path| path.top_level_index == candidate && path.nested.is_empty()),
            })
    })
}

fn is_unsigned_builtin_codec(codec: &str) -> bool {
    matches!(
        codec,
        "U8" | "BeU16"
            | "LeU16"
            | "BeU24"
            | "LeU24"
            | "BeU32"
            | "LeU32"
            | "BeU64"
            | "LeU64"
            | "BeU128"
            | "LeU128"
    )
}

pub(in crate::derive) fn validate_byte_fields(
    fields: &[Field],
    wire_lifetime: Option<&Lifetime>,
) -> syn::Result<Vec<Option<usize>>> {
    let mut controlled_by = vec![None; fields.len()];

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
        if fields[source_index].computation.is_some() {
            return Err(syn::Error::new_spanned(
                &source,
                "a computed field cannot be a byte length source because `bytes` owns framing geometry",
            ));
        }
        let valid_source = match &fields[source_index].kind {
            FieldKind::Fixed(Codec::Builtin(codec)) => is_unsigned_builtin_codec(codec),
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
        if controlled_by[source_index].is_some() {
            return Err(syn::Error::new_spanned(
                &source,
                "a byte length source may control only one field",
            ));
        }
        controlled_by[source_index] = Some(index);
    }
    Ok(controlled_by)
}

pub(in crate::derive) fn validate_positions(
    fields: &mut [Field],
    controlled_by: &[Option<usize>],
) -> syn::Result<Vec<bool>> {
    let mut position_sources = vec![false; fields.len()];

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
        if !is_unsigned_builtin_codec(codec) {
            return Err(syn::Error::new_spanned(
                &source,
                "position source must be a built-in unsigned integer field",
            ));
        }
        if fields[source_index]
            .computation
            .as_ref()
            .is_some_and(|computation| computation.requires_geometry)
        {
            return Err(syn::Error::new_spanned(
                &source,
                "a computed position source cannot depend on physical geometry that its position controls",
            ));
        }
        if controlled_by[source_index].is_some() || position_sources[source_index] {
            return Err(syn::Error::new_spanned(
                &source,
                "a geometry source may control only one field",
            ));
        }
        position_sources[source_index] = true;
        fields[index].position = Some(FieldPosition::Source(source));
    }
    Ok(position_sources)
}
