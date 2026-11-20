use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;

use super::super::model::{
    ArrayField, DynamicExtent, Field, FieldKind, FixedBytes, FixedScalarArray, NestedField,
    Position, RawBytes, Scalar, ScalarType, Schema, ValueType,
};
use super::super::{
    from_bytes_method, private_ident, scalar_type_tokens, value_type_tokens, view_offset,
};
use super::error::{FieldVariantNames, VariantNames};

/// Inputs shared by every framing step of one schema.
struct Frame<'a> {
    schema: &'a Schema,
    error: &'a syn::Ident,
    frame_input: &'a syn::Ident,
    frame_offset: &'a syn::Ident,
    runtime: &'a TokenStream,
    exact: bool,
}

/// One field of a fixed-offset frame and the bindings its steps share.
struct FixedSite<'a> {
    index: usize,
    field: &'a Field,
    field_name: &'a str,
    names: &'a FieldVariantNames,
    offset: &'a syn::Ident,
    absolute: &'a syn::Ident,
    input_end: &'a syn::Ident,
}

/// One field of a cursor-driven frame and the bindings its steps share.
struct SequentialSite<'a> {
    index: usize,
    field: &'a Field,
    field_name: &'a str,
    names: &'a FieldVariantNames,
    start: &'a syn::Ident,
    end: &'a syn::Ident,
    absolute: &'a syn::Ident,
    input_end: &'a syn::Ident,
    present_value: &'a TokenStream,
}

pub(super) fn render_steps(
    schema: &Schema,
    error: &syn::Ident,
    names: &VariantNames,
    frame_input: &syn::Ident,
    frame_offset: &syn::Ident,
    runtime: &TokenStream,
    exact: bool,
) -> Vec<TokenStream> {
    let frame = Frame {
        schema,
        error,
        frame_input,
        frame_offset,
        runtime,
        exact,
    };
    if schema.has_sequential_geometry() {
        return render_sequential_steps(&frame, names);
    }
    let mut steps = Vec::new();
    for (index, (field, names)) in schema.fields.iter().zip(&names.fields).enumerate() {
        let field_name = field.name.unraw().to_string();
        let offset = private_ident(schema, &format!("field_{index}_offset"));
        let offset_value = view_offset(&field.offset, runtime);
        let absolute = private_ident(schema, &format!("field_{index}_absolute"));
        let input_end = private_ident(schema, &format!("field_{index}_input_end"));
        steps.push(quote! {
            let #offset = #offset_value.ok_or(#error::LayoutUnavailable { field: #field_name })?;
            let #absolute = #frame_offset
                .checked_add(#offset)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            let #input_end = #frame_offset
                .checked_add(#frame_input.len())
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        });
        let site = FixedSite {
            index,
            field,
            field_name: &field_name,
            names,
            offset: &offset,
            absolute: &absolute,
            input_end: &input_end,
        };
        match &field.kind {
            FieldKind::Scalar(scalar) => push_fixed_scalar(&mut steps, &frame, &site, scalar),
            FieldKind::Bytes(bytes_field) => {
                steps.push(fixed_bytes_step(&frame, &site, bytes_field));
            }
            FieldKind::ScalarArray(array) => {
                steps.push(fixed_scalar_array_step(&frame, &site, array));
            }
            FieldKind::Nested(nested) => steps.push(fixed_nested_step(&frame, &site, nested)),
            FieldKind::Array(_) => unreachable!("sequential geometry uses its own frame renderer"),
            FieldKind::RawBytes(_) => {
                unreachable!("sequential geometry uses its own frame renderer")
            }
            FieldKind::Flag(_) => unreachable!("sequential geometry uses its own frame renderer"),
            FieldKind::BitProjection(_) => {}
            FieldKind::Recursive(_) => unreachable!("recursive body uses its own frame renderer"),
        }
    }
    steps
}

fn push_fixed_scalar(
    steps: &mut Vec<TokenStream>,
    frame: &Frame<'_>,
    site: &FixedSite<'_>,
    scalar: &Scalar,
) {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let FixedSite {
        index,
        field_name,
        names,
        offset,
        absolute,
        input_end,
        ..
    } = *site;
    let bytes = private_ident(schema, &format!("scalar_{index}_bytes"));
    let raw = private_ident(schema, &format!("scalar_{index}_raw"));
    let decoded = private_ident(schema, &format!("scalar_{index}_value"));
    let expected_value = private_ident(schema, &format!("scalar_{index}_expected"));
    let width = scalar.width();
    let end = private_ident(schema, &format!("field_{index}_end"));
    let shortage = &names.shortage;
    steps.push(quote! {
        let #end = #offset.checked_add(#width)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        let #bytes: [u8; #width] = #frame_input
            .get(#offset..#end)
            .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            }))?
            .try_into()
            .expect("scalar range has its declared fixed width");
    });
    if scalar.constant.is_some() || scalar.value_type.is_converted() {
        let wire_ty = scalar_type_tokens(scalar.wire_type);
        let value_ty = value_type_tokens(&scalar.value_type);
        let decode = from_bytes_method(scalar.endian);
        let conversion = convert_from_wire(scalar, &raw);
        let converted = if let Some(variant) = &names.conversion {
            quote! {
                let #decoded: #value_ty = #conversion.ok_or_else(|| {
                    #error::#variant(#runtime::ScalarConversionError {
                        offset: #absolute,
                        from: stringify!(#wire_ty),
                        to: stringify!(#value_ty),
                    })
                })?;
            }
        } else {
            quote!(let #decoded: #value_ty = #raw;)
        };
        let constant_check = scalar.constant.as_ref().map(|expected| {
            let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
            let differs = constants_differ(scalar, &decoded, &expected_value);
            quote! {
                let #expected_value: #value_ty = #expected;
                if #differs {
                    return Err(#error::#mismatch(#runtime::ConstantMismatch {
                        offset: #absolute,
                        expected: #expected_value,
                        actual: #decoded,
                    }));
                }
            }
        });
        steps.push(quote! {
            let #raw = #wire_ty::#decode(#bytes);
            #converted
            #constant_check
        });
    }
}

fn fixed_bytes_step(
    frame: &Frame<'_>,
    site: &FixedSite<'_>,
    bytes_field: &FixedBytes,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let FixedSite {
        index,
        field,
        field_name,
        names,
        offset,
        absolute,
        input_end,
    } = *site;
    let bytes = private_ident(schema, &format!("bytes_{index}_value"));
    let end = private_ident(schema, &format!("field_{index}_end"));
    let len = &bytes_field.len;
    let ty = &field.ty;
    let shortage = &names.shortage;
    let constant_check = bytes_field.constant.as_ref().map(|constant| {
        let expected = private_ident(schema, &format!("bytes_{index}_expected"));
        let actual = private_ident(schema, &format!("bytes_{index}_actual"));
        let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
        quote! {
            let #expected: #ty = #constant;
            if #bytes != #expected.as_slice() {
                let #actual: #ty = #bytes
                    .try_into()
                    .expect("mismatched byte-array span has its declared width");
                return Err(#error::#mismatch(#runtime::ConstantMismatch {
                    offset: #absolute,
                    expected: #expected,
                    actual: #actual,
                }));
            }
        }
    });
    quote! {
        let #end = #offset.checked_add(#len)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        let #bytes = #frame_input
            .get(#offset..#end)
            .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            }))?;
        #constant_check
    }
}

fn fixed_scalar_array_step(
    frame: &Frame<'_>,
    site: &FixedSite<'_>,
    array: &FixedScalarArray,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let FixedSite {
        index,
        field_name,
        names,
        offset,
        input_end,
        ..
    } = *site;
    let len = &array.len;
    let element_width = array.element.width();
    let total = private_ident(schema, &format!("array_{index}_width"));
    let end = private_ident(schema, &format!("field_{index}_end"));
    let shortage = &names.shortage;
    quote! {
        let #total = (#len as usize)
            .checked_mul(#element_width)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        let #end = #offset
            .checked_add(#total)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        #frame_input
            .get(#offset..#end)
            .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            }))?;
    }
}

fn fixed_nested_step(frame: &Frame<'_>, site: &FixedSite<'_>, nested: &NestedField) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        exact,
        ..
    } = *frame;
    let FixedSite {
        field,
        field_name,
        names,
        offset,
        absolute,
        input_end,
        ..
    } = *site;
    let ty = &nested.ty;
    let available = private_ident(schema, &format!("{}_input", field.name.unraw()));
    let child_frame = private_ident(schema, &format!("{}_frame", field.name.unraw()));
    let child_state = private_ident(schema, &format!("{}_state", field.name.unraw()));
    let child_consumed = private_ident(schema, &format!("{}_consumed", field.name.unraw()));
    let child_total = private_ident(schema, &format!("{}_total", field.name.unraw()));
    let shortage = &names.shortage;
    let variant = names.nested.as_ref().expect("nested error variant");
    let extent = names.extent.as_ref().expect("nested extent variant");
    let frame_method = if exact {
        quote!(frame_exact)
    } else {
        quote!(frame)
    };
    quote! {
        let #available = #frame_input.get(#offset..).ok_or_else(|| {
            #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #offset.saturating_sub(#frame_input.len()),
            })
        })?;
        let #child_frame = <#ty as #runtime::WireView>::#frame_method(
            #available,
            #absolute,
        )
        .map_err(#error::#variant)?;
        let (#child_state, #child_consumed) = #child_frame.into_parts();
        if #child_consumed > #available.len() {
            return Err(#error::#extent(#runtime::InvalidFrameExtent {
                offset: #absolute,
                consumed: #child_consumed,
                available: #available.len(),
            }));
        }
        let #child_total = #offset.checked_add(#child_consumed)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
    }
}

fn render_sequential_steps(frame: &Frame<'_>, names: &VariantNames) -> Vec<TokenStream> {
    let Frame {
        schema,
        error,
        frame_input,
        frame_offset,
        ..
    } = *frame;
    let cursor = private_ident(schema, "geometry_cursor");
    let input_end = private_ident(schema, "geometry_input_end");
    let mut steps = vec![quote! {
        let mut #cursor = 0usize;
        let #input_end = #frame_offset
            .checked_add(#frame_input.len())
            .ok_or(#error::LayoutUnavailable { field: "<input>" })?;
    }];

    for (index, (field, names)) in schema.fields.iter().zip(&names.fields).enumerate() {
        let field_name = field.name.unraw().to_string();
        let start = private_ident(schema, &format!("field_{index}_start"));
        let end = private_ident(schema, &format!("field_{index}_end"));
        let absolute = private_ident(schema, &format!("field_{index}_absolute"));
        let position = sequential_position(frame, field, &field_name, &cursor);
        let present =
            field.layout.condition.as_ref().map(|condition| {
                private_ident(schema, &format!("flag_{}_value", condition.unraw()))
            });
        let present_value = present
            .as_ref()
            .map(|present| quote!(#present))
            .unwrap_or_else(|| quote!(true));
        let position = quote!(if #present_value { #position } else { #cursor });
        let absolute_position = if matches!(field.kind, FieldKind::RawBytes(_))
            && field.layout.pad_before.is_none()
            && field.layout.align_before.is_none()
            && field.layout.position.is_none()
        {
            // The sequential cursor is already checked; raw fields never use its absolute start.
            TokenStream::new()
        } else {
            quote! {
                let #absolute = #frame_offset
                    .checked_add(#start)
                    .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            }
        };
        steps.push(quote! {
            let #start: usize = #position;
            if #start < #cursor {
                return Err(#error::PositionBeforeCursor {
                    field: #field_name,
                    position: #start,
                    cursor: #cursor,
                });
            }
            #absolute_position
        });

        let site = SequentialSite {
            index,
            field,
            field_name: &field_name,
            names,
            start: &start,
            end: &end,
            absolute: &absolute,
            input_end: &input_end,
            present_value: &present_value,
        };
        match &field.kind {
            FieldKind::Scalar(scalar) => steps.push(sequential_scalar_step(frame, &site, scalar)),
            FieldKind::Flag(flag) => {
                let value = private_ident(schema, &format!("flag_{}_value", field.name.unraw()));
                let controller =
                    private_ident(schema, &format!("presence_{}", flag.controller.unraw()));
                steps.push(quote! {
                    let #end = #start;
                    let #value = #controller;
                });
            }
            FieldKind::BitProjection(_) => {
                steps.push(quote! {
                    let #end = #start;
                });
            }
            FieldKind::Bytes(bytes_field) => {
                steps.push(sequential_bytes_step(frame, &site, bytes_field));
            }
            FieldKind::ScalarArray(array) => {
                steps.push(sequential_scalar_array_step(frame, &site, array));
            }
            FieldKind::RawBytes(raw) => steps.push(sequential_raw_bytes_step(frame, &site, raw)),
            FieldKind::Array(array) => steps.push(sequential_array_step(frame, &site, array)),
            FieldKind::Nested(nested) => steps.push(sequential_nested_step(frame, &site, nested)),
            FieldKind::Recursive(_) => {
                unreachable!("recursive body uses its own frame renderer")
            }
        }
        steps.push(quote! {
            #cursor = #end;
        });
    }
    steps
}

/// Renders the start a present field requests: a fixed or controlled position, or the cursor
/// after its padding and alignment.
fn sequential_position(
    frame: &Frame<'_>,
    field: &Field,
    field_name: &str,
    cursor: &syn::Ident,
) -> TokenStream {
    let Frame {
        schema,
        error,
        runtime,
        ..
    } = *frame;
    match &field.layout.position {
        Some(Position::Static(position)) => quote!(#position),
        Some(Position::Field(controller)) => {
            let value = private_ident(schema, &format!("controller_{}", controller.unraw()));
            quote!(#value)
        }
        None => {
            let pad = field
                .layout
                .pad_before
                .as_ref()
                .map(|pad| {
                    quote! {
                        start = start.checked_add(#pad)
                            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                    }
                })
                .unwrap_or_default();
            let align = field
                .layout
                .align_before
                .as_ref()
                .map(|align| {
                    quote! {
                        start = #runtime::__private::checked_align(start, #align)
                            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                    }
                })
                .unwrap_or_default();
            quote! {{
                let mut start = #cursor;
                #pad
                #align
                start
            }}
        }
    }
}

fn sequential_scalar_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    scalar: &Scalar,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let SequentialSite {
        index,
        field,
        field_name,
        names,
        start,
        end,
        absolute,
        input_end,
        present_value,
    } = *site;
    let width = scalar.width();
    let shortage = &names.shortage;
    let bytes = private_ident(schema, &format!("scalar_{index}_bytes"));
    let raw = private_ident(schema, &format!("scalar_{index}_raw"));
    let value = private_ident(schema, &format!("scalar_{index}_value"));
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let value_ty = value_type_tokens(&scalar.value_type);
    let decode = from_bytes_method(scalar.endian);
    let conversion = convert_from_wire(scalar, &raw);
    let converted = if let Some(variant) = &names.conversion {
        quote! {
            let #value: #value_ty = #conversion.ok_or_else(|| {
                #error::#variant(#runtime::ScalarConversionError {
                    offset: #absolute,
                    from: stringify!(#wire_ty),
                    to: stringify!(#value_ty),
                })
            })?;
        }
    } else {
        quote!(let #value: #value_ty = #raw;)
    };
    let constant_check = scalar.constant.as_ref().map(|expected| {
        let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
        let expected_value = private_ident(schema, &format!("scalar_{index}_expected"));
        let differs = constants_differ(scalar, &value, &expected_value);
        quote! {
            let #expected_value: #value_ty = #expected;
            if #differs {
                return Err(#error::#mismatch(#runtime::ConstantMismatch {
                    offset: #absolute,
                    expected: #expected_value,
                    actual: #value,
                }));
            }
        }
    });
    let controller_value = sequential_controller_value(frame, site, scalar, &raw, &wire_ty);
    let presence_controller_value = schema.is_presence_controller(&field.name).then(|| {
        let presence = private_ident(schema, &format!("presence_{}", field.name.unraw()));
        quote!(let #presence = #value;)
    });
    let decode_value = if scalar.is_variable() {
        let available = private_ident(schema, &format!("scalar_{index}_available"));
        let encoded_len = private_ident(schema, &format!("scalar_{index}_encoded_len"));
        let decode = super::super::integer::decode_field(
            scalar,
            frame_input,
            start,
            absolute,
            input_end,
            &available,
            runtime,
        );
        quote! {
            let (#raw, #encoded_len) = #decode.map_err(#error::#shortage)?;
            let #raw: #wire_ty = #raw as #wire_ty;
            let #end = #start.checked_add(#encoded_len)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            #converted
            #constant_check
            #controller_value
            #presence_controller_value
        }
    } else {
        quote! {
            let #bytes: [u8; #width] = #frame_input
                .get(#start..#end)
                .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                    offset: #input_end,
                    additional_at_least: #end.saturating_sub(#frame_input.len()),
                }))?
                .try_into()
                .expect("scalar range has its declared fixed width");
            let #raw = #wire_ty::#decode(#bytes);
            #converted
            #constant_check
            #controller_value
            #presence_controller_value
        }
    };
    if scalar.is_variable() {
        decode_value
    } else if field.layout.condition.is_some() {
        quote! {
            let #end = if #present_value {
                #start.checked_add(#width)
                    .ok_or(#error::LayoutUnavailable { field: #field_name })?
            } else {
                #start
            };
            if #present_value {
                #decode_value
            }
        }
    } else {
        quote! {
            let #end = #start.checked_add(#width)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            #decode_value
        }
    }
}

/// Renders the `usize` binding later fields read when this scalar controls a length, a count,
/// or a position.
fn sequential_controller_value(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    scalar: &Scalar,
    raw: &syn::Ident,
    wire_ty: &TokenStream,
) -> Option<TokenStream> {
    let Frame {
        schema,
        error,
        runtime,
        ..
    } = *frame;
    let SequentialSite {
        field,
        field_name,
        names,
        absolute,
        ..
    } = *site;
    let is_controller = schema.is_length_controller(&field.name)
        || schema.is_count_controller(&field.name)
        || schema.fields.iter().any(|candidate| {
            matches!(
                &candidate.layout.position,
                Some(Position::Field(controller)) if controller == &field.name
            )
        });
    is_controller.then(|| {
        let controller = private_ident(schema, &format!("controller_{}", field.name.unraw()));
        let conversion_error = if scalar.wire_type.is_signed_integer() {
            let variant = names.conversion.as_ref().expect("signed count conversion");
            quote!(#error::#variant(#runtime::ScalarConversionError {
                offset: #absolute,
                from: stringify!(#wire_ty),
                to: "usize",
            }))
        } else {
            quote!(#error::LayoutUnavailable { field: #field_name })
        };
        quote! {
            let #controller = usize::try_from(#raw)
                .map_err(#[cold] |_| #conversion_error)?;
        }
    })
}

fn sequential_bytes_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    bytes_field: &FixedBytes,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let SequentialSite {
        index,
        field,
        field_name,
        names,
        start,
        end,
        absolute,
        input_end,
        ..
    } = *site;
    let shortage = &names.shortage;
    let len = &bytes_field.len;
    let bytes = private_ident(schema, &format!("bytes_{index}_value"));
    let ty = &field.ty;
    let constant_check = bytes_field.constant.as_ref().map(|constant| {
        let expected = private_ident(schema, &format!("bytes_{index}_expected"));
        let actual = private_ident(schema, &format!("bytes_{index}_actual"));
        let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
        quote! {
            let #expected: #ty = #constant;
            if #bytes != #expected.as_slice() {
                let #actual: #ty = #bytes.try_into()
                    .expect("byte-array span has its declared width");
                return Err(#error::#mismatch(#runtime::ConstantMismatch {
                    offset: #absolute,
                    expected: #expected,
                    actual: #actual,
                }));
            }
        }
    });
    quote! {
        let #end = #start.checked_add(#len)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        let #bytes = #frame_input
            .get(#start..#end)
            .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            }))?;
        #constant_check
    }
}

fn sequential_scalar_array_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    array: &FixedScalarArray,
) -> TokenStream {
    let Frame {
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let SequentialSite {
        field_name,
        names,
        start,
        end,
        input_end,
        ..
    } = *site;
    let shortage = &names.shortage;
    let len = &array.len;
    let element_width = array.element.width();
    quote! {
        let total = (#len as usize)
            .checked_mul(#element_width)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        let #end = #start
            .checked_add(total)
            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        #frame_input
            .get(#start..#end)
            .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            }))?;
    }
}

fn sequential_raw_bytes_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    raw: &RawBytes,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        ..
    } = *frame;
    let SequentialSite {
        field_name,
        names,
        start,
        end,
        input_end,
        ..
    } = *site;
    let shortage = &names.shortage;
    let extent = match &raw.extent {
        DynamicExtent::Bounded(controller) => {
            let value = private_ident(schema, &format!("controller_{}", controller.unraw()));
            quote!(#start.checked_add(#value)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?)
        }
        DynamicExtent::Rest => {
            quote!(::core::cmp::max(#start, #frame_input.len()))
        }
    };
    quote! {
        let #end = #extent;
        #frame_input.get(#start..#end).ok_or_else(|| {
            #error::#shortage(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #end.saturating_sub(#frame_input.len()),
            })
        })?;
    }
}

fn sequential_array_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    array: &ArrayField,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        exact,
        ..
    } = *frame;
    let SequentialSite {
        index,
        field_name,
        names,
        start,
        end,
        absolute,
        input_end,
        ..
    } = *site;
    let item = &array.item;
    let controller = private_ident(schema, &format!("controller_{}", array.controller.unraw()));
    let variant = names.nested.as_ref().expect("array error variant");
    if index + 1 == schema.fields.len() && exact {
        quote! {
            #frame_input.get(#start..).ok_or_else(|| {
                #error::#variant(#runtime::ArrayError::NeedMore(
                    #runtime::NeedMore {
                        offset: #input_end,
                        additional_at_least: #start
                            .saturating_sub(#frame_input.len()),
                    },
                ))
            })?;
            let #end = #frame_input.len();
        }
    } else {
        let leading = quote! {
            if #controller != 0 && !<#item as #runtime::WireView>::LEADING_EXTENT {
                return Err(#error::LayoutUnavailable { field: #field_name });
            }
        };
        quote! {
            let available = #frame_input.get(#start..).ok_or_else(|| {
                #error::#variant(#runtime::ArrayError::NeedMore(
                    #runtime::NeedMore {
                        offset: #input_end,
                        additional_at_least: #start
                            .saturating_sub(#frame_input.len()),
                    },
                ))
            })?;
            #leading
            let consumed = #runtime::__private::frame_array_extent::<#item>(
                available,
                #controller,
                #absolute,
            )
            .map_err(#error::#variant)?;
            let #end = #start.checked_add(consumed)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        }
    }
}

fn sequential_nested_step(
    frame: &Frame<'_>,
    site: &SequentialSite<'_>,
    nested: &NestedField,
) -> TokenStream {
    let Frame {
        schema,
        error,
        frame_input,
        runtime,
        exact,
        ..
    } = *frame;
    let SequentialSite {
        field,
        field_name,
        names,
        start,
        end,
        absolute,
        input_end,
        ..
    } = *site;
    let ty = &nested.ty;
    let shortage = &names.shortage;
    let variant = names.nested.as_ref().expect("nested error variant");
    let extent_variant = names.extent.as_ref().expect("nested extent variant");
    let available = private_ident(schema, &format!("{}_input", field.name.unraw()));
    let child_frame = private_ident(schema, &format!("{}_frame", field.name.unraw()));
    let child_state = private_ident(schema, &format!("{}_state", field.name.unraw()));
    let child_consumed = private_ident(schema, &format!("{}_consumed", field.name.unraw()));
    let declared = nested
        .extent
        .as_ref()
        .map(|controller| private_ident(schema, &format!("controller_{}", controller.unraw())));
    let frame_method = if nested.extent.is_some() || (exact && nested.terminal) {
        quote!(frame_exact)
    } else {
        quote!(frame)
    };
    let leading_check = (!nested.terminal && nested.extent.is_none()).then(|| {
        quote! {
            if <#ty as #runtime::WireView>::FIXED_SIZE.is_none()
                && !<#ty as #runtime::WireView>::LEADING_EXTENT
            {
                return Err(#error::LayoutUnavailable { field: #field_name });
            }
        }
    });
    let prepare_end = if let Some(declared) = &declared {
        quote! {
            let #end = #start.checked_add(#declared)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            let #available = #frame_input.get(#start..#end).ok_or_else(|| {
                #error::#shortage(#runtime::NeedMore {
                    offset: #input_end,
                    additional_at_least: #end.saturating_sub(#frame_input.len()),
                })
            })?;
        }
    } else if !nested.terminal {
        let fixed_end = private_ident(schema, &format!("{}_fixed_end", field.name.unraw()));
        quote! {
            let #available = if let Some(width) =
                <#ty as #runtime::WireView>::FIXED_SIZE
            {
                let #fixed_end = #start.checked_add(width)
                    .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                #frame_input.get(#start..#fixed_end).ok_or_else(|| {
                    #error::#shortage(#runtime::NeedMore {
                        offset: #input_end,
                        additional_at_least: #fixed_end
                            .saturating_sub(#frame_input.len()),
                    })
                })?
            } else {
                #frame_input.get(#start..).ok_or_else(|| {
                    #error::#shortage(#runtime::NeedMore {
                        offset: #input_end,
                        additional_at_least: #start
                            .saturating_sub(#frame_input.len()),
                    })
                })?
            };
        }
    } else {
        quote! {
            let #available = #frame_input.get(#start..).ok_or_else(|| {
                #error::#shortage(#runtime::NeedMore {
                    offset: #input_end,
                    additional_at_least: #start.saturating_sub(#frame_input.len()),
                })
            })?;
        }
    };
    let finish_end = if let Some(declared) = &declared {
        quote! {
            if #child_consumed != #declared {
                return Err(#error::#extent_variant(#runtime::InvalidFrameExtent {
                    offset: #absolute,
                    consumed: #child_consumed,
                    available: #declared,
                }));
            }
        }
    } else {
        let fixed_check = (!nested.terminal).then(|| {
            quote! {
                if let Some(expected) = <#ty as #runtime::WireView>::FIXED_SIZE {
                    if #child_consumed != expected {
                        return Err(#error::#extent_variant(#runtime::InvalidFrameExtent {
                            offset: #absolute,
                            consumed: #child_consumed,
                            available: expected,
                        }));
                    }
                }
            }
        });
        quote! {
            #fixed_check
            let #end = #start.checked_add(#child_consumed)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        }
    };
    quote! {
        #leading_check
        #prepare_end
        let #child_frame = <#ty as #runtime::WireView>::#frame_method(#available, #absolute)
            .map_err(#error::#variant)?;
        let (#child_state, #child_consumed) = #child_frame.into_parts();
        if #child_consumed > #available.len() {
            return Err(#error::#extent_variant(#runtime::InvalidFrameExtent {
                offset: #absolute,
                consumed: #child_consumed,
                available: #available.len(),
            }));
        }
        #finish_end
    }
}

fn constants_differ(scalar: &Scalar, actual: &syn::Ident, expected: &syn::Ident) -> TokenStream {
    if matches!(
        &scalar.value_type,
        ValueType::Scalar(ScalarType::F32 | ScalarType::F64)
    ) {
        quote!(#actual.to_bits() != #expected.to_bits())
    } else {
        quote!(#actual != #expected)
    }
}

fn convert_from_wire(scalar: &Scalar, raw: &syn::Ident) -> TokenStream {
    match &scalar.value_type {
        ValueType::Scalar(_) => quote!(Some(#raw)),
        ValueType::Usize => quote!(usize::try_from(#raw).ok()),
        ValueType::Isize => quote!(isize::try_from(#raw).ok()),
        ValueType::Bool => quote!(match #raw {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }),
        ValueType::Char => quote!(u32::try_from(#raw).ok().and_then(char::from_u32)),
        ValueType::Custom(ty) => quote!(<#ty>::try_from(#raw).ok()),
    }
}
