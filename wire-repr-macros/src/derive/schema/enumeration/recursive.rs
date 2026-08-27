mod skip;
pub(super) mod writer;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Type;
use syn::ext::IdentExt;

use super::super::{fresh_type_ident, from_bytes_method, scalar_type_tokens};
use super::{EnumSchema, variant_type_stems};
pub(super) fn render_view(
    mut schema: EnumSchema,
    runtime: &TokenStream,
    recursive_slots: Vec<Option<Type>>,
) -> syn::Result<TokenStream> {
    let recursive_root = schema.name.clone();
    for variant in &mut schema.variants {
        variant.body = super::super::recursive::normalize_root_self(&variant.body, &recursive_root);
    }
    if !schema.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive enum roots cannot declare additional generics",
        ));
    }
    if schema.unknown().is_some() {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive enum roots cannot declare an unknown terminal body",
        ));
    }

    let vis = &schema.vis;
    let name = &schema.name;
    let view_trait = format_ident!("{}View", name.unraw());
    let view_error = format_ident!("{}ViewError", name.unraw());
    let variant_view = format_ident!("{}Variant", name.unraw());
    let state = format_ident!("__WireRepr{}ViewState", name.unraw());
    let view_impl = format_ident!("__WireRepr{}ViewImpl", name.unraw());
    let callback = format_ident!("__WireRepr{}RecursiveCallback", name.unraw());
    let depth = format_ident!("__WIRE_REPR_DEPTH");
    let selector_ty = scalar_type_tokens(schema.selector);
    let selector_width = schema.selector.width();
    let decode = from_bytes_method(schema.endian);
    let known = schema.known_variants().collect::<Vec<_>>();
    let primary_marker = recursive_slots
        .iter()
        .flatten()
        .next()
        .expect("recursive renderer has a recursive marker");
    let variant_lifetime = syn::Lifetime::new(
        "'__wire_repr_recursive_variant",
        proc_macro2::Span::mixed_site(),
    );
    let backing = format_ident!("__WireReprBacking");
    let holder = format_ident!("__WireReprStateHolder");

    let selector_values = known
        .iter()
        .map(|variant| variant.value.as_ref().expect("known selector"))
        .collect::<Vec<_>>();
    let selector_count = selector_values.len();
    let selector_validation = quote! {
        let _values: [#selector_ty; #selector_count] = [#(#selector_values),*];
    };
    let duplicate_checks = selector_values
        .iter()
        .enumerate()
        .flat_map(|(left, value)| {
            let selector_ty = &selector_ty;
            selector_values.iter().skip(left + 1).map(move |other| {
                quote!({
                    let left: #selector_ty = #value;
                    let right: #selector_ty = #other;
                    left == right
                })
            })
        })
        .collect::<Vec<_>>();
    let duplicate_selector = quote!(false #(|| #duplicate_checks)*);
    let type_stems = variant_type_stems(&known, false);
    let error_variant_names = type_stems
        .iter()
        .map(|stem| format_ident!("Variant{stem}"))
        .collect::<Vec<_>>();

    let state_variants = known.iter().zip(&recursive_slots).map(|(variant, slot)| {
        let variant_name = &variant.name;
        let body = &variant.body;
        if let Some(slot) = slot {
            quote!(
                #variant_name(
                    <#body as #runtime::__private::RecursiveBody<#callback, #slot>>::State
                ),
            )
        } else {
            quote!(#variant_name(<#body as #runtime::WireView>::State),)
        }
    });
    let state_declaration = quote! {
        #[doc(hidden)]
        #vis enum #state {
            #(#state_variants)*
        }
    };

    let body_view_types = known
        .iter()
        .zip(&recursive_slots)
        .map(|(variant, slot)| {
            let body = &variant.body;
            if let Some(slot) = slot {
                quote!(
                    <#body as #runtime::__private::RecursiveBody<
                        #callback,
                        #slot,
                    >>::View<#variant_lifetime, #depth>
                )
            } else {
                quote!(<#body as #runtime::WireView>::View<#variant_lifetime>)
            }
        })
        .collect::<Vec<_>>();
    let variant_declaration = {
        let variants = known.iter().zip(&body_view_types).map(|(variant, ty)| {
            let variant_name = &variant.name;
            quote!(#variant_name(#ty),)
        });
        quote! {
            #[doc = "Borrowed recursive enum variant generated for this schema."]
            #vis enum #variant_view<
                #variant_lifetime,
                const #depth: usize,
            > {
                #(#variants)*
            }
        }
    };

    let normal_errors = known
        .iter()
        .zip(&recursive_slots)
        .zip(&error_variant_names)
        .filter(|((_, slot), _)| slot.is_none())
        .map(|((variant, _), error_variant)| {
            let body = &variant.body;
            let message = format!("enum variant `{}` failed", variant.name.unraw());
            quote! {
                #[error(#message)]
                #error_variant(#[source] <#body as #runtime::WireView>::Error),
            }
        });
    let error_declaration = quote! {
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #view_error {
            #[error(transparent)]
            NeedMore(#[from] #runtime::NeedMore),
            #[error(transparent)]
            Layout(#[from] #runtime::LayoutError),
            #[error(transparent)]
            InvalidFrame(#[from] #runtime::InvalidFrameExtent),
            #[error(transparent)]
            Trailing(#[from] #runtime::TrailingBytes),
            #[error(transparent)]
            DepthExceeded(#[from] #runtime::DepthExceeded),
            #[error(transparent)]
            Recursive(#[from] #runtime::__private::RecursiveError),
            #(#normal_errors)*
            #[error("unknown selector {selector} at byte offset {offset}")]
            UnknownSelector { selector: #selector_ty, offset: usize },
            #[error("enum contains duplicate selector values")]
            DuplicateSelector,
        }
    };

    let frame_offset = format_ident!("__wire_repr_frame_offset");
    let body_depth = format_ident!("__wire_repr_body_depth");
    let frame_arms = known
        .iter()
        .zip(&recursive_slots)
        .zip(&error_variant_names)
        .map(|((variant, slot), error_variant)| {
            let variant_name = &variant.name;
            let value = variant.value.as_ref().expect("known selector");
            let body = &variant.body;
            let frame_body = if let Some(slot) = slot {
                quote! {
                    let frame = <#body as #runtime::__private::RecursiveBody<
                        #callback,
                        #slot,
                    >>::frame_recursive::<#depth>(
                        &input[#selector_width..],
                        body_offset,
                        #body_depth,
                    )
                    .map_err(|error| match error {
                        #runtime::__private::RecursiveError::DepthExceeded(source) => {
                            #view_error::DepthExceeded(source)
                        }
                        other => #view_error::Recursive(other),
                    })?;
                }
            } else {
                quote! {
                    let frame = <#body as #runtime::WireView>::frame(
                        &input[#selector_width..],
                        body_offset,
                    )
                    .map_err(#view_error::#error_variant)?;
                }
            };
            quote! {
                value if value == { let selector: #selector_ty = #value; selector } => {
                    let body_offset = #frame_offset
                        .checked_add(#selector_width)
                        .ok_or(#runtime::LayoutError {
                            field: stringify!(#variant_name),
                        })?;
                    #frame_body
                    let (body_state, body_consumed) = frame.into_parts();
                    if body_consumed > input.len() - #selector_width {
                        return Err(#view_error::InvalidFrame(#runtime::InvalidFrameExtent {
                            offset: body_offset,
                            consumed: body_consumed,
                            available: input.len() - #selector_width,
                        }));
                    }
                    let consumed = #selector_width
                        .checked_add(body_consumed)
                        .ok_or(#runtime::LayoutError {
                            field: stringify!(#variant_name),
                        })?;
                    #runtime::Frame::new(#state::#variant_name(body_state), consumed)
                }
            }
        })
        .collect::<Vec<_>>();

    let selector_arms = known.iter().map(|variant| {
        let variant_name = &variant.name;
        let value = variant.value.as_ref().expect("known selector");
        quote!(#state::#variant_name(_) => { let selector: #selector_ty = #value; selector },)
    });
    let variant_arms = known
        .iter()
        .zip(&recursive_slots)
        .zip(&body_view_types)
        .map(|((variant, slot), concrete_view)| {
            let variant_name = &variant.name;
            let body = &variant.body;
            if let Some(slot) = slot {
                quote! {
                    #state::#variant_name(state) => {
                        // SAFETY: root framing produced this local body state for the exact suffix.
                        let body: #concrete_view = unsafe {
                            <#body as #runtime::__private::RecursiveBody<
                                #callback,
                                #slot,
                            >>::from_recursive_parts::<#depth>(
                                &self.as_bytes()[#selector_width..],
                                state,
                                self.offset + #selector_width,
                                self.depth,
                            )
                        };
                        #variant_view::#variant_name(body)
                    }
                }
            } else {
                quote! {
                    #state::#variant_name(state) => {
                        // SAFETY: root framing produced this state for the exact body suffix.
                        let body = unsafe {
                            <#body as #runtime::WireView>::from_validated_parts(
                                &self.as_bytes()[#selector_width..],
                                state,
                            )
                        };
                        #variant_view::#variant_name(body)
                    }
                }
            }
        });
    let variant_type = quote!(#variant_view<#variant_lifetime, #depth>);

    let trait_declaration = quote! {
        #[doc = "Exact-source view API generated for this recursive enum schema."]
        #vis trait #view_trait<const #depth: usize>:
            AsRef<[u8]> + #runtime::ExactWire<#name>
        {
            fn as_bytes(&self) -> &[u8];
            fn selector(&self) -> #selector_ty;
            fn variant<#variant_lifetime>(
                &#variant_lifetime self,
            ) -> #variant_type;
        }
    };

    let normal_bounds = known
        .iter()
        .zip(&recursive_slots)
        .filter(|(_, slot)| slot.is_none())
        .map(|(variant, _)| {
            let body = &variant.body;
            quote!(#body: #runtime::WireView)
        })
        .collect::<Vec<_>>();
    let recursive_bounds = known
        .iter()
        .zip(&recursive_slots)
        .filter_map(|(variant, slot)| {
            slot.as_ref().map(|slot| {
                let body = &variant.body;
                quote!(#body: #runtime::__private::RecursiveBody<#callback, #slot>)
            })
        })
        .collect::<Vec<_>>();

    let render_root_frame = quote! {
        if #duplicate_selector {
            return Err(#view_error::DuplicateSelector);
        }
        if input.len() < #selector_width {
            return Err(#view_error::NeedMore(#runtime::NeedMore {
                offset: #frame_offset.saturating_add(input.len()),
                additional_at_least: #selector_width - input.len(),
            }));
        }
        let selector = #selector_ty::#decode(
            input[..#selector_width]
                .try_into()
                .expect("selector width checked"),
        );
        Ok(match selector {
            #(#frame_arms)*
            value => {
                return Err(#view_error::UnknownSelector {
                    selector: value,
                    offset: #frame_offset,
                });
            }
        })
    };

    let render_skip = skip::render(skip::Context {
        schema: &schema,
        runtime,
        known: &known,
        recursive_slots: &recursive_slots,
        error_variant_names: &error_variant_names,
        callback: &callback,
        view_error: &view_error,
        selector_ty: &selector_ty,
        selector_width,
        decode: &decode,
        depth: &depth,
    })?;

    let callback_slot = fresh_type_ident(&schema.generics, "RecursiveSlot");
    let callback_impls = core::iter::once(quote! {
        impl<#callback_slot> #runtime::__private::RecursiveFrame<#callback_slot>
            for #callback
        where
            #(#normal_bounds,)*
            #(#recursive_bounds,)*
        {
            type Root = #name;
            type State = #state;
            type Error = #view_error;
            type View<
                '__wire_repr_recursive_item,
                const #depth: usize,
            > = #view_impl<
                &'__wire_repr_recursive_item [u8],
                #state,
                #depth,
            >;

            fn frame<const #depth: usize>(
                input: &[u8],
                #frame_offset: usize,
                #body_depth: #runtime::__private::RecursiveDepth,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                #render_root_frame
            }

            #[allow(unsafe_code)]
            #[inline(always)]
            fn skip<const #depth: usize>(
                input: &[u8],
                absolute_offset: usize,
                depth: #runtime::__private::RecursiveDepth,
            ) -> Result<#runtime::__private::RecursiveMeasure, Self::Error> {
                #render_skip
            }

            #[allow(unsafe_code)]
            unsafe fn into_view<
                '__wire_repr_recursive_item,
                const #depth: usize,
            >(
                input: &'__wire_repr_recursive_item [u8],
                state: Self::State,
                absolute_offset: usize,
                body_depth: #runtime::__private::RecursiveDepth,
            ) -> Self::View<'__wire_repr_recursive_item, #depth> {
                #view_impl {
                    input,
                    represented_length: input.len(),
                    state,
                    offset: absolute_offset,
                    depth: body_depth,
                }
            }
        }
    });

    let flatten_arms = known
        .iter()
        .zip(&recursive_slots)
        .zip(&error_variant_names)
        .filter(|((_, slot), _)| slot.is_none())
        .map(|((variant, _), error_variant)| {
            let body = &variant.body;
            quote!(#view_error::#error_variant(source) => {
                <#body as #runtime::WireView>::flatten_recursive_error(
                    source,
                    fallback_offset,
                )
            },)
        });
    let flatten_impl = quote! {
        impl #runtime::__private::FlattenRecursiveError for #view_error {
            fn flatten_recursive(
                self,
                fallback_offset: usize,
            ) -> #runtime::__private::RecursiveError {
                match self {
                    #view_error::NeedMore(source) => {
                        #runtime::__private::RecursiveError::NeedMore(source)
                    }
                    #view_error::Layout(source) => {
                        #runtime::__private::RecursiveError::Layout(source)
                    }
                    #view_error::InvalidFrame(source) => {
                        #runtime::__private::RecursiveError::InvalidFrame(source)
                    }
                    #view_error::Trailing(source) => {
                        #runtime::__private::RecursiveError::Trailing(source)
                    }
                    #view_error::DepthExceeded(source) => {
                        #runtime::__private::RecursiveError::DepthExceeded(source)
                    }
                    #view_error::Recursive(source) => source,
                    #(#flatten_arms)*
                    #view_error::UnknownSelector { offset, .. } => {
                        #runtime::__private::RecursiveError::UnknownSelector { offset }
                    }
                    #view_error::DuplicateSelector => {
                        #runtime::__private::RecursiveError::Child {
                            offset: fallback_offset,
                        }
                    }
                }
            }
        }
    };

    let selector_methods = quote! {
        #[inline(always)]
        fn selector(&self) -> #selector_ty {
            match self.state.borrow() {
                #(#selector_arms)*
            }
        }

        #[inline(always)]
        fn variant<#variant_lifetime>(
            &#variant_lifetime self,
        ) -> #variant_type {
            match self.state.borrow() {
                #(#variant_arms)*
            }
        }
    };
    let view_impls = quote! {
        #[doc(hidden)]
        #vis struct #view_impl<#backing, #holder, const #depth: usize> {
            input: #backing,
            represented_length: usize,
            state: #holder,
            offset: usize,
            depth: #runtime::__private::RecursiveDepth,
        }

        impl<#backing, #holder, const #depth: usize> AsRef<[u8]>
            for #view_impl<#backing, #holder, #depth>
        where
            #backing: AsRef<[u8]>,
        {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.input.as_ref()[..self.represented_length]
            }
        }

        impl<#backing, #holder, const #depth: usize> #view_trait<#depth>
            for #view_impl<#backing, #holder, #depth>
        where
            #backing: AsRef<[u8]>,
            #holder: ::core::borrow::Borrow<#state>,
            #(#normal_bounds,)*
            #(#recursive_bounds,)*
        {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
            #selector_methods
        }

        impl<#backing, #holder, const #depth: usize> #runtime::ExactWire<#name>
            for #view_impl<#backing, #holder, #depth>
        where
            #backing: AsRef<[u8]>,
        {
            #[inline(always)]
            fn as_wire_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
        }
    };

    let root_entry = quote! {
        impl #name {
            /// Frames one exact recursive representation with a caller-selected depth.
            #vis fn view<const __WIRE_REPR_DEPTH: usize>(
                input: impl AsRef<[u8]>,
            ) -> Result<impl #view_trait<__WIRE_REPR_DEPTH>, #view_error>
            where
                #(#normal_bounds,)*
                #(#recursive_bounds,)*
            {
                #selector_validation
                let bytes = input.as_ref();
                let input_len = bytes.len();
                let body_depth = #runtime::__private::RecursiveDepth::new(
                    __WIRE_REPR_DEPTH,
                )
                .enter(0)
                .map_err(#view_error::DepthExceeded)?;
                let frame = <#callback as #runtime::__private::RecursiveFrame<
                    #primary_marker,
                >>::frame::<__WIRE_REPR_DEPTH>(bytes, 0, body_depth)?;
                let (state, consumed) = frame.into_parts();
                if consumed > input_len {
                    return Err(#view_error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed,
                        available: input_len,
                    }));
                }
                if consumed < input_len {
                    return Err(#view_error::Trailing(#runtime::TrailingBytes {
                        offset: consumed,
                        trailing: input_len - consumed,
                    }));
                }
                Ok(#view_impl {
                    input,
                    represented_length: consumed,
                    state,
                    offset: 0,
                    depth: body_depth,
                })
            }
        }
    };

    Ok(quote! {
        #state_declaration
        #variant_declaration
        #error_declaration
        #trait_declaration

        #[doc(hidden)]
        #vis struct #callback;

        #view_impls
        #flatten_impl
        #(#callback_impls)*
        #root_entry
    })
}
