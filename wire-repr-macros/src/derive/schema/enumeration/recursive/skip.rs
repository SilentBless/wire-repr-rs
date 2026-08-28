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
    let mut grammar_entries = Vec::<(&Type, &Type)>::new();
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
                    grammar_entries.push((body, slot));
                    grammar_keys.len() - 1
                });
            Some(kind)
        })
        .collect::<Vec<_>>();
    let recursive_kind_count = grammar_entries.len();
    let continuation = quote::format_ident!(
        "__WireRepr{}Continuation",
        schema.name,
        span = proc_macro2::Span::mixed_site(),
    );
    let continuation_variants = grammar_entries
        .iter()
        .enumerate()
        .map(|(kind, (body, slot))| {
            let variant = quote::format_ident!("Kind{kind}");
            quote! {
                #variant(
                    <#body as #runtime::__private::RecursiveBody<
                        #callback,
                        #slot,
                    >>::Continuation,
                )
            }
        })
        .collect::<Vec<_>>();
    let continuation_declaration = if recursive_kind_count == 1 {
        let (body, slot) = grammar_entries[0];
        quote! {
            type #continuation =
                <#body as #runtime::__private::RecursiveBody<
                    #callback,
                    #slot,
                >>::Continuation;
        }
    } else {
        quote! {
            #[derive(Clone, Copy)]
            enum #continuation {
                #(#continuation_variants,)*
            }
        }
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
                let stored_continuation = if recursive_kind_count == 1 {
                    quote!(continuation)
                } else {
                    let variant = quote::format_ident!("Kind{kind}");
                    quote!(#continuation::#variant(continuation))
                };
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
                                pending[stack_depth].write(#stored_continuation);
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
    let mut single_resume = None;
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
            let call = quote! {
                {
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
            };
            if recursive_kind_count == 1 {
                single_resume = Some(call);
                None
            } else {
                let variant = quote::format_ident!("Kind{kind}");
                Some(quote! {
                    #continuation::#variant(continuation) => {
                        match #call {
                            #runtime::__private::RecursiveStep::Done { advance } => {
                                #runtime::__private::RecursiveStep::Done { advance }
                            }
                            #runtime::__private::RecursiveStep::Child {
                                advance,
                                continuation: next,
                            } => {
                                #runtime::__private::RecursiveStep::Child {
                                    advance,
                                    continuation: #continuation::#variant(next),
                                }
                            }
                        }
                    }
                })
            }
        })
        .collect::<Vec<_>>();
    let resume_dispatch = if recursive_kind_count == 1 {
        single_resume.expect("one recursive continuation grammar")
    } else {
        quote! {
            match continuation {
                #(#resume_arms,)*
            }
        }
    };
    let render_skip = quote! {
        #continuation_declaration
        let mut pending = [
            ::core::mem::MaybeUninit::<#continuation>::uninit();
            #depth
        ];
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
                // SAFETY: pending[frame] is written before stack_depth includes the frame;
                // Continuation: Copy leaves no drop state behind.
                let continuation = unsafe { pending[frame].assume_init() };
                let step = #resume_dispatch;
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
