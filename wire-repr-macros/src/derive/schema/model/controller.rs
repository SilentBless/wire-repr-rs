use syn::Ident;

use super::{Field, FieldKind, ScalarType, SizeTerm, ValueType};
pub(super) fn validate_unsigned_controller(
    fields: &[Field],
    dependent_index: usize,
    controller: &Ident,
    role: &str,
    allow_dynamic_offset: bool,
) -> syn::Result<()> {
    let Some(controller_index) = fields.iter().position(|field| field.name == *controller) else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` is not a schema field"),
        ));
    };
    if controller_index >= dependent_index {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be physically earlier"),
        ));
    }
    let controller_field = &fields[controller_index];
    if controller_field.layout.pad_before.is_some()
        || controller_field.layout.align_before.is_some()
        || controller_field.layout.position.is_some()
        || controller_field.layout.condition.is_some()
        || (!allow_dynamic_offset
            && controller_field
                .offset
                .terms
                .iter()
                .any(|term| matches!(term, SizeTerm::Dynamic)))
    {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must have fixed sequential geometry"),
        ));
    }
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be an integer scalar"),
        ));
    };
    if !scalar.wire_type.is_unsigned_integer()
        || !matches!(
            &scalar.value_type,
            ValueType::Scalar(
                ScalarType::U8
                    | ScalarType::U16
                    | ScalarType::U32
                    | ScalarType::U64
                    | ScalarType::U128
            ) | ValueType::Usize
        )
    {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` must be an unsigned integer"),
        ));
    }
    if scalar.constant.is_some() {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role} controller `{controller}` cannot be a constant"),
        ));
    }
    Ok(())
}
