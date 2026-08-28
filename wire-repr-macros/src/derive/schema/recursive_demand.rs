use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericParam, Ident};

use super::model::{DynamicExtent, FieldKind, Schema};

pub(super) fn render(
    schema: &Schema,
    slot_index: usize,
    marker: &Ident,
    root: &Ident,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let demand = validate(schema, root)?;
    let vis = &schema.vis;
    let name = &schema.name;
    let (_, schema_types, schema_where) = schema.generics.split_for_impl();
    let schema_impl = &schema.generics.split_for_impl().0;
    let self_type = quote!(#name #schema_types);
    let state = format_ident!("{}DemandState", marker);
    let continuation = format_ident!("{}DemandContinuation", marker);
    let body_view = format_ident!("{}DemandView", marker);
    let callback = super::fresh_type_ident(&schema.generics, "RecursiveCallback");
    let depth = super::fresh_type_ident(&schema.generics, "RecursiveDepth");
    let mut impl_generics = schema.generics.clone();
    impl_generics
        .params
        .push(GenericParam::Type(syn::parse_quote!(#callback)));
    let (body_impl, _, body_where) = impl_generics.split_for_impl();
    let controller_name = &schema.fields[demand.controller].name;
    let left_name = &schema.fields[demand.left].name;
    let bytes_name = &schema.fields[demand.bytes].name;
    let scalar_name = &schema.fields[demand.scalar].name;
    let right_name = &schema.fields[demand.right].name;
    let controller = demand.controller_scalar;
    let controller_wire = super::scalar_type_tokens(controller.wire_type);
    let controller_decode = super::from_bytes_method(controller.endian);
    let controller_width = controller.width();
    let scalar = demand.scalar_field;
    let scalar_ty = super::value_type_tokens(scalar.value_type);
    let scalar_wire = super::scalar_type_tokens(scalar.wire_type);
    let scalar_decode = super::from_bytes_method(scalar.endian);
    let scalar_width = scalar.width();
    let scalar_layout = &schema.fields[demand.scalar].layout;
    let scalar_pad = scalar_layout
        .pad_before
        .as_ref()
        .map(|pad| quote!(#pad))
        .unwrap_or_else(|| quote!(0usize));
    let scalar_align = scalar_layout.align_before.as_ref();
    let needs_body_start = scalar_align.is_some();
    let resume_layout = if let Some(align) = scalar_align {
        quote! {
            let bytes_end = absolute_offset.checked_add(extent).ok_or(
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ),
            )?;
            let mut scalar_relative = bytes_end
                .checked_sub(continuation.body_start())
                .and_then(|offset| offset.checked_add(#scalar_pad))
                .ok_or(#runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ))?;
            scalar_relative = #runtime::__private::checked_align(scalar_relative, #align)
                .ok_or(#runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ))?;
            let scalar_absolute = continuation
                .body_start()
                .checked_add(scalar_relative)
                .ok_or(#runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ))?;
        }
    } else {
        quote! {
            let scalar_absolute = absolute_offset
                .checked_add(extent)
                .and_then(|offset| offset.checked_add(#scalar_pad))
                .ok_or(#runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ))?;
        }
    };
    let frame_layout = if let Some(align) = scalar_align {
        quote! {
            let mut scalar_start = bytes_end.checked_add(#scalar_pad).ok_or(
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ),
            )?;
            scalar_start = #runtime::__private::checked_align(scalar_start, #align)
                .ok_or(#runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ))?;
        }
    } else {
        quote! {
            let scalar_start = bytes_end.checked_add(#scalar_pad).ok_or(
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: stringify!(#scalar_name) },
                ),
            )?;
        }
    };
    let body_start_parameter = needs_body_start.then(|| quote!(, body_start: usize));
    let body_start_argument = needs_body_start.then(|| quote!(, absolute_offset));
    let body_start_extra = needs_body_start.then(|| quote!(+ ::core::mem::size_of::<usize>()));
    let continuation_width = quote!(1usize + #controller_width #body_start_extra);
    let pack_body_start = needs_body_start.then(|| {
        quote! {
            packed[1 + #controller_width..].copy_from_slice(&body_start.to_ne_bytes());
        }
    });
    let body_start_method = needs_body_start.then(|| {
        quote! {
            #[inline(always)]
            fn body_start(self) -> usize {
                usize::from_ne_bytes(
                    self.0[1 + #controller_width..]
                        .try_into()
                        .expect("packed recursive body start width"),
                )
            }
        }
    });

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

        impl #schema_impl #runtime::__private::RecursiveSlot<#slot_index>
            for #self_type #schema_where
        {
            type Marker = #marker<#root>;
        }

        #[doc(hidden)]
        #[derive(Clone, Copy)]
        #[repr(transparent)]
        #vis struct #continuation([u8; #continuation_width]);

        impl #continuation {

            #[inline(always)]
            fn after_left(controller: #controller_wire #body_start_parameter) -> Self {
                let mut packed = [0u8; #continuation_width];
                packed[0] = 1;
                packed[1..1 + #controller_width]
                    .copy_from_slice(&controller.to_ne_bytes());
                #pack_body_start
                Self(packed)
            }

            #[inline(always)]
            fn after_right() -> Self {
                let mut packed = [0u8; #continuation_width];
                packed[0] = 2;
                Self(packed)
            }

            #[inline(always)]
            fn resume(self) -> u8 {
                self.0[0]
            }

            #[inline(always)]
            fn controller(self) -> #controller_wire {
                #controller_wire::from_ne_bytes(
                    self.0[1..1 + #controller_width]
                        .try_into()
                        .expect("packed recursive controller width"),
                )
            }

            #body_start_method
        }

        #[doc(hidden)]
        #vis struct #state {
            offsets: [usize; 7],
        }

        #[doc(hidden)]
        #vis struct #body_view<
            '__wire_repr_recursive_view,
            #callback,
            #root,
            const #depth: usize,
        > {
            input: &'__wire_repr_recursive_view [u8],
            state: &'__wire_repr_recursive_view #state,
            absolute_offset: usize,
            depth: #runtime::__private::RecursiveDepth,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        impl<'__wire_repr_recursive_view, #callback, #root, const #depth: usize>
            AsRef<[u8]> for #body_view<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                #depth,
            >
        {
            fn as_ref(&self) -> &[u8] {
                &self.input[..self.state.offsets[6]]
            }
        }

        impl<'__wire_repr_recursive_view, #callback, #root, const #depth: usize>
            #body_view<'__wire_repr_recursive_view, #callback, #root, #depth>
        where
            #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
        {
            pub fn #controller_name(&self) -> #controller_wire {
                let bytes: [u8; #controller_width] = self.input[
                    self.state.offsets[0]..self.state.offsets[1]
                ]
                    .try_into()
                    .expect("framed recursive controller width");
                #controller_wire::#controller_decode(bytes)
            }

            pub fn #left_name(&self) -> Result<
                <#callback as #runtime::__private::RecursiveFrame<#marker<#root>>>::View<'_, #depth>,
                #runtime::__private::RecursiveError,
            > {
                self.child(self.state.offsets[1], self.state.offsets[2], stringify!(#left_name))
            }

            pub fn #bytes_name(&self) -> &[u8] {
                &self.input[self.state.offsets[2]..self.state.offsets[3]]
            }

            pub fn #scalar_name(&self) -> #scalar_ty {
                let bytes: [u8; #scalar_width] = self.input[
                    self.state.offsets[4]..self.state.offsets[5]
                ]
                    .try_into()
                    .expect("framed recursive scalar width");
                #scalar_wire::#scalar_decode(bytes)
            }

            pub fn #right_name(&self) -> Result<
                <#callback as #runtime::__private::RecursiveFrame<#marker<#root>>>::View<'_, #depth>,
                #runtime::__private::RecursiveError,
            > {
                self.child(self.state.offsets[5], self.state.offsets[6], stringify!(#right_name))
            }

            fn child(
                &self,
                start: usize,
                end: usize,
                field: &'static str,
            ) -> Result<
                <#callback as #runtime::__private::RecursiveFrame<#marker<#root>>>::View<'_, #depth>,
                #runtime::__private::RecursiveError,
            > {
                let absolute = self.absolute_offset.checked_add(start).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field },
                    ),
                )?;
                let body_depth = self.depth.enter(absolute).map_err(
                    #runtime::__private::RecursiveError::DepthExceeded,
                )?;
                let input = &self.input[start..end];
                let frame = <#callback as #runtime::__private::RecursiveFrame<
                    #marker<#root>,
                >>::frame::<#depth>(input, absolute, body_depth)
                .map_err(|source| {
                    #runtime::__private::FlattenRecursiveError::flatten_recursive(source, absolute)
                })?;
                let (state, consumed) = frame.into_parts();
                if consumed != input.len() {
                    return Err(#runtime::__private::RecursiveError::InvalidFrame(
                        #runtime::InvalidFrameExtent {
                            offset: absolute,
                            consumed,
                            available: input.len(),
                        },
                    ));
                }
                // SAFETY: state was framed from this exact child span, offset, and depth.
                Ok(unsafe {
                    <#callback as #runtime::__private::RecursiveFrame<
                        #marker<#root>,
                    >>::into_view::<#depth>(input, state, absolute, body_depth)
                })
            }
        }

        impl #body_impl #runtime::__private::RecursiveBody<#callback, #marker<#root>>
            for #self_type #body_where
        {
            type State = #state;
            type Continuation = #continuation;
            type Error = #runtime::__private::RecursiveError;
            type View<'__wire_repr_recursive_view, const #depth: usize>
                = #body_view<'__wire_repr_recursive_view, #callback, #root, #depth>;

            fn recursive_start(
                input: &[u8],
                absolute_offset: usize,
            ) -> Result<
                #runtime::__private::RecursiveStep<Self::Continuation>,
                Self::Error,
            > {
                let bytes: [u8; #controller_width] = input
                    .get(..#controller_width)
                    .ok_or(#runtime::__private::RecursiveError::NeedMore(
                        #runtime::NeedMore {
                            offset: absolute_offset.saturating_add(input.len()),
                            additional_at_least: #controller_width.saturating_sub(input.len()),
                        },
                    ))?
                    .try_into()
                    .expect("controller width checked");
                let controller = #controller_wire::#controller_decode(bytes);
                Ok(#runtime::__private::RecursiveStep::Child {
                    advance: #controller_width,
                    continuation: #continuation::after_left(controller #body_start_argument),
                })
            }

            fn recursive_resume(
                input: &[u8],
                absolute_offset: usize,
                continuation: Self::Continuation,
            ) -> Result<
                #runtime::__private::RecursiveStep<Self::Continuation>,
                Self::Error,
            > {
                match continuation.resume() {
                    1 => {
                        let extent = usize::try_from(continuation.controller()).map_err(|_| {
                            #runtime::__private::RecursiveError::Layout(
                                #runtime::LayoutError { field: stringify!(#bytes_name) },
                            )
                        })?;
                        #resume_layout
                        let advance = scalar_absolute
                            .checked_sub(absolute_offset)
                            .and_then(|offset| offset.checked_add(#scalar_width))
                            .ok_or(#runtime::__private::RecursiveError::Layout(
                                #runtime::LayoutError { field: stringify!(#scalar_name) },
                            ))?;
                        if input.len() < advance {
                            return Err(#runtime::__private::RecursiveError::NeedMore(
                                #runtime::NeedMore {
                                    offset: absolute_offset.saturating_add(input.len()),
                                    additional_at_least: advance - input.len(),
                                },
                            ));
                        }
                        Ok(#runtime::__private::RecursiveStep::Child {
                            advance,
                            continuation: #continuation::after_right(),
                        })
                    }
                    2 => Ok(#runtime::__private::RecursiveStep::Done { advance: 0 }),
                    _ => Err(#runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: "recursive continuation" },
                    )),
                }
            }

            fn frame_recursive<const #depth: usize>(
                input: &[u8],
                absolute_offset: usize,
                depth: #runtime::__private::RecursiveDepth,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error>
            where
                #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
            {
                let mut offsets = [0usize; 7];
                let controller_end = #controller_width;
                let controller_bytes: [u8; #controller_width] = input
                    .get(..controller_end)
                    .ok_or(#runtime::__private::RecursiveError::NeedMore(
                        #runtime::NeedMore {
                            offset: absolute_offset.saturating_add(input.len()),
                            additional_at_least: controller_end.saturating_sub(input.len()),
                        },
                    ))?
                    .try_into()
                    .expect("controller width checked");
                let controller = #controller_wire::#controller_decode(controller_bytes);
                offsets[1] = controller_end;

                let left_offset = absolute_offset.checked_add(controller_end).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#left_name) },
                    ),
                )?;
                let left = <#callback as #runtime::__private::RecursiveFrame<
                    #marker<#root>,
                >>::skip::<#depth>(&input[controller_end..], left_offset, depth)
                .map_err(|source| {
                    #runtime::__private::FlattenRecursiveError::flatten_recursive(
                        source,
                        left_offset,
                    )
                })?;
                let left_end = controller_end.checked_add(left.consumed).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#left_name) },
                    ),
                )?;
                offsets[2] = left_end;

                let extent = usize::try_from(controller).map_err(|_| {
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#bytes_name) },
                    )
                })?;
                let bytes_end = left_end.checked_add(extent).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#bytes_name) },
                    ),
                )?;
                if bytes_end > input.len() {
                    return Err(#runtime::__private::RecursiveError::NeedMore(
                        #runtime::NeedMore {
                            offset: absolute_offset.saturating_add(input.len()),
                            additional_at_least: bytes_end - input.len(),
                        },
                    ));
                }
                offsets[3] = bytes_end;

                #frame_layout
                let scalar_end = scalar_start.checked_add(#scalar_width).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#scalar_name) },
                    ),
                )?;
                if scalar_end > input.len() {
                    return Err(#runtime::__private::RecursiveError::NeedMore(
                        #runtime::NeedMore {
                            offset: absolute_offset.saturating_add(input.len()),
                            additional_at_least: scalar_end - input.len(),
                        },
                    ));
                }
                offsets[4] = scalar_start;
                offsets[5] = scalar_end;

                let right_offset = absolute_offset.checked_add(scalar_end).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#right_name) },
                    ),
                )?;
                let right = <#callback as #runtime::__private::RecursiveFrame<
                    #marker<#root>,
                >>::skip::<#depth>(&input[scalar_end..], right_offset, depth)
                .map_err(|source| {
                    #runtime::__private::FlattenRecursiveError::flatten_recursive(
                        source,
                        right_offset,
                    )
                })?;
                let right_end = scalar_end.checked_add(right.consumed).ok_or(
                    #runtime::__private::RecursiveError::Layout(
                        #runtime::LayoutError { field: stringify!(#right_name) },
                    ),
                )?;
                if right_end > input.len() {
                    return Err(#runtime::__private::RecursiveError::InvalidFrame(
                        #runtime::InvalidFrameExtent {
                            offset: right_offset,
                            consumed: right.consumed,
                            available: input.len().saturating_sub(scalar_end),
                        },
                    ));
                }
                offsets[6] = right_end;
                Ok(#runtime::Frame::new(#state { offsets }, right_end))
            }

            // SAFETY: callers must supply state produced by `frame_recursive` for this exact span.
            unsafe fn from_recursive_parts<
                '__wire_repr_recursive_view,
                const #depth: usize,
            >(
                input: &'__wire_repr_recursive_view [u8],
                state: &'__wire_repr_recursive_view Self::State,
                absolute_offset: usize,
                depth: #runtime::__private::RecursiveDepth,
            ) -> Self::View<'__wire_repr_recursive_view, #depth>
            where
                #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
            {
                #body_view {
                    input,
                    state,
                    absolute_offset,
                    depth,
                    marker: ::core::marker::PhantomData,
                }
            }
        }
    })
}

pub(super) struct Demand<'a> {
    pub(super) controller: usize,
    pub(super) left: usize,
    pub(super) bytes: usize,
    pub(super) scalar: usize,
    pub(super) right: usize,
    pub(super) controller_scalar: &'a super::model::Scalar,
    pub(super) scalar_field: &'a super::model::Scalar,
}

pub(super) fn validate<'a>(schema: &'a Schema, root: &Ident) -> syn::Result<Demand<'a>> {
    if !schema.validators.is_empty() || schema.fields.len() != 5 {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive demand body currently requires controller, child, bounded bytes, scalar, child",
        ));
    }
    let FieldKind::Scalar(controller_scalar) = &schema.fields[0].kind else {
        return Err(syn::Error::new_spanned(
            &schema.fields[0].ty,
            "recursive demand controller must be an unsigned scalar",
        ));
    };
    if !controller_scalar.wire_type.is_unsigned_integer()
        || controller_scalar.value_type.is_converted()
        || controller_scalar.constant.is_some()
        || controller_scalar.computed.is_some()
    {
        return Err(syn::Error::new_spanned(
            &schema.fields[0].ty,
            "recursive demand controller must be a stored unsigned scalar",
        ));
    }
    let FieldKind::Recursive(left) = &schema.fields[1].kind else {
        return Err(syn::Error::new_spanned(
            &schema.fields[1].ty,
            "expected recursive child",
        ));
    };
    let FieldKind::RawBytes(bytes) = &schema.fields[2].kind else {
        return Err(syn::Error::new_spanned(
            &schema.fields[2].ty,
            "expected bounded bytes",
        ));
    };
    let DynamicExtent::Bounded(controller) = &bytes.extent else {
        return Err(syn::Error::new_spanned(
            &schema.fields[2].ty,
            "expected bounded bytes",
        ));
    };
    let FieldKind::Scalar(scalar_field) = &schema.fields[3].kind else {
        return Err(syn::Error::new_spanned(
            &schema.fields[3].ty,
            "expected fixed scalar",
        ));
    };
    if scalar_field.value_type.is_converted()
        || scalar_field.constant.is_some()
        || scalar_field.computed.is_some()
    {
        return Err(syn::Error::new_spanned(
            &schema.fields[3].ty,
            "recursive demand scalar must use its direct stored representation",
        ));
    }
    let FieldKind::Recursive(right) = &schema.fields[4].kind else {
        return Err(syn::Error::new_spanned(
            &schema.fields[4].ty,
            "expected recursive child",
        ));
    };
    if !type_is_parameter(&left.root, root) || !type_is_parameter(&right.root, root) {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive demand children must use the direct root parameter",
        ));
    }
    if schema.fields[0].name != *controller {
        return Err(syn::Error::new_spanned(
            controller,
            "recursive demand bytes must use the leading controller",
        ));
    }
    if schema
        .fields
        .iter()
        .any(|field| field.layout.condition.is_some() || field.layout.position.is_some())
        || schema.fields.iter().enumerate().any(|(index, field)| {
            index != 3 && (field.layout.pad_before.is_some() || field.layout.align_before.is_some())
        })
    {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive demand placement is supported only as padding/alignment before the fixed scalar",
        ));
    }
    Ok(Demand {
        controller: 0,
        left: 1,
        bytes: 2,
        scalar: 3,
        right: 4,
        controller_scalar,
        scalar_field,
    })
}

fn type_is_parameter(ty: &syn::Type, parameter: &Ident) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.len() == 1
        && path.path.segments[0].ident == *parameter
        && matches!(path.path.segments[0].arguments, syn::PathArguments::None)
}
