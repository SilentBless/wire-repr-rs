use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;

use super::super::model::{Field, FieldKind, Position, Schema, SizeTerm};
use super::super::{
    builder_offset, private_ident, scalar_type_tokens, to_bytes_method, value_type_tokens,
};
use super::{encode_scalar, scalar_bytes};

pub(in super::super) fn progressive_expansions(
    schema: &Schema,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    variable_controllers(schema)
        .rev()
        .map(|field| {
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("a variable controller is a scalar")
            };
            let offset =
                private_ident(schema, &format!("{}_controller_offset", field.name.unraw()));
            let value = private_ident(schema, &format!("{}_controller_value", field.name.unraw()));
            let encoded = private_ident(
                schema,
                &format!("{}_controller_encoded", field.name.unraw()),
            );
            let encoding = encode_scalar(scalar, quote!(self.#value), runtime);
            quote! {
                let #encoded = #encoding;
                self.writer.expand_integer_at(self.#offset, #encoded.as_ref())?;
            }
        })
        .collect()
}

pub(in super::super) fn detached_expansions(
    schema: &Schema,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    variable_controllers(schema)
        .filter(|field| {
            schema.is_count_controller(&field.name)
                || schema
                    .length_dependents(&field.name)
                    .next()
                    .is_some_and(|first| matches!(first.kind, FieldKind::Nested(_)))
        })
        .rev()
        .map(|field| {
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("a variable controller is a scalar")
            };
            let offset =
                private_ident(schema, &format!("{}_controller_offset", field.name.unraw()));
            let value = private_ident(schema, &format!("{}_controller_value", field.name.unraw()));
            let encoded = private_ident(
                schema,
                &format!("{}_controller_encoded", field.name.unraw()),
            );
            let encoding = encode_scalar(scalar, quote!(#value), runtime);
            quote! {
                let #encoded = #encoding;
                writer.expand_integer_at(#offset, #encoded.as_ref())?;
            }
        })
        .collect()
}

pub(in super::super) fn detached_state(
    schema: &Schema,
    field: &Field,
    encoded: &syn::Ident,
) -> TokenStream {
    let FieldKind::Scalar(scalar) = &field.kind else {
        unreachable!("a controller is a scalar")
    };
    let offset = (schema.is_count_controller(&field.name)
        || schema.is_length_controller(&field.name))
    .then(|| {
        let offset = private_ident(schema, &format!("{}_controller_offset", field.name.unraw()));
        quote!(let #offset = writer.position();)
    });
    let value = private_ident(schema, &format!("{}_controller_value", field.name.unraw()));
    let value_state = if schema.is_count_controller(&field.name) && scalar.is_variable() {
        quote!(let #value: usize;)
    } else if schema.is_length_controller(&field.name) {
        let first = schema
            .length_dependents(&field.name)
            .next()
            .expect("length controller has one dependent");
        if matches!(first.kind, FieldKind::Nested(_)) {
            quote!(let #value: usize;)
        } else {
            quote!(let #value = #encoded as usize;)
        }
    } else {
        TokenStream::new()
    };
    quote!(#offset #value_state)
}
pub(in super::super) fn variable_controllers(
    schema: &Schema,
) -> impl DoubleEndedIterator<Item = &Field> {
    schema.fields.iter().filter(|field| {
        matches!(&field.kind, FieldKind::Scalar(scalar) if scalar.is_variable())
            && (schema.is_length_controller(&field.name) || schema.is_count_controller(&field.name))
    })
}
pub(in super::super) fn retained_controller_offsets(
    schema: &Schema,
) -> Vec<(syn::Ident, syn::Ident)> {
    schema
        .fields
        .iter()
        .filter(|field| {
            (schema.is_count_controller(&field.name)
                || schema.is_length_controller(&field.name)
                || schema.is_presence_controller(&field.name)
                || schema.is_bit_controller(&field.name)
                || schema.fields.iter().any(|dependent| {
                    matches!(&dependent.layout.position,
                        Some(Position::Field(name)) if *name == field.name)
                }))
                && (matches!(&field.kind, FieldKind::Scalar(scalar) if scalar.is_variable())
                    || field
                        .offset
                        .terms
                        .iter()
                        .any(|term| matches!(term, SizeTerm::Dynamic | SizeTerm::Nested(_))))
        })
        .map(|field| {
            (
                field.name.clone(),
                private_ident(schema, &format!("{}_controller_offset", field.name.unraw())),
            )
        })
        .collect()
}
pub(in super::super) fn render_length_patch(
    schema: &Schema,
    controller: &syn::Ident,
    dependent: &syn::Ident,
    length: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let controller_field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated length controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated length controller is scalar")
    };
    let controller_name = controller.unraw().to_string();
    let value_state = private_ident(schema, &format!("{}_controller_value", controller.unraw()));
    if scalar.is_variable() {
        let wire_ty = scalar_type_tokens(scalar.wire_type);
        let is_first = schema
            .length_dependents(controller)
            .next()
            .is_some_and(|field| field.name == *dependent);
        return if is_first {
            quote! {
                let actual = #length;
                let _: #wire_ty = #wire_ty::try_from(actual).map_err(|_| {
                    #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #controller_name }),
                    )
                })?;
                self.#value_state = actual;
            }
        } else {
            quote! {
                let expected = self.#value_state;
                let actual = #length;
                if actual != expected {
                    return Err(#runtime::WriteError::Schema(#error::LengthConflict {
                        controller: #controller_name,
                        expected,
                        actual,
                    }));
                }
            }
        };
    }
    let static_offset = builder_offset(&controller_field.offset, runtime);
    let controller_offset = retained_controller_offsets(schema)
        .into_iter()
        .find(|(name, _)| name == controller)
        .map(|(_, state)| quote!((#static_offset).or(Some(self.#state))))
        .unwrap_or(static_offset);
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let encode = to_bytes_method(scalar.endian);
    let decode = super::super::from_bytes_method(scalar.endian);
    let is_first = schema
        .length_dependents(controller)
        .next()
        .is_some_and(|field| field.name == *dependent);
    if is_first {
        quote! {
            let controller_offset = #controller_offset.ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let controller_value = #wire_ty::try_from(#length).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            self.writer
                .write_at(controller_offset, &controller_value.#encode())?;
        }
    } else {
        quote! {
            let controller_offset = #controller_offset.ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let controller_bytes = self.writer
                .read_at::<{ ::core::mem::size_of::<#wire_ty>() }>(controller_offset)
                .ok_or_else(|| #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                ))?;
            let expected = usize::try_from(#wire_ty::#decode(controller_bytes)).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let actual = #length;
            if actual != expected {
                return Err(#runtime::WriteError::Schema(#error::LengthConflict {
                    controller: #controller_name,
                    expected,
                    actual,
                }));
            }
        }
    }
}
pub(in super::super) fn render_count_patch(
    schema: &Schema,
    controller: &syn::Ident,
    count: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let controller_field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated item count controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated item count controller is scalar")
    };
    let controller_name = controller.unraw().to_string();
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let logical_check = scalar.value_type.is_converted().then(|| {
        let value_ty = value_type_tokens(&scalar.value_type);
        quote! {
            let _: #value_ty = #value_ty::try_from(actual).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
        }
    });
    if scalar.is_variable() {
        let state = private_ident(schema, &format!("{}_controller_value", controller.unraw()));
        return quote! {
            let actual = #count;
            #logical_check
            let _: #wire_ty = #wire_ty::try_from(actual).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            self.#state = actual;
        };
    }
    let static_offset = builder_offset(&controller_field.offset, runtime);
    let controller_offset = retained_controller_offsets(schema)
        .into_iter()
        .find(|(name, _)| name == controller)
        .map(|(_, state)| quote!((#static_offset).or(Some(self.#state))))
        .unwrap_or(static_offset);
    let encode = to_bytes_method(scalar.endian);
    quote! {
        let actual = #count;
        #logical_check
        let controller_offset = #controller_offset.ok_or_else(|| {
            #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #controller_name }),
            )
        })?;
        let controller_value = #wire_ty::try_from(actual).map_err(|_| {
            #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #controller_name }),
            )
        })?;
        self.writer
            .write_at(controller_offset, &controller_value.#encode())?;
    }
}
pub(in super::super) fn render_detached_length_patch(
    schema: &Schema,
    controller: &syn::Ident,
    dependent: &syn::Ident,
    length: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated length controller");
    let FieldKind::Scalar(scalar) = &field.kind else {
        unreachable!("validated length controller is scalar")
    };
    let name = controller.unraw().to_string();
    let offset = private_ident(schema, &format!("{}_controller_offset", controller.unraw()));
    let value = private_ident(schema, &format!("{}_controller_value", controller.unraw()));
    let is_first = schema
        .length_dependents(controller)
        .next()
        .is_some_and(|field| field.name == *dependent);
    if is_first {
        let wire_ty = scalar_type_tokens(scalar.wire_type);
        let encoded = private_ident(schema, "controller_encoded");
        let patch = if scalar.is_variable() {
            TokenStream::new()
        } else {
            let bytes = scalar_bytes(scalar, &encoded, runtime);
            quote!(writer.patch_at(#offset, #bytes)?;)
        };
        quote! {
            let actual = #length;
            let #encoded: #wire_ty = #wire_ty::try_from(actual).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #name }),
                )
            })?;
            #value = actual;
            #patch
        }
    } else {
        quote! {
            let expected = #value;
            let actual = #length;
            if actual != expected {
                return Err(#runtime::WriteError::Schema(#error::LengthConflict {
                    controller: #name,
                    expected,
                    actual,
                }));
            }
        }
    }
}
