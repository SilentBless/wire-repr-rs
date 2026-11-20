use proc_macro2::TokenStream;
use quote::quote;

use super::super::model::{Field, FieldKind, LayoutOffset, Schema, SizeTerm};
use super::super::{private_ident, view_offset};

pub(super) fn retains_start(schema: &Schema, index: usize) -> bool {
    let field = &schema.fields[index];
    schema.has_sequential_geometry()
        && (field.layout.pad_before.is_some()
            || field.layout.align_before.is_some()
            || field.layout.position.is_some())
}

pub(super) fn retains_end(schema: &Schema, index: usize) -> bool {
    if !schema.has_sequential_geometry() {
        return false;
    }
    let field = &schema.fields[index];
    if matches!(field.kind, FieldKind::Flag(_) | FieldKind::BitProjection(_)) {
        return false;
    }
    matches!(field.kind, FieldKind::RawBytes(_) | FieldKind::Array(_))
        || (matches!(field.kind, FieldKind::Nested(_)) && !has_fixed_suffix(schema, index))
        || field.layout.condition.is_some()
        || matches!(&field.kind, FieldKind::Scalar(scalar) if scalar.is_variable())
        || field.layout.pad_before.is_some()
        || field.layout.align_before.is_some()
        || field.layout.position.is_some()
}

fn has_fixed_suffix(schema: &Schema, index: usize) -> bool {
    // The represented range and fixed suffix recover the final implicit child's exact end.
    !schema.has_explicit_geometry()
        && schema.fields[index + 1..].iter().all(|field| {
            matches!(
                field.kind,
                FieldKind::Scalar(_)
                    | FieldKind::Bytes(_)
                    | FieldKind::ScalarArray(_)
                    | FieldKind::BitProjection(_)
            )
        })
}

fn state_value(schema: &Schema, index: usize, suffix: &str) -> TokenStream {
    let field = private_ident(schema, &format!("field_{index}_{suffix}"));
    quote!(self.descriptor.#field)
}

pub(super) fn field_start(schema: &Schema, index: usize, runtime: &TokenStream) -> TokenStream {
    if retains_start(schema, index) {
        return state_value(schema, index, "start");
    }
    let field = &schema.fields[index];
    if !field
        .offset
        .terms
        .iter()
        .any(|term| matches!(term, SizeTerm::Dynamic))
    {
        let offset = view_offset(&field.offset, runtime);
        if schema.has_nested_geometry()
            && field
                .offset
                .terms
                .iter()
                .any(|term| matches!(term, SizeTerm::Nested(_)))
        {
            let preceding_end = field_end(schema, index - 1, runtime);
            return quote!(#offset.unwrap_or_else(|| #preceding_end));
        }
        return quote!(#offset.expect("validated static field offset"));
    }
    field_end(schema, index - 1, runtime)
}

pub(super) fn field_end(schema: &Schema, index: usize, runtime: &TokenStream) -> TokenStream {
    if retains_end(schema, index) {
        return state_value(schema, index, "end");
    }
    let start = field_start(schema, index, runtime);
    match &schema.fields[index].kind {
        FieldKind::Scalar(scalar) => {
            let width = scalar.width();
            quote!(#start + #width)
        }
        FieldKind::Bytes(bytes) => {
            let width = &bytes.len;
            quote!(#start + #width)
        }
        FieldKind::ScalarArray(array) => {
            let len = &array.len;
            let width = array.element.width();
            quote!(#start + (#len) * #width)
        }
        FieldKind::Flag(_) | FieldKind::BitProjection(_) => start,
        FieldKind::Nested(nested) => {
            if nested.terminal {
                quote!(self.as_bytes().len())
            } else {
                let suffix = LayoutOffset {
                    terms: schema.fields[index + 1..]
                        .iter()
                        .map(|field| field.kind.size_term())
                        .collect(),
                };
                let width = view_offset(&suffix, runtime);
                quote!(
                    self.as_bytes().len() - #width.expect("framed fixed suffix width")
                )
            }
        }
        FieldKind::RawBytes(_) | FieldKind::Array(_) | FieldKind::Recursive(_) => {
            unreachable!("dynamic field end is retained")
        }
    }
}

pub(super) fn view_offset_for_end(
    _schema: &Schema,
    field: &Field,
    runtime: &TokenStream,
) -> TokenStream {
    let offset = view_offset(&field.offset, runtime);
    if let FieldKind::ScalarArray(array) = &field.kind {
        let len = &array.len;
        let element_width = array.element.width();
        return quote!(
            #offset.and_then(|offset| {
                (#len as usize)
                    .checked_mul(#element_width)
                    .and_then(|width| offset.checked_add(width))
            })
        );
    }
    let width = match &field.kind {
        FieldKind::Scalar(scalar) => {
            let width = scalar.width();
            quote!(#width)
        }
        FieldKind::Bytes(bytes) => {
            let len = &bytes.len;
            quote!(#len)
        }
        FieldKind::ScalarArray(_) => unreachable!("handled by checked scaled width"),
        FieldKind::BitProjection(_) => quote!(0usize),
        FieldKind::RawBytes(_)
        | FieldKind::Array(_)
        | FieldKind::Recursive(_)
        | FieldKind::Flag(_)
        | FieldKind::Nested(_) => unreachable!(),
    };
    quote!(#offset.and_then(|offset| offset.checked_add(#width)))
}
