use syn::Ident;

use super::{Field, FieldKind, Scalar, ScalarType, SizeTerm, ValueType};

#[derive(Clone, Copy)]
pub(super) enum ControllerRole {
    ByteLength,
    ItemCount,
    FieldPosition,
    BitProjection,
}

impl ControllerRole {
    const fn name(self) -> &'static str {
        match self {
            Self::ByteLength => "byte length",
            Self::ItemCount => "item count",
            Self::FieldPosition => "field position",
            Self::BitProjection => "bit projection",
        }
    }
}

pub(super) fn validate_controller(
    fields: &[Field],
    dependent_index: usize,
    controller: &Ident,
    role: ControllerRole,
    allow_dynamic_offset: bool,
) -> syn::Result<()> {
    let role_name = role.name();
    let Some(controller_index) = fields.iter().position(|field| field.name == *controller) else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role_name} controller `{controller}` is not a schema field"),
        ));
    };
    if controller_index >= dependent_index {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role_name} controller `{controller}` must be physically earlier"),
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
            format!("{role_name} controller `{controller}` must have fixed sequential geometry"),
        ));
    }
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role_name} controller `{controller}` must be an integer scalar"),
        ));
    };
    if scalar.is_variable()
        && matches!(
            role,
            ControllerRole::BitProjection | ControllerRole::FieldPosition
        )
    {
        return Err(syn::Error::new_spanned(
            controller,
            format!(
                "`{}` cannot be a {role_name} controller",
                scalar.encoding.name()
            ),
        ));
    }
    let valid_type = if matches!(role, ControllerRole::ItemCount) {
        scalar.is_count_controller()
    } else {
        scalar.is_unsigned_controller()
    };
    if !valid_type {
        let required = if matches!(role, ControllerRole::ItemCount) {
            "an integer scalar"
        } else {
            "an unsigned integer"
        };
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role_name} controller `{controller}` must be {required}"),
        ));
    }
    if scalar.constant.is_some() {
        return Err(syn::Error::new_spanned(
            controller,
            format!("{role_name} controller `{controller}` cannot be a constant"),
        ));
    }
    Ok(())
}

impl Scalar {
    pub(in super::super) fn is_count_controller(&self) -> bool {
        self.is_unsigned_controller()
            || (self.wire_type.is_signed_integer()
                && match self.value_type {
                    ValueType::Scalar(value_type) => value_type.is_signed_integer(),
                    ValueType::Isize => true,
                    _ => false,
                })
    }

    fn is_unsigned_controller(&self) -> bool {
        self.wire_type.is_unsigned_integer()
            && matches!(
                self.value_type,
                ValueType::Scalar(
                    ScalarType::U8
                        | ScalarType::U16
                        | ScalarType::U32
                        | ScalarType::U64
                        | ScalarType::U128
                ) | ValueType::Usize
            )
    }
}
