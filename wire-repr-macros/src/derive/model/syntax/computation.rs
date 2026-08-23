//! Computed-field callback syntax parsing.

use syn::{Expr, Ident, Member, Path};

pub(in crate::derive::model) enum ComputationSyntax {
    Callback {
        path: Path,
        arguments: Vec<ComputationArgument>,
    },
}

pub(in crate::derive::model) enum ComputationArgument {
    Semantic(Ident),
    Bytes(ComputationBytes),
}

pub(in crate::derive::model) enum ComputationBytes {
    Include(Vec<ComputationFieldPath>),
    Exclude(Vec<ComputationFieldPath>),
}

pub(in crate::derive::model) struct ComputationFieldPath {
    pub(in crate::derive::model) top_level: Ident,
    pub(in crate::derive::model) nested: Vec<Ident>,
}

pub(super) fn parse_computation(expression: Expr) -> syn::Result<ComputationSyntax> {
    let Expr::Call(call) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed fields require `computed = callback(arg, include(field, ...), exclude(field, ...))`",
        ));
    };
    let syn::ExprCall { func, args, .. } = call;
    let Expr::Path(function) = *func else {
        return Err(syn::Error::new_spanned(
            func,
            "computed callbacks must be function paths",
        ));
    };
    if function.qself.is_some() {
        return Err(syn::Error::new_spanned(
            function,
            "computed callbacks must be function paths",
        ));
    }
    let arguments = args
        .iter()
        .map(parse_computation_argument)
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(ComputationSyntax::Callback {
        path: function.path,
        arguments,
    })
}

fn parse_computation_argument(expression: &Expr) -> syn::Result<ComputationArgument> {
    if let Expr::Path(path) = expression {
        if let (None, Some(name)) = (&path.qself, path.path.get_ident()) {
            return Ok(ComputationArgument::Semantic(name.clone()));
        }
        return Err(syn::Error::new_spanned(
            path,
            "computed semantic arguments must be top-level field names",
        ));
    }
    parse_computation_bytes(expression).map(ComputationArgument::Bytes)
}

fn parse_computation_bytes(expression: &Expr) -> syn::Result<ComputationBytes> {
    let Expr::Call(call) = expression else {
        return Err(syn::Error::new_spanned(
            expression,
            "computed arguments must be top-level field names, `include(field, ...)`, or `exclude(field, ...)`",
        ));
    };
    let Expr::Path(mode) = call.func.as_ref() else {
        return Err(syn::Error::new_spanned(
            &call.func,
            "computed byte selections require `include(field, ...)` or `exclude(field, ...)`",
        ));
    };
    let selection = if mode.path.is_ident("include") {
        ComputationBytes::Include
    } else if mode.path.is_ident("exclude") {
        ComputationBytes::Exclude
    } else {
        return Err(syn::Error::new_spanned(
            mode,
            "computed byte selections require `include(field, ...)` or `exclude(field, ...)`",
        ));
    };
    call.args
        .iter()
        .map(parse_computation_field_path)
        .collect::<syn::Result<Vec<_>>>()
        .map(selection)
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
