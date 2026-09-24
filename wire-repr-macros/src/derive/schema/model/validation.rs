use std::collections::BTreeSet;

use syn::{Expr, Ident, Member};

use super::controller::validate_unsigned_controller;
use super::{
    DynamicExtent, Field, FieldKind, NestedField, Position, RawBytes, SizeTerm, ValueType,
};

pub(super) fn validate(fields: &[Field]) -> syn::Result<Vec<usize>> {
    for field in fields {
        if field.kind.computed().is_none() {
            continue;
        }
        if field.layout.condition.is_some() {
            return Err(syn::Error::new_spanned(
                &field.name,
                "computed destinations cannot be conditional",
            ));
        }
        if field.layout.pad_before.is_some()
            || field.layout.align_before.is_some()
            || field.layout.position.is_some()
        {
            return Err(syn::Error::new_spanned(
                &field.name,
                "computed destinations cannot declare placement geometry",
            ));
        }
        if field
            .offset
            .terms
            .iter()
            .any(|term| matches!(term, SizeTerm::Dynamic))
        {
            return Err(syn::Error::new_spanned(
                &field.name,
                "computed destinations must have a fixed offset before demand geometry",
            ));
        }
    }
    validate_conditions(fields)?;

    validate_geometry_controllers(fields)?;
    validate_arrays(fields)?;
    validate_bit_controller_roles(fields)?;

    validate_computed_controller_roles(fields)?;
    let computed_order = validate_computed_dependencies(fields)?;
    Ok(computed_order)
}

fn validate_computed_controller_roles(fields: &[Field]) -> syn::Result<()> {
    for computed in fields
        .iter()
        .filter(|field| field.kind.computed().is_some())
    {
        let name = &computed.name;
        let controls_geometry = fields.iter().any(|field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == name,
            FieldKind::Array(array) => &array.controller == name,
            FieldKind::Flag(flag) => &flag.controller == name,
            FieldKind::BitProjection(projection) => &projection.controller == name,
            FieldKind::ScalarArray(_)
            | FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Recursive(_)
            | FieldKind::Nested(_) => false,
        }) || fields.iter().any(|field| {
            matches!(
                &field.layout.position,
                Some(Position::Field(controller)) if controller == name
            )
        });
        if controls_geometry {
            return Err(syn::Error::new_spanned(
                name,
                "computed fields cannot control representation geometry",
            ));
        }
    }
    Ok(())
}

fn validate_computed_dependencies(fields: &[Field]) -> syn::Result<Vec<usize>> {
    let computed = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| field.kind.computed().map(|_| index))
        .collect::<Vec<_>>();
    let computed_set = computed.iter().copied().collect::<BTreeSet<_>>();
    let mut dependencies = Vec::with_capacity(computed.len());
    for index in &computed {
        let field = &fields[*index];
        let expression = &field.kind.computed().expect("computed field").expression;
        let names = computed_dependency_names(expression, &field.name, fields)?;
        let deps = names
            .into_iter()
            .filter_map(|name| fields.iter().position(|candidate| candidate.name == name))
            .filter(|dependency| computed_set.contains(dependency))
            .collect::<BTreeSet<_>>();
        dependencies.push((*index, deps));
    }
    let mut remaining = computed_set;
    let mut ordered = Vec::with_capacity(computed.len());
    while !remaining.is_empty() {
        let Some(next) = computed.iter().copied().find(|candidate| {
            remaining.contains(candidate)
                && dependencies
                    .iter()
                    .find(|(index, _)| index == candidate)
                    .is_some_and(|(_, deps)| deps.is_disjoint(&remaining))
        }) else {
            let field = &fields[*remaining.iter().next().expect("nonempty cycle")];
            return Err(syn::Error::new_spanned(
                &field.name,
                "computed field dependency cycle",
            ));
        };
        remaining.remove(&next);
        ordered.push(next);
    }
    Ok(ordered)
}

fn computed_dependency_names(
    expression: &Expr,
    destination: &Ident,
    fields: &[Field],
) -> syn::Result<Vec<Ident>> {
    let Expr::Call(callback) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed callback must be a call",
        ));
    };
    let mut names = Vec::new();
    for argument in &callback.args {
        match argument {
            Expr::Path(path) => {
                let name = simple_computed_name(path)?;
                names.push(if name == "self" {
                    destination.clone()
                } else {
                    name.clone()
                });
            }
            Expr::Call(selection) => {
                let Expr::Path(operation) = selection.func.as_ref() else {
                    return Err(syn::Error::new_spanned(
                        &selection.func,
                        "selection operation must be include or exclude",
                    ));
                };
                let operation = simple_computed_name(operation)?;
                let selected = selection
                    .args
                    .iter()
                    .map(|argument| {
                        fn root(expression: &Expr) -> syn::Result<(Ident, bool)> {
                            match expression {
                                Expr::Path(path) => Ok((simple_computed_name(path)?.clone(), true)),
                                Expr::Field(field) => {
                                    if !matches!(&field.member, Member::Named(_)) {
                                        return Err(syn::Error::new_spanned(
                                            &field.member,
                                            "selection paths require named fields",
                                        ));
                                    }
                                    let (root, _) = root(&field.base)?;
                                    Ok((root, false))
                                }
                                _ => Err(syn::Error::new_spanned(
                                    expression,
                                    "selection fields must be physical field paths",
                                )),
                            }
                        }
                        let (name, whole) = root(argument)?;
                        let name = if name == "self" {
                            destination.clone()
                        } else {
                            name
                        };
                        Ok((name, whole))
                    })
                    .collect::<syn::Result<Vec<_>>>()?;
                if operation == "include" {
                    names.extend(selected.into_iter().map(|(name, _)| name));
                } else if operation == "exclude" {
                    let wholly_excluded = selected
                        .into_iter()
                        .filter_map(|(name, whole)| whole.then_some(name))
                        .collect::<BTreeSet<_>>();
                    names.extend(
                        fields
                            .iter()
                            .filter(|field| !wholly_excluded.contains(&field.name))
                            .map(|field| field.name.clone()),
                    );
                } else {
                    return Err(syn::Error::new_spanned(
                        operation,
                        "selection operation must be include or exclude",
                    ));
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    argument,
                    "computed arguments must be logical fields or selections",
                ));
            }
        }
    }
    for name in &names {
        if !fields.iter().any(|field| field.name == *name) {
            return Err(syn::Error::new_spanned(
                name,
                "computed callback references an unknown field",
            ));
        }
    }
    Ok(names)
}

fn simple_computed_name(path: &syn::ExprPath) -> syn::Result<&Ident> {
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            path,
            "expected a simple field name",
        ));
    }
    Ok(&path.path.segments[0].ident)
}

fn validate_geometry_controllers(fields: &[Field]) -> syn::Result<()> {
    let length_roles = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => Some((index, controller)),
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::ScalarArray(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let mut length_controllers = BTreeSet::new();
    for (index, controller) in &length_roles {
        length_controllers.insert(controller.to_string());
        validate_unsigned_controller(fields, *index, controller, "byte length", false)?;
    }
    for (index, field) in fields.iter().enumerate() {
        if let Some(Position::Field(controller)) = &field.layout.position {
            if length_controllers.contains(&controller.to_string()) {
                return Err(syn::Error::new_spanned(
                    controller,
                    format!(
                        "controller `{controller}` cannot control both byte length and field position before the controller DAG ships"
                    ),
                ));
            }
            validate_unsigned_controller(fields, index, controller, "field position", false)?;
        }
    }
    Ok(())
}

fn validate_arrays(fields: &[Field]) -> syn::Result<()> {
    let mut controllers = BTreeSet::new();
    for (index, field) in fields.iter().enumerate() {
        let FieldKind::Array(array) = &field.kind else {
            continue;
        };
        if !controllers.insert(array.controller.to_string()) {
            return Err(syn::Error::new_spanned(
                &array.controller,
                format!(
                    "item count controller `{}` cannot control multiple arrays",
                    array.controller
                ),
            ));
        }
        if field.layout.condition.is_some() {
            return Err(syn::Error::new_spanned(
                &field.name,
                "runtime arrays cannot be conditional in this vertical",
            ));
        }
        let shared_role = fields.iter().any(|candidate| match &candidate.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(controller),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(controller),
                ..
            }) => controller == &array.controller,
            FieldKind::Flag(flag) => flag.controller == array.controller,
            FieldKind::BitProjection(projection) => projection.controller == array.controller,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::ScalarArray(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Nested(_) => false,
        });
        let placement_role = fields.iter().any(|candidate| {
            matches!(
                &candidate.layout.position,
                Some(Position::Field(controller)) if controller == &array.controller
            )
        });
        if shared_role || placement_role {
            return Err(syn::Error::new_spanned(
                &array.controller,
                "item count controller cannot control another dependency role",
            ));
        }

        validate_unsigned_controller(fields, index, &array.controller, "item count", true)?;
    }
    Ok(())
}

fn validate_bit_controller_roles(fields: &[Field]) -> syn::Result<()> {
    let controllers = fields
        .iter()
        .filter_map(|field| {
            let FieldKind::BitProjection(projection) = &field.kind else {
                return None;
            };
            Some(projection.controller.clone())
        })
        .collect::<BTreeSet<_>>();
    for controller in controllers {
        let shared = fields.iter().any(|field| match &field.kind {
            FieldKind::RawBytes(RawBytes {
                extent: DynamicExtent::Bounded(other),
            })
            | FieldKind::Nested(NestedField {
                extent: Some(other),
                ..
            }) => *other == controller,
            FieldKind::Array(array) => array.controller == controller,
            FieldKind::Flag(flag) => flag.controller == controller,
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::ScalarArray(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Recursive(_)
            | FieldKind::BitProjection(_)
            | FieldKind::Nested(_) => false,
        }) || fields.iter().any(|field| {
            matches!(
                &field.layout.position,
                Some(Position::Field(other)) if *other == controller
            )
        });
        if shared {
            return Err(syn::Error::new_spanned(
                controller,
                "bit projection controller cannot control another dependency role",
            ));
        }
    }
    Ok(())
}

fn validate_conditions(fields: &[Field]) -> syn::Result<()> {
    let mut controllers = BTreeSet::new();
    for (index, field) in fields.iter().enumerate() {
        if let FieldKind::Flag(flag) = &field.kind {
            if !controllers.insert(flag.controller.to_string()) {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    format!(
                        "presence controller `{}` cannot define multiple logical groups",
                        flag.controller
                    ),
                ));
            }
            let Some(controller_index) = fields
                .iter()
                .position(|candidate| candidate.name == flag.controller)
            else {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller is not a schema field",
                ));
            };
            if controller_index >= index {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be physically earlier",
                ));
            }
            let FieldKind::Scalar(scalar) = &fields[controller_index].kind else {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be a bool scalar",
                ));
            };
            if !matches!(&scalar.value_type, ValueType::Bool) || scalar.constant.is_some() {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "logical flag controller must be a nonconstant bool scalar",
                ));
            }
            let controller_field = &fields[controller_index];
            if controller_field.layout.pad_before.is_some()
                || controller_field.layout.align_before.is_some()
                || controller_field.layout.position.is_some()
                || controller_field.layout.condition.is_some()
                || controller_field
                    .offset
                    .terms
                    .iter()
                    .any(|term| matches!(term, SizeTerm::Dynamic))
            {
                return Err(syn::Error::new_spanned(
                    &flag.controller,
                    "presence controller must have fixed sequential geometry",
                ));
            }
        }
        if let Some(condition) = &field.layout.condition {
            let Some(flag_index) = fields
                .iter()
                .position(|candidate| candidate.name == *condition)
            else {
                return Err(syn::Error::new_spanned(
                    condition,
                    "condition does not name a logical flag field",
                ));
            };
            if flag_index >= index || !matches!(fields[flag_index].kind, FieldKind::Flag(_)) {
                return Err(syn::Error::new_spanned(
                    condition,
                    "condition must name a physically earlier logical flag field",
                ));
            }
            if field.layout.pad_before.is_some()
                || field.layout.align_before.is_some()
                || field.layout.position.is_some()
            {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "conditional dependent fields cannot declare independent geometry",
                ));
            }
            let FieldKind::Scalar(scalar) = &field.kind else {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "this conditional-group vertical currently requires scalar dependent fields",
                ));
            };
            if scalar.constant.is_some() || scalar.value_type.is_converted() {
                return Err(syn::Error::new_spanned(
                    &field.name,
                    "conditional dependent scalars must have direct nonconstant representations",
                ));
            }
        }
    }
    for flag in fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Flag(_)))
    {
        let flag_index = fields
            .iter()
            .position(|field| field.name == flag.name)
            .expect("flag belongs to schema");
        let dependent_indices = fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                (field.layout.condition.as_ref() == Some(&flag.name)).then_some(index)
            })
            .collect::<Vec<_>>();
        let Some(first) = dependent_indices.first().copied() else {
            return Err(syn::Error::new_spanned(
                &flag.name,
                "logical flag has no dependent fields",
            ));
        };
        let last = *dependent_indices.last().expect("nonempty dependent fields");
        if first != flag_index + 1
            || fields[first..=last]
                .iter()
                .any(|field| field.layout.condition.as_ref() != Some(&flag.name))
        {
            return Err(syn::Error::new_spanned(
                &flag.name,
                "conditional group fields must be contiguous immediately after their logical flag",
            ));
        }
    }
    Ok(())
}
