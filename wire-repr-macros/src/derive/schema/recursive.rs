use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::visit::{self, Visit};
use syn::{GenericArgument, GenericParam, Ident, PathArguments, Type};

use super::model::{FieldKind, Schema};

pub(super) struct RecursiveSlot {
    pub(super) generic: Ident,
    pub(super) index: usize,
    pub(super) marker: Ident,
    pub(super) fields: Vec<usize>,
}

pub(super) fn schema_slots(schema: &Schema) -> Vec<RecursiveSlot> {
    schema
        .generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            GenericParam::Type(parameter) => Some(parameter.ident.clone()),
            GenericParam::Lifetime(_) | GenericParam::Const(_) => None,
        })
        .enumerate()
        .filter_map(|(slot_index, generic)| {
            let fields = schema
                .fields
                .iter()
                .enumerate()
                .filter_map(|(index, field)| match &field.kind {
                    FieldKind::Array(array) if type_is_parameter(&array.item, &generic) => {
                        Some(index)
                    }
                    FieldKind::Scalar(_)
                    | FieldKind::Bytes(_)
                    | FieldKind::RawBytes(_)
                    | FieldKind::Array(_)
                    | FieldKind::Flag(_)
                    | FieldKind::Nested(_)
                    | FieldKind::BitProjection(_) => None,
                })
                .collect::<Vec<_>>();
            (!fields.is_empty()).then(|| RecursiveSlot {
                marker: format_ident!(
                    "__WireRepr{}RecursiveSlot{}",
                    schema.name.unraw(),
                    slot_index
                ),
                index: slot_index,
                generic,
                fields,
            })
        })
        .collect()
}

pub(super) fn type_is_parameter(ty: &Type, parameter: &Ident) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.len() == 1
        && path.path.segments[0].arguments.is_empty()
        && path.path.segments[0].ident == *parameter
}

fn type_is_root(ty: &Type, root: &Ident) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.len() == 1
        && path.path.segments[0].arguments.is_empty()
        && (path.path.segments[0].ident == *root || path.path.segments[0].ident == "Self")
}

pub(super) fn normalize_root_self(body: &Type, root: &Ident) -> Type {
    let mut normalized = body.clone();
    let Type::Path(path) = &mut normalized else {
        return normalized;
    };
    let Some(segment) = path.path.segments.last_mut() else {
        return normalized;
    };
    let PathArguments::AngleBracketed(arguments) = &mut segment.arguments else {
        return normalized;
    };
    for argument in &mut arguments.args {
        let GenericArgument::Type(Type::Path(path)) = argument else {
            continue;
        };
        if path.qself.is_none()
            && path.path.segments.len() == 1
            && path.path.segments[0].ident == "Self"
            && path.path.segments[0].arguments.is_empty()
        {
            *argument = GenericArgument::Type(syn::parse_quote!(#root));
        }
    }
    normalized
}

pub(super) fn root_slot_marker(
    body: &Type,
    root: &Ident,
    runtime: &TokenStream,
) -> syn::Result<Option<Type>> {
    if !contains_root_generic_argument(body, root) {
        return Ok(None);
    }
    let Type::Path(path) = body else {
        return Err(syn::Error::new_spanned(
            body,
            "recursive enum bodies must use a plain generic schema type",
        ));
    };
    if path.qself.is_some() {
        return Err(syn::Error::new_spanned(
            body,
            "recursive enum bodies must use a plain schema type path",
        ));
    }
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            body,
            "recursive enum body path is empty",
        ));
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            body,
            "recursive enum bodies must pass the root as a direct generic argument",
        ));
    };

    let mut type_index = 0usize;
    let mut selected = None;
    for argument in &arguments.args {
        let GenericArgument::Type(argument) = argument else {
            continue;
        };
        if type_is_root(argument, root) {
            if selected.replace(type_index).is_some() {
                return Err(syn::Error::new_spanned(
                    body,
                    "recursive enum body may contain the root in only one direct generic slot",
                ));
            }
        } else if contains_root_generic_argument(argument, root) {
            return Err(syn::Error::new_spanned(
                argument,
                "recursive enum body must pass the root directly to its recursive schema slot",
            ));
        }
        type_index += 1;
    }
    let Some(selected) = selected else {
        return Err(syn::Error::new_spanned(
            body,
            "recursive enum body must pass the root as a direct generic argument",
        ));
    };

    let normalized = normalize_root_self(body, root);
    Ok(Some(syn::parse_quote!(
        <#normalized as #runtime::__private::RecursiveSlot<#selected>>::Marker
    )))
}

pub(super) fn contains_root_generic_argument(body: &Type, root: &Ident) -> bool {
    let mut visitor = RootArgumentVisitor { root, found: false };
    visitor.visit_type(body);
    visitor.found
}

struct RootArgumentVisitor<'a> {
    root: &'a Ident,
    found: bool,
}
impl<'ast> Visit<'ast> for RootArgumentVisitor<'_> {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if type_is_root(&Type::Path(path.clone()), self.root) {
            self.found = true;
            return;
        }
        visit::visit_type_path(self, path);
    }
}

pub(super) fn render_bodies(schema: &Schema, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let slots = schema_slots(schema);
    if slots.is_empty() {
        return Ok(TokenStream::new());
    }
    if schema.generics.params.len() != 1
        || !matches!(schema.generics.params.first(), Some(GenericParam::Type(_)))
    {
        return Ok(TokenStream::new());
    }

    let name = &schema.name;
    let vis = &schema.vis;
    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let mut rendered = TokenStream::new();

    for slot in slots {
        if slot.fields.len() != 1 {
            continue;
        }
        let field_index = slot.fields[0];
        let field = &schema.fields[field_index];
        let FieldKind::Array(array) = &field.kind else {
            unreachable!("schema_slots selects arrays")
        };
        if field_index + 1 != schema.fields.len() {
            continue;
        }
        let Some((controller_index, controller)) = schema
            .fields
            .iter()
            .enumerate()
            .find(|(_, candidate)| candidate.name == array.controller)
        else {
            continue;
        };
        if controller_index != 0 || field_index != 1 {
            continue;
        }
        let FieldKind::Scalar(controller_scalar) = &controller.kind else {
            continue;
        };
        if !controller_scalar.wire_type.is_unsigned_integer()
            || controller_scalar.constant.is_some()
            || controller_scalar.computed.is_some()
            || controller.layout.condition.is_some()
            || controller.layout.position.is_some()
            || controller.layout.pad_before.is_some()
            || controller.layout.align_before.is_some()
            || field.layout.condition.is_some()
            || field.layout.position.is_some()
            || field.layout.pad_before.is_some()
            || field.layout.align_before.is_some()
        {
            continue;
        }

        let marker = &slot.marker;
        let root = &slot.generic;
        let slot_index = slot.index;
        let state = format_ident!("{}State", marker);
        let body_view = format_ident!("{}View", marker);
        let width = controller_scalar.width();
        let wire_type = super::scalar_type_tokens(controller_scalar.wire_type);
        let value_type = super::value_type_tokens(controller_scalar.value_type);
        let decode = super::from_bytes_method(controller_scalar.endian);
        let callback = super::fresh_type_ident(&schema.generics, "RecursiveCallback");
        let count_name = &controller.name;
        let items_name = &field.name;
        let mut recursive_impl_generics = schema.generics.clone();
        recursive_impl_generics
            .params
            .push(syn::parse_quote!(#callback));
        let recursive_depth = super::fresh_type_ident(&schema.generics, "RecursiveDepth");
        let (body_impl, _, body_where) = recursive_impl_generics.split_for_impl();

        rendered.extend(quote! {
            #[doc(hidden)]
            #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

            impl #schema_impl #runtime::__private::RecursiveSlot<#slot_index>
                for #self_type #schema_where
            {
                type Marker = #marker<#root>;
            }

            #[doc(hidden)]
            #vis struct #state {
                count: usize,
                start: usize,
                end: usize,
                geometry: #runtime::__private::RecursiveGeometry,
            }

            #[doc(hidden)]
            #vis struct #body_view<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                const #recursive_depth: usize,
            > {
                input: &'__wire_repr_recursive_view [u8],
                state: &'__wire_repr_recursive_view #state,
                offset: usize,
                depth: #runtime::__private::RecursiveDepth,
                marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
            }

            impl<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                const #recursive_depth: usize,
            > #body_view<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                #recursive_depth,
            >
            where
                #callback: #runtime::__private::RecursiveFrame<
                    #marker<#root>,
                    Root = #root,
                >,
            {
                #[inline(always)]
                pub fn #count_name(&self) -> #value_type {
                    #value_type::try_from(self.state.count)
                        .expect("framing decoded the count from this representation")
                }

                #[inline(always)]
                pub fn #items_name(&self) -> #runtime::__private::RecursiveArrayView<
                    '__wire_repr_recursive_view,
                    '_,
                    #callback,
                    #marker<#root>,
                    #recursive_depth,
                > {
                    // SAFETY: body framing established this exact item span, count, and geometry.
                    #[allow(unsafe_code)]
                    unsafe {
                        #runtime::__private::RecursiveArrayView::from_validated_parts(
                            &self.input[self.state.start..self.state.end],
                            self.state.count,
                            self.offset + self.state.start,
                            self.depth,
                            &self.state.geometry,
                        )
                    }
                }
            }

            impl<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                const #recursive_depth: usize,
            > AsRef<[u8]> for #body_view<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                #recursive_depth,
            >
            {
                fn as_ref(&self) -> &[u8] {
                    self.input
                }
            }

            #[allow(unsafe_code)]
            impl #body_impl #runtime::__private::RecursiveBody<#callback, #marker<#root>>
                for #self_type #body_where
            {
                type State = #state;
                type Error = #runtime::__private::RecursiveError;
                type View<
                    '__wire_repr_recursive_view,
                    const #recursive_depth: usize,
                > = #body_view<
                    '__wire_repr_recursive_view,
                    #callback,
                    #root,
                    #recursive_depth,
                >;

                fn recursive_children(
                    input: &[u8],
                    absolute_offset: usize,
                ) -> Result<#runtime::__private::RecursiveChildren, Self::Error> {
                    let bytes: [u8; #width] = input
                        .get(..#width)
                        .ok_or(#runtime::__private::RecursiveError::NeedMore(
                            #runtime::NeedMore {
                                offset: absolute_offset.saturating_add(input.len()),
                                additional_at_least: #width.saturating_sub(input.len()),
                            },
                        ))?
                        .try_into()
                        .expect("count width checked");
                    let raw = #wire_type::#decode(bytes);
                    let count = u32::try_from(raw).map_err(|_| {
                        #runtime::__private::RecursiveError::Layout(#runtime::LayoutError {
                            field: stringify!(#count_name),
                        })
                    })?;
                    Ok(#runtime::__private::RecursiveChildren {
                        count,
                        prefix: #width,
                    })
                }

                fn frame_recursive<const #recursive_depth: usize>(
                    input: &[u8],
                    absolute_offset: usize,
                    depth: #runtime::__private::RecursiveDepth,
                ) -> Result<#runtime::Frame<Self::State>, Self::Error>
                where
                    #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
                {
                    let children = <Self as #runtime::__private::RecursiveBody<
                        #callback,
                        #marker<#root>,
                    >>::recursive_children(input, absolute_offset)?;
                    let count = usize::try_from(children.count).map_err(|_| {
                        #runtime::__private::RecursiveError::Layout(#runtime::LayoutError {
                            field: stringify!(#count_name),
                        })
                    })?;
                    let available = &input[children.prefix..];
                    let array_offset = absolute_offset.checked_add(children.prefix).ok_or(
                        #runtime::__private::RecursiveError::Layout(#runtime::LayoutError {
                            field: stringify!(#items_name),
                        }),
                    )?;
                    let mut geometry = #runtime::__private::RecursiveGeometry::new();
                    let consumed = #runtime::__private::frame_recursive_array_extent::<
                        #callback,
                        #marker<#root>,
                        #recursive_depth,
                    >(available, count, array_offset, depth, &mut geometry)
                    .map_err(|error| {
                        #runtime::__private::flatten_recursive_array_error(
                            error,
                            array_offset,
                        )
                    })?;
                    let end = children.prefix.checked_add(consumed).ok_or(
                        #runtime::__private::RecursiveError::Layout(#runtime::LayoutError {
                            field: stringify!(#items_name),
                        }),
                    )?;
                    Ok(#runtime::Frame::new(
                        #state {
                            count,
                            start: children.prefix,
                            end,
                            geometry,
                        },
                        end,
                    ))
                }

                unsafe fn from_recursive_parts<
                    '__wire_repr_recursive_view,
                    const #recursive_depth: usize,
                >(
                    input: &'__wire_repr_recursive_view [u8],
                    state: &'__wire_repr_recursive_view Self::State,
                    absolute_offset: usize,
                    depth: #runtime::__private::RecursiveDepth,
                ) -> Self::View<'__wire_repr_recursive_view, #recursive_depth>
                where
                    #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
                {
                    #body_view {
                        input,
                        state,
                        offset: absolute_offset,
                        depth,
                        marker: ::core::marker::PhantomData,
                    }
                }
            }
        });
    }
    Ok(rendered)
}
