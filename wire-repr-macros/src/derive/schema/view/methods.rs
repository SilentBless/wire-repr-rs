use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt;

use super::super::model::{
    ArrayField, BitProjection, Field, FieldKind, FixedBytes, FixedScalarArray, NestedField, Scalar,
    Schema, ValueType,
};
use super::super::{
    fresh_schema_lifetime, from_bytes_method, private_ident, scalar_type_tokens, value_type_tokens,
    view_offset,
};
use super::geometry;

/// One field's getter inputs: its static offset and its retained or derived span.
struct Getter<'a> {
    schema: &'a Schema,
    runtime: &'a TokenStream,
    index: usize,
    field: &'a Field,
    offset_value: TokenStream,
    dynamic_start: TokenStream,
    dynamic_end: TokenStream,
}

pub(super) fn render_bodies(schema: &Schema, runtime: &TokenStream) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.name;
            let getter = Getter {
                schema,
                runtime,
                index,
                field,
                offset_value: view_offset(&field.offset, runtime),
                dynamic_start: geometry::field_start(schema, index, runtime),
                dynamic_end: geometry::field_end(schema, index, runtime),
            };
            match &field.kind {
                FieldKind::Scalar(scalar) => render_scalar(&getter, scalar),
                FieldKind::Bytes(bytes) => render_bytes(&getter, bytes),
                FieldKind::ScalarArray(array) => render_scalar_array(&getter, array),
                FieldKind::Array(array) => render_array(&getter, array),
                FieldKind::Flag(_) => {
                    let value = quote!(self.descriptor.#name);
                    quote! {
                        #[inline(always)]
                        fn #name(&self) -> bool {
                            #value
                        }
                    }
                }
                FieldKind::BitProjection(projection) => render_bit_projection(&getter, projection),
                FieldKind::RawBytes(_) => {
                    let Getter {
                        dynamic_start,
                        dynamic_end,
                        ..
                    } = &getter;
                    quote! {
                        #[inline(always)]
                        fn #name(&self) -> &[u8] {
                            &self.as_bytes()[#dynamic_start..#dynamic_end]
                        }
                    }
                }
                FieldKind::Nested(nested) => render_nested(&getter, nested),
                FieldKind::Recursive(_) => {
                    unreachable!("recursive body uses its own view renderer")
                }
            }
        })
        .collect()
}

fn render_scalar(getter: &Getter<'_>, scalar: &Scalar) -> TokenStream {
    let &Getter {
        schema,
        runtime,
        field,
        ref offset_value,
        ref dynamic_start,
        ref dynamic_end,
        ..
    } = getter;
    let name = &field.name;
    let condition = field
        .layout
        .condition
        .as_ref()
        .map(|condition| quote!(self.descriptor.#condition));
    let width = scalar.width();
    let value_ty = value_type_tokens(&scalar.value_type);
    let body = if matches!(&scalar.value_type, ValueType::Char | ValueType::Custom(_))
        && scalar.constant.is_none()
    {
        quote!(self.descriptor.#name)
    } else if let Some(constant) = &scalar.constant {
        quote! {
            let value: #value_ty = #constant;
            value
        }
    } else {
        let wire_ty = scalar_type_tokens(scalar.wire_type);
        let decode = from_bytes_method(scalar.endian);
        let raw = private_ident(schema, "scalar_raw");
        let decoded = if scalar.value_type.is_converted() {
            let conversion = convert_from_validated_wire(scalar, &raw);
            quote!(#conversion)
        } else {
            quote!(#raw)
        };
        if scalar.is_variable() {
            let value =
                super::super::integer::scalar_value(scalar, dynamic_start, dynamic_end, runtime);
            quote! {
                let #raw: #wire_ty = #value;
                #decoded
            }
        } else if schema.has_sequential_geometry() {
            quote! {
                let #raw = #wire_ty::#decode(
                    self.as_bytes()[#dynamic_start..#dynamic_end]
                        .try_into()
                        .expect("validated scalar field span"),
                );
                #decoded
            }
        } else {
            quote! {
                let offset = #offset_value.expect("validated scalar offset");
                let #raw = #wire_ty::#decode(
                    self.as_bytes()[offset..offset + #width]
                        .try_into()
                        .expect("validated scalar field span"),
                );
                #decoded
            }
        }
    };
    let (return_ty, body) = if let Some(condition) = condition {
        (
            quote!(Option<#value_ty>),
            quote! {
                if #condition {
                    Some({ #body })
                } else {
                    None
                }
            },
        )
    } else {
        (value_ty, body)
    };
    quote! {
        #[inline(always)]
        fn #name(&self) -> #return_ty {
            #body
        }
    }
}

fn render_bytes(getter: &Getter<'_>, bytes: &FixedBytes) -> TokenStream {
    let &Getter {
        schema,
        field,
        ref offset_value,
        ref dynamic_start,
        ref dynamic_end,
        ..
    } = getter;
    let name = &field.name;
    let ty = &field.ty;
    let len = &bytes.len;
    let body = if let Some(constant) = &bytes.constant {
        quote! {
            let value: #ty = #constant;
            value
        }
    } else if schema.has_sequential_geometry() {
        quote! {
            self.as_bytes()[#dynamic_start..#dynamic_end]
                .try_into()
                .expect("validated byte-array field span")
        }
    } else {
        quote! {
            let offset = #offset_value.expect("validated byte-array offset");
            self.as_bytes()[offset..offset + #len]
                .try_into()
                .expect("validated byte-array field span")
        }
    };
    quote! {
        #[inline(always)]
        fn #name(&self) -> #ty {
            #body
        }
    }
}

fn render_scalar_array(getter: &Getter<'_>, array: &FixedScalarArray) -> TokenStream {
    let &Getter {
        schema,
        field,
        ref offset_value,
        ref dynamic_start,
        ..
    } = getter;
    let name = &field.name;
    let ty = &field.ty;
    let element = scalar_type_tokens(array.element);
    let width = array.element.width();
    let decode = from_bytes_method(array.endian);
    let base = if schema.has_sequential_geometry() {
        quote!(#dynamic_start)
    } else {
        quote!(#offset_value.expect("validated scalar-array offset"))
    };
    quote! {
        #[inline]
        fn #name(&self) -> #ty {
            let base = #base;
            ::core::array::from_fn(|index| {
                let start = base + index * #width;
                #element::#decode(
                    self.as_bytes()[start..start + #width]
                        .try_into()
                        .expect("validated scalar-array element span"),
                )
            })
        }
    }
}

fn render_array(getter: &Getter<'_>, array: &ArrayField) -> TokenStream {
    let &Getter {
        schema,
        runtime,
        index,
        field,
        ref dynamic_start,
        ref dynamic_end,
        ..
    } = getter;
    let name = &field.name;
    let item = &array.item;
    let count_field = private_ident(schema, &format!("array_{index}_count"));
    let base_field = private_ident(schema, "array_base_offset");
    let base = quote!(self.descriptor.#base_field);
    let count = quote!(self.descriptor.#count_field);
    let constructor = if index + 1 == schema.fields.len() {
        quote! {
            #runtime::ArrayView::terminal(input, count, #base + #dynamic_start)
        }
    } else {
        quote! {
            // SAFETY: parent framing either framed every variable-width item or
            // proved the complete fixed-width span from `count * FIXED_SIZE`.
            unsafe {
                #runtime::ArrayView::from_validated_parts(
                    input,
                    count,
                    #base + #dynamic_start,
                )
            }
        }
    };
    quote! {
        #[inline(always)]
        fn #name(&self) -> #runtime::ArrayView<'_, #item> {
            let count = #count;
            let input = &self.as_bytes()[#dynamic_start..#dynamic_end];
            #constructor
        }
    }
}

fn render_bit_projection(getter: &Getter<'_>, projection: &BitProjection) -> TokenStream {
    let &Getter { schema, field, .. } = getter;
    let name = &field.name;
    let ty = &field.ty;
    let controller = &projection.controller;
    let controller_field = schema
        .fields
        .iter()
        .find(|candidate| candidate.name == *controller)
        .expect("validated bit controller");
    let FieldKind::Scalar(controller_scalar) = &controller_field.kind else {
        unreachable!("validated bit controller is scalar")
    };
    let controller_ty = scalar_type_tokens(controller_scalar.wire_type);
    let start = projection.start;
    let width = projection.end - projection.start + 1;
    let mask = if width == 128 {
        u128::MAX
    } else {
        (1u128 << width) - 1
    };
    let body = if matches!(primitive_name_for_projection(ty).as_deref(), Some("bool")) {
        quote!(((self.#controller() >> #start) & 1) != 0)
    } else {
        quote!(((self.#controller() >> #start) & (#mask as #controller_ty)) as #ty)
    };
    quote! {
        #[inline(always)]
        fn #name(&self) -> #ty {
            #body
        }
    }
}

fn render_nested(getter: &Getter<'_>, nested: &NestedField) -> TokenStream {
    let &Getter {
        schema,
        runtime,
        field,
        ref offset_value,
        ref dynamic_start,
        ref dynamic_end,
        ..
    } = getter;
    let name = &field.name;
    let ty = &nested.ty;
    let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
    let state = quote!(&self.descriptor.#name);
    let offset_setup = if schema.has_sequential_geometry() {
        quote! {
            let offset = #dynamic_start;
            let end = #dynamic_end;
        }
    } else {
        quote! {
            let offset = #offset_value.expect("validated nested offset");
            let end = self.as_bytes().len();
        }
    };
    quote! {
        #[inline(always)]
        fn #name<#field_lifetime>(&#field_lifetime self)
            -> <#ty as #runtime::WireView>::View<#field_lifetime>
        {
            #offset_setup
            // SAFETY: framing produced this state for a child span of this exact length.
            unsafe {
                <#ty as #runtime::WireView>::from_validated_parts(
                    &self.as_bytes()[offset..end],
                    #state,
                )
            }
        }
    }
}

pub(super) fn render_declarations(schema: &Schema, runtime: &TokenStream) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .map(|field| {
            let name = &field.name;
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let ty = value_type_tokens(&scalar.value_type);
                    let ty = if field.layout.condition.is_some() {
                        quote!(Option<#ty>)
                    } else {
                        ty
                    };
                    quote! {
                        #[doc = concat!("Returns decoded field `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    quote! {
                        #[doc = concat!("Returns fixed byte-array field `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::ScalarArray(_) => {
                    let ty = &field.ty;
                    quote! {
                        #[doc = concat!("Returns fixed scalar-array field `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::RawBytes(_) => {
                    quote! {
                        #[doc = concat!("Returns raw byte field `", stringify!(#name), "`.")]
                        fn #name(&self) -> &[u8];
                    }
                }
                FieldKind::Array(array) => {
                    let item = &array.item;
                    let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
                    quote! {
                        #[doc = concat!("Returns counted array field `", stringify!(#name), "`.")]
                        fn #name<#field_lifetime>(
                            &#field_lifetime self,
                        ) -> #runtime::ArrayView<#field_lifetime, #item>;
                    }
                }
                FieldKind::Flag(_) => {
                    quote! {
                        #[doc = concat!("Returns logical presence flag `", stringify!(#name), "`.")]
                        fn #name(&self) -> bool;
                    }
                }
                FieldKind::BitProjection(_) => {
                    let ty = &field.ty;
                    quote! {
                        #[doc = concat!("Returns logical bit projection `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::Nested(nested) => {
                    let ty = &nested.ty;
                    let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
                    let ty = if field.layout.condition.is_some() {
                        quote!(Option<<#ty as #runtime::WireView>::View<#field_lifetime>>)
                    } else {
                        quote!(<#ty as #runtime::WireView>::View<#field_lifetime>)
                    };
                    quote! {
                        #[doc = concat!("Returns nested field `", stringify!(#name), "`.")]
                        fn #name<#field_lifetime>(&#field_lifetime self) -> #ty;
                    }
                }
                FieldKind::Recursive(_) => {
                    unreachable!("recursive body uses its own view renderer")
                }
            }
        })
        .collect()
}

fn convert_from_validated_wire(scalar: &Scalar, raw: &syn::Ident) -> TokenStream {
    match &scalar.value_type {
        ValueType::Scalar(_) => quote!(#raw),
        ValueType::Usize => quote!(#raw as usize),
        ValueType::Isize => quote!(#raw as isize),
        ValueType::Bool => quote!(#raw != 0),
        ValueType::Char | ValueType::Custom(_) => {
            unreachable!("retained converted getters use descriptor state")
        }
    }
}

fn primitive_name_for_projection(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments[0].ident.unraw().to_string())
}
