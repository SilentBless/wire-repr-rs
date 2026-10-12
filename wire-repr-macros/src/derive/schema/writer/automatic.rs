use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;

use super::super::computed::computed_offset_state;
use super::super::integer::{retained_controller_offsets, scalar_bytes};
use super::super::model::{FieldKind, Schema};
use super::super::write_fields::{Slot, convert_to_wire};
use super::super::{
    builder_offset, private_ident, scalar_type_tokens, to_bytes_method, value_type_tokens,
};
use super::render_writer_start;

pub(super) fn render_pending_fields(
    schema: &Schema,
    target_physical_index: usize,
    slots: &[Slot],
    conversion_names: &[Option<syn::Ident>],
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let target_slot_index = slots
        .iter()
        .position(|slot| slot.field == schema.fields[target_physical_index].name)
        .expect("target field has one writer slot");
    let previous_physical_index = target_slot_index
        .checked_sub(1)
        .and_then(|index| slots.get(index))
        .and_then(|slot| {
            schema
                .fields
                .iter()
                .position(|field| field.name == slot.field)
        });
    let start = previous_physical_index.map_or(0, |index| index + 1);
    render_automatic_fields(
        schema,
        start,
        target_physical_index,
        conversion_names,
        error,
        runtime,
    )
}

pub(super) fn render_automatic_fields(
    schema: &Schema,
    start: usize,
    end: usize,
    conversion_names: &[Option<syn::Ident>],
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let controller_offsets = retained_controller_offsets(schema);
    let writes =
        schema.fields[start..end]
            .iter()
            .enumerate()
            .filter_map(|(relative_index, field)| {
                let index = start + relative_index;
                if let Some(state) = computed_offset_state(schema, field) {
                    let offset = private_ident(schema, &format!("pending_computed_{index}_offset"));
                    let geometry = render_writer_start(schema, field, &offset, error, runtime);
                    let FieldKind::Scalar(scalar) = &field.kind else {
                        unreachable!("computed destination is scalar")
                    };
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    return Some(quote! {
                        #geometry
                        let placeholder: #wire_ty = 0 as #wire_ty;
                        self.writer.write_at(#offset, &placeholder.#encode())?;
                        self.#state = #offset;
                    });
                }
                let retained_controller = controller_offsets
                    .iter()
                    .find(|(controller, _)| *controller == field.name);
                if let Some((_, state)) = retained_controller {
                    let offset =
                        private_ident(schema, &format!("pending_controller_{index}_offset"));
                    let geometry = render_writer_start(schema, field, &offset, error, runtime);
                    let FieldKind::Scalar(scalar) = &field.kind else {
                        unreachable!("validated count controller is scalar")
                    };
                    let width = if scalar.is_variable() {
                        1
                    } else {
                        scalar.width()
                    };
                    return Some(quote! {
                        #geometry
                        self.writer.write_at(#offset, &[0u8; #width])?;
                        self.#state = #offset;
                    });
                }
                render_constant(
                    schema,
                    index,
                    conversion_names[index].as_ref(),
                    error,
                    runtime,
                )
            });
    quote!(#(#writes)*)
}

pub(super) fn render_finish_constants(
    schema: &Schema,
    last_slot_physical: Option<usize>,
    conversion_names: &[Option<syn::Ident>],
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    if schema.has_nested_geometry() {
        return TokenStream::new();
    }
    let writes = (0..schema.fields.len()).filter_map(|index| {
        if schema.has_explicit_geometry() && last_slot_physical.is_some_and(|last| index <= last) {
            return None;
        }
        render_constant(
            schema,
            index,
            conversion_names[index].as_ref(),
            error,
            runtime,
        )
    });
    quote!(#(#writes)*)
}

fn render_constant(
    schema: &Schema,
    index: usize,
    conversion_variant: Option<&syn::Ident>,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> Option<TokenStream> {
    let field = &schema.fields[index];
    let constant = field.kind.constant()?;
    let offset_local = private_ident(schema, &format!("constant_{index}_offset"));
    let offset_binding = if schema.has_sequential_geometry() {
        render_writer_start(schema, field, &offset_local, error, runtime)
    } else {
        let offset = builder_offset(&field.offset, runtime);
        if schema.layout_can_fail() {
            let field_name = field.name.unraw().to_string();
            quote! {
                let #offset_local = #offset.ok_or_else(|| #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                ))?;
            }
        } else {
            quote! {
                let #offset_local = #offset.expect("validated fixed layout offset");
            }
        }
    };
    Some(match &field.kind {
        FieldKind::Scalar(scalar) => {
            let wire_ty = scalar_type_tokens(scalar.wire_type);
            let encoded = private_ident(schema, &format!("constant_{index}_encoded"));
            let bytes = scalar_bytes(scalar, &encoded, runtime);
            let value = if let Some(variant) = conversion_variant {
                let value_ty = value_type_tokens(&scalar.value_type);
                let semantic = private_ident(schema, &format!("constant_{index}_semantic"));
                let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                quote! {
                    let #semantic: #value_ty = #constant;
                    let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                        #runtime::WriteError::Schema(
                            #error::#variant(#runtime::ScalarBuildConversionError {
                                from: stringify!(#value_ty),
                                to: stringify!(#wire_ty),
                            }),
                        )
                    })?;
                }
            } else {
                quote!(let #encoded: #wire_ty = #constant;)
            };
            quote! {
                #offset_binding
                #value
                self.writer.write_at(#offset_local, #bytes)?;
            }
        }
        FieldKind::Bytes(_) => {
            let ty = &field.ty;
            let value = private_ident(schema, &format!("constant_{index}_bytes"));
            quote! {
                #offset_binding
                let #value: #ty = #constant;
                self.writer.write_at(#offset_local, &#value)?;
            }
        }
        FieldKind::RawBytes(_)
        | FieldKind::ScalarArray(_)
        | FieldKind::Array(_)
        | FieldKind::Recursive(_)
        | FieldKind::Flag(_)
        | FieldKind::BitProjection(_)
        | FieldKind::Nested(_) => unreachable!(),
    })
}
