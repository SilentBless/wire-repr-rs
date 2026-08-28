use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericParam, Ident};

use super::model::{Field, FieldKind, Schema};

pub(super) fn render(
    schema: &Schema,
    slot_index: usize,
    marker: &Ident,
    root: &Ident,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    if schema
        .fields
        .iter()
        .any(|field| matches!(field.kind, FieldKind::RawBytes(_)))
    {
        return super::recursive_demand::render(schema, slot_index, marker, root, runtime);
    }
    validate(schema, root)?;

    let name = &schema.name;
    let vis = &schema.vis;
    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let state = format_ident!("{}State", marker);
    let body_view = format_ident!("{}View", marker);
    let callback = super::fresh_type_ident(&schema.generics, "RecursiveCallback");
    let depth = super::fresh_type_ident(&schema.generics, "RecursiveDepth");
    let mut impl_generics = schema.generics.clone();
    impl_generics
        .params
        .push(GenericParam::Type(syn::parse_quote!(#callback)));
    let (body_impl, _, body_where) = impl_generics.split_for_impl();
    let field_count = schema.fields.len();
    let offsets_len = field_count + 1;

    let frame_steps = render_frame_steps(schema, &callback, marker, root, &depth, runtime);
    let getters = render_getters(schema, &callback, marker, root, &depth, runtime);
    let start_step = render_transition(schema, 0, runtime);
    let resume_arms = recursive_positions(schema)
        .enumerate()
        .map(|(recursive_index, field_index)| {
            let continuation = u32::try_from(recursive_index + 1)
                .expect("recursive field count fits u32 after validation");
            let transition = render_transition(schema, field_index + 1, runtime);
            quote!(#continuation => { #transition })
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

        impl #schema_impl #runtime::__private::RecursiveSlot<#slot_index>
            for #self_type #schema_where
        {
            type Marker = #marker<#root>;
        }

        #[doc(hidden)]
        #vis struct #state {
            offsets: [usize; #offsets_len],
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
                &self.input[..self.state.offsets[#field_count]]
            }
        }

        impl<'__wire_repr_recursive_view, #callback, #root, const #depth: usize>
            #body_view<'__wire_repr_recursive_view, #callback, #root, #depth>
        where
            #callback: #runtime::__private::RecursiveFrame<#marker<#root>>,
        {
            #(#getters)*
        }

        impl #body_impl #runtime::__private::RecursiveBody<#callback, #marker<#root>>
            for #self_type #body_where
        {
            type State = #state;
            type Continuation = u32;
            type Error = #runtime::__private::RecursiveError;
            type View<
                '__wire_repr_recursive_view,
                const #depth: usize,
            > = #body_view<
                '__wire_repr_recursive_view,
                #callback,
                #root,
                #depth,
            >;

            fn recursive_start(
                input: &[u8],
                absolute_offset: usize,
            ) -> Result<
                #runtime::__private::RecursiveStep<Self::Continuation>,
                Self::Error,
            > {
                #start_step
            }

            fn recursive_resume(
                input: &[u8],
                absolute_offset: usize,
                continuation: Self::Continuation,
            ) -> Result<
                #runtime::__private::RecursiveStep<Self::Continuation>,
                Self::Error,
            > {
                match continuation {
                    #(#resume_arms,)*
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
                let mut offsets = [0usize; #offsets_len];
                let mut cursor = 0usize;
                #(#frame_steps)*
                offsets[#field_count] = cursor;
                Ok(#runtime::Frame::new(#state { offsets }, cursor))
            }

            // SAFETY: callers must supply state produced by `frame_recursive` for this exact
            // input span, offset, and recursive depth.
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

pub(super) fn validate(schema: &Schema, root: &Ident) -> syn::Result<()> {
    if !schema.validators.is_empty() {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive object bodies currently do not support schema validators",
        ));
    }
    let recursive_count = recursive_positions(schema).count();
    if recursive_count == 0 {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive object body requires at least one wire::Recursive<T> field",
        ));
    }
    if recursive_count > u32::MAX as usize {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive object body has too many continuation fields",
        ));
    }
    for field in &schema.fields {
        if field.layout.condition.is_some()
            || field.layout.position.is_some()
            || field.layout.pad_before.is_some()
            || field.layout.align_before.is_some()
        {
            return Err(syn::Error::new_spanned(
                &field.name,
                "recursive object bodies currently require fixed sequential nonrecursive fields",
            ));
        }
        match &field.kind {
            FieldKind::Recursive(recursive) if type_is_parameter(&recursive.root, root) => {}
            FieldKind::Recursive(_) => {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "recursive object field must name the body's direct root type parameter",
                ));
            }
            FieldKind::Scalar(scalar)
                if scalar.constant.is_none()
                    && scalar.computed.is_none()
                    && !scalar.value_type.is_converted() => {}
            FieldKind::Bytes(bytes) if bytes.constant.is_none() => {}
            _ => {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "recursive object bodies currently support recursive markers, plain fixed scalars, and fixed byte arrays",
                ));
            }
        }
    }
    Ok(())
}

fn recursive_positions(schema: &Schema) -> impl Iterator<Item = usize> + '_ {
    schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| matches!(field.kind, FieldKind::Recursive(_)).then_some(index))
}

fn render_transition(schema: &Schema, start: usize, runtime: &TokenStream) -> TokenStream {
    let next = schema.fields[start..]
        .iter()
        .position(|field| matches!(field.kind, FieldKind::Recursive(_)))
        .map(|relative| start + relative);
    let end = next.unwrap_or(schema.fields.len());
    let widths = schema.fields[start..end]
        .iter()
        .map(field_width)
        .collect::<Vec<_>>();
    let field_name = schema.fields[start..end]
        .first()
        .or_else(|| schema.fields.get(start.saturating_sub(1)))
        .map(|field| field.name.to_string())
        .unwrap_or_else(|| "recursive object".to_owned());
    let result = if let Some(field_index) = next {
        let continuation = u32::try_from(
            recursive_positions(schema)
                .position(|index| index == field_index)
                .expect("recursive field is inventoried")
                + 1,
        )
        .expect("validated recursive count");
        quote!(#runtime::__private::RecursiveStep::Child { advance, continuation: #continuation })
    } else {
        quote!(#runtime::__private::RecursiveStep::Done { advance })
    };
    quote! {
        let mut advance = 0usize;
        #(
            advance = advance.checked_add(#widths).ok_or(
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field: #field_name },
                ),
            )?;
        )*
        if input.len() < advance {
            return Err(#runtime::__private::RecursiveError::NeedMore(#runtime::NeedMore {
                offset: absolute_offset.saturating_add(input.len()),
                additional_at_least: advance - input.len(),
            }));
        }
        Ok(#result)
    }
}

fn render_frame_steps(
    schema: &Schema,
    callback: &Ident,
    marker: &Ident,
    root: &Ident,
    depth: &Ident,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let field_name = field.name.to_string();
            match &field.kind {
                FieldKind::Recursive(_) => quote! {
                    offsets[#index] = cursor;
                    let absolute = absolute_offset.checked_add(cursor).ok_or(
                        #runtime::__private::RecursiveError::Layout(
                            #runtime::LayoutError { field: #field_name },
                        ),
                    )?;
                    let measure = <#callback as #runtime::__private::RecursiveFrame<
                        #marker<#root>,
                    >>::skip::<#depth>(&input[cursor..], absolute, depth)
                    .map_err(|source| {
                        #runtime::__private::FlattenRecursiveError::flatten_recursive(
                            source,
                            absolute,
                        )
                    })?;
                    if measure.consumed == 0 {
                        return Err(#runtime::__private::RecursiveError::Child { offset: absolute });
                    }
                    cursor = cursor.checked_add(measure.consumed).ok_or(
                        #runtime::__private::RecursiveError::Layout(
                            #runtime::LayoutError { field: #field_name },
                        ),
                    )?;
                    if cursor > input.len() {
                        return Err(#runtime::__private::RecursiveError::InvalidFrame(
                            #runtime::InvalidFrameExtent {
                                offset: absolute,
                                consumed: measure.consumed,
                                available: input.len().saturating_sub(offsets[#index]),
                            },
                        ));
                    }
                },
                FieldKind::Scalar(_) | FieldKind::Bytes(_) => {
                    let width = field_width(field);
                    quote! {
                        offsets[#index] = cursor;
                        let end = cursor.checked_add(#width).ok_or(
                            #runtime::__private::RecursiveError::Layout(
                                #runtime::LayoutError { field: #field_name },
                            ),
                        )?;
                        if end > input.len() {
                            return Err(#runtime::__private::RecursiveError::NeedMore(
                                #runtime::NeedMore {
                                    offset: absolute_offset.saturating_add(input.len()),
                                    additional_at_least: end - input.len(),
                                },
                            ));
                        }
                        cursor = end;
                    }
                }
                _ => unreachable!("validated recursive object field"),
            }
        })
        .collect()
}

fn render_getters(
    schema: &Schema,
    callback: &Ident,
    marker: &Ident,
    root: &Ident,
    depth: &Ident,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.name;
            let next = index + 1;
            match &field.kind {
                FieldKind::Recursive(_) => quote! {
                    pub fn #name(&self) -> Result<
                        <#callback as #runtime::__private::RecursiveFrame<#marker<#root>>>::View<'_, #depth>,
                        #runtime::__private::RecursiveError,
                    > {
                        let start = self.state.offsets[#index];
                        let end = self.state.offsets[#next];
                        let absolute = self.absolute_offset.checked_add(start).ok_or(
                            #runtime::__private::RecursiveError::Layout(
                                #runtime::LayoutError { field: stringify!(#name) },
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
                            #runtime::__private::FlattenRecursiveError::flatten_recursive(
                                source,
                                absolute,
                            )
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
                },
                FieldKind::Scalar(scalar) => {
                    let width = scalar.width();
                    let ty = super::value_type_tokens(&scalar.value_type);
                    let wire_ty = super::scalar_type_tokens(scalar.wire_type);
                    let decode = super::from_bytes_method(scalar.endian);
                    quote! {
                        pub fn #name(&self) -> #ty {
                            let start = self.state.offsets[#index];
                            let bytes: [u8; #width] = self.input[start..self.state.offsets[#next]]
                                .try_into()
                                .expect("framed recursive scalar width");
                            #wire_ty::#decode(bytes)
                        }
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    quote! {
                        pub fn #name(&self) -> #ty {
                            self.input[self.state.offsets[#index]..self.state.offsets[#next]]
                                .try_into()
                                .expect("framed recursive byte-array width")
                        }
                    }
                }
                _ => unreachable!("validated recursive object field"),
            }
        })
        .collect()
}

fn field_width(field: &Field) -> TokenStream {
    match &field.kind {
        FieldKind::Scalar(scalar) => {
            let width = scalar.width();
            quote!(#width)
        }
        FieldKind::Bytes(bytes) => {
            let width = &bytes.len;
            quote!(#width)
        }
        FieldKind::Recursive(_) => quote!(0usize),
        _ => unreachable!("validated recursive object field"),
    }
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
