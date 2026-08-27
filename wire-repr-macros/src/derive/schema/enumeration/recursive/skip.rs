use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Type};

use super::super::{EnumSchema, Variant};

pub(super) struct Context<'a> {
    pub(super) schema: &'a EnumSchema,
    pub(super) runtime: &'a TokenStream,
    pub(super) known: &'a [&'a Variant],
    pub(super) recursive_slots: &'a [Option<Type>],
    pub(super) error_variant_names: &'a [Ident],
    pub(super) callback: &'a Ident,
    pub(super) view_error: &'a Ident,
    pub(super) selector_ty: &'a TokenStream,
    pub(super) selector_width: usize,
    pub(super) decode: &'a Ident,
    pub(super) depth: &'a Ident,
}

pub(super) fn render(context: Context<'_>) -> syn::Result<TokenStream> {
    let Context {
        schema,
        runtime,
        known,
        recursive_slots,
        error_variant_names,
        callback,
        view_error,
        selector_ty,
        selector_width,
        decode,
        depth,
    } = context;
    let mut grammar_keys = Vec::<String>::new();
    let recursive_kinds = known
        .iter()
        .zip(recursive_slots)
        .map(|(variant, slot)| {
            let slot = slot.as_ref()?;
            let body = &variant.body;
            let key = quote!(#body #slot).to_string();
            let kind = grammar_keys
                .iter()
                .position(|candidate| candidate == &key)
                .unwrap_or_else(|| {
                    grammar_keys.push(key);
                    grammar_keys.len() - 1
                });
            Some(kind)
        })
        .collect::<Vec<_>>();
    let recursive_kind_count = grammar_keys.len();
    if recursive_kind_count > usize::from(u16::MAX) {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive enum has too many continuation body kinds",
        ));
    }
    let kind_suffix = if recursive_kind_count <= usize::from(u8::MAX) + 1 {
        "u8"
    } else {
        "u16"
    };
    let kind_type = if kind_suffix == "u8" {
        quote!(u8)
    } else {
        quote!(u16)
    };
    let skip_arms = known
        .iter()
        .zip(recursive_slots)
        .zip(error_variant_names)
        .enumerate()
        .map(|(index, ((variant, slot), error_variant))| {
            let variant_name = &variant.name;
            let value = variant.value.as_ref().expect("known selector");
            let body = &variant.body;
            if let Some(slot) = slot {
                let kind = recursive_kinds[index].expect("recursive variant kind");
                let kind = syn::LitInt::new(
                    &format!("{kind}{kind_suffix}"),
                    proc_macro2::Span::call_site(),
                );
                let store_kind =
                    (recursive_kind_count > 1).then(|| quote!(kinds[stack_depth].write(#kind);));
                quote! {
                    value if value == { let selector: #selector_ty = #value; selector } => {
                        let body_start = cursor
                            .checked_add(#selector_width)
                            .ok_or(#view_error::Layout(#runtime::LayoutError {
                                field: stringify!(#variant_name),
                            }))?;
                        let body_offset = absolute_offset
                            .checked_add(body_start)
                            .ok_or(#view_error::Layout(#runtime::LayoutError {
                                field: stringify!(#variant_name),
                            }))?;
                        let step = <#body as #runtime::__private::RecursiveBody<
                            #callback,
                            #slot,
                        >>::recursive_start(&input[body_start..], body_offset)
                        .map_err(|error| match error {
                            #runtime::__private::RecursiveError::DepthExceeded(source) => {
                                #view_error::DepthExceeded(source)
                            }
                            other => #view_error::Recursive(other),
                        })?;
                        if !shape_active {
                            shape = 0xcbf2_9ce4_8422_2325u64;
                            shape_active = true;
                        }
                        shape ^= selector as u64;
                        shape = shape.rotate_left(13).wrapping_add(0x9e37_79b9_7f4a_7c15);
                        cursor = body_start;
                        match step {
                            #runtime::__private::RecursiveStep::Done { advance } => {
                                cursor = cursor.checked_add(advance).ok_or(
                                    #view_error::Layout(#runtime::LayoutError {
                                        field: stringify!(#variant_name),
                                    }),
                                )?;
                            }
                            #runtime::__private::RecursiveStep::Child {
                                advance,
                                continuation,
                            } => {
                                cursor = cursor.checked_add(advance).ok_or(
                                    #view_error::Layout(#runtime::LayoutError {
                                        field: stringify!(#variant_name),
                                    }),
                                )?;
                                if cursor > input.len() {
                                    return Err(#view_error::NeedMore(#runtime::NeedMore {
                                        offset: absolute_offset.saturating_add(input.len()),
                                        additional_at_least: cursor - input.len(),
                                    }));
                                }
                                pending[stack_depth].write(continuation);
                                #store_kind
                                stack_depth += 1;
                                nested_depth = nested_depth.max(
                                    u32::try_from(stack_depth).unwrap_or(u32::MAX),
                                );
                                continue 'parse;
                            }
                        }
                        if cursor > input.len() {
                            return Err(#view_error::NeedMore(#runtime::NeedMore {
                                offset: absolute_offset.saturating_add(input.len()),
                                additional_at_least: cursor - input.len(),
                            }));
                        }
                    }
                }
            } else {
                quote! {
                    value if value == { let selector: #selector_ty = #value; selector } => {
                        let body_start = cursor
                            .checked_add(#selector_width)
                            .ok_or(#view_error::Layout(#runtime::LayoutError {
                                field: stringify!(#variant_name),
                            }))?;
                        let body_offset = absolute_offset
                            .checked_add(body_start)
                            .ok_or(#view_error::Layout(#runtime::LayoutError {
                                field: stringify!(#variant_name),
                            }))?;
                        let frame = <#body as #runtime::WireView>::frame(
                            &input[body_start..],
                            body_offset,
                        )
                        .map_err(#view_error::#error_variant)?;
                        let (_, consumed) = frame.into_parts();
                        if shape_active {
                            shape ^= selector as u64;
                            shape = shape.rotate_left(13).wrapping_add(0x9e37_79b9_7f4a_7c15);
                            shape ^= 0x1eaf_0000_0000_0000u64
                                ^ (consumed as u64).rotate_left(23);
                            shape = shape.rotate_left(13).wrapping_add(0x9e37_79b9_7f4a_7c15);
                        }
                        cursor = body_start.checked_add(consumed).ok_or(
                            #view_error::Layout(#runtime::LayoutError {
                                field: stringify!(#variant_name),
                            }),
                        )?;
                        if cursor > input.len() {
                            return Err(#view_error::InvalidFrame(
                                #runtime::InvalidFrameExtent {
                                    offset: body_offset,
                                    consumed,
                                    available: input.len() - body_start,
                                },
                            ));
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let mut emitted_kinds = vec![false; recursive_kind_count];
    let resume_arms = known
        .iter()
        .zip(recursive_slots)
        .enumerate()
        .filter_map(|(index, (variant, slot))| {
            let slot = slot.as_ref()?;
            let kind = recursive_kinds[index].expect("recursive variant kind");
            if core::mem::replace(&mut emitted_kinds[kind], true) {
                return None;
            }
            let body = &variant.body;
            let variant_name = &variant.name;
            Some(quote! {
                #kind => {
                    let body_offset = absolute_offset.checked_add(cursor).ok_or(
                        #view_error::Layout(#runtime::LayoutError {
                            field: stringify!(#variant_name),
                        }),
                    )?;
                    <#body as #runtime::__private::RecursiveBody<
                        #callback,
                        #slot,
                    >>::recursive_resume(&input[cursor..], body_offset, continuation)
                    .map_err(|error| match error {
                        #runtime::__private::RecursiveError::DepthExceeded(source) => {
                            #view_error::DepthExceeded(source)
                        }
                        other => #view_error::Recursive(other),
                    })?
                }
            })
        })
        .collect::<Vec<_>>();
    let kind_storage = (recursive_kind_count > 1).then(|| {
        quote! {
            let mut kinds = [
                ::core::mem::MaybeUninit::<#kind_type>::uninit();
                #depth
            ];
        }
    });
    let read_kind = if recursive_kind_count > 1 {
        quote! {
            // SAFETY: the kind slot is initialized before `stack_depth` includes this frame.
            let kind = usize::from(unsafe { kinds[frame].assume_init() });
        }
    } else {
        quote!(let kind = 0usize;)
    };
    let render_skip = quote! {
        let mut pending = [
            ::core::mem::MaybeUninit::<u32>::uninit();
            #depth
        ];
        #kind_storage
        let mut stack_depth = 0usize;
        let mut cursor = 0usize;
        let mut shape = 0u64;
        let mut shape_active = false;
        let mut nested_depth = 0u32;
        'parse: loop {
            if stack_depth >= depth.remaining() || stack_depth >= #depth {
                return Err(#view_error::DepthExceeded(#runtime::DepthExceeded {
                    limit: depth.limit(),
                    offset: absolute_offset.saturating_add(cursor),
                }));
            }
            if input.len() - cursor < #selector_width {
                return Err(#view_error::NeedMore(#runtime::NeedMore {
                    offset: absolute_offset.saturating_add(input.len()),
                    additional_at_least: #selector_width - (input.len() - cursor),
                }));
            }
            let selector = #selector_ty::#decode(
                input[cursor..cursor + #selector_width]
                    .try_into()
                    .expect("selector width checked"),
            );
            match selector {
                #(#skip_arms)*
                value => {
                    return Err(#view_error::UnknownSelector {
                        selector: value,
                        offset: absolute_offset.saturating_add(cursor),
                    });
                }
            }

            loop {
                if stack_depth == 0 {
                    return Ok(#runtime::__private::RecursiveMeasure {
                        consumed: cursor,
                        shape,
                        nested_depth,
                    });
                }
                let frame = stack_depth - 1;
                // SAFETY: both slots are initialized before `stack_depth` includes this frame.
                let continuation = unsafe { pending[frame].assume_init() };
                #read_kind
                let step = match kind {
                    #(#resume_arms,)*
                    _ => {
                        return Err(#view_error::Layout(#runtime::LayoutError {
                            field: "recursive continuation",
                        }));
                    }
                };
                match step {
                    #runtime::__private::RecursiveStep::Done { advance } => {
                        cursor = cursor.checked_add(advance).ok_or(
                            #view_error::Layout(#runtime::LayoutError {
                                field: "recursive continuation",
                            }),
                        )?;
                        if cursor > input.len() {
                            return Err(#view_error::NeedMore(#runtime::NeedMore {
                                offset: absolute_offset.saturating_add(input.len()),
                                additional_at_least: cursor - input.len(),
                            }));
                        }
                        stack_depth -= 1;
                    }
                    #runtime::__private::RecursiveStep::Child {
                        advance,
                        continuation,
                    } => {
                        cursor = cursor.checked_add(advance).ok_or(
                            #view_error::Layout(#runtime::LayoutError {
                                field: "recursive continuation",
                            }),
                        )?;
                        if cursor > input.len() {
                            return Err(#view_error::NeedMore(#runtime::NeedMore {
                                offset: absolute_offset.saturating_add(input.len()),
                                additional_at_least: cursor - input.len(),
                            }));
                        }
                        pending[frame].write(continuation);
                        nested_depth = nested_depth
                            .max(u32::try_from(stack_depth).unwrap_or(u32::MAX));
                        continue 'parse;
                    }
                }
            }
        }
    };
    Ok(render_skip)
}
