//! Prepared struct encoding body rendering.

use super::codec_tokens;
use crate::derive::model::{
    ComputationArgument, ComputationByteSelection, Field, FieldKind, FieldPosition,
};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) layout: Layout<'a>,
    pub(super) scheduling: Scheduling<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) types: Types<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Layout<'a> {
    pub(super) fields: &'a [Field],
    pub(super) plans: &'a [Ident],
    pub(super) gaps: &'a [Option<Ident>],
    pub(super) gap_names: &'a [&'a Ident],
    pub(super) variants: &'a [Ident],
}

pub(super) struct Scheduling<'a> {
    pub(super) controlled_by: &'a [Option<usize>],
    pub(super) computation_order: &'a [usize],
}

pub(super) struct Operation<'a> {
    pub(super) operation_prepare: Option<&'a Ident>,
    pub(super) operation_value: &'a Ident,
}

pub(super) struct Types<'a> {
    pub(super) encode_error: &'a Ident,
    pub(super) plan: &'a Ident,
    pub(super) plan_lifetime_init: &'a TokenStream,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        layout:
            Layout {
                fields,
                plans,
                gaps,
                gap_names,
                variants,
            },
        scheduling: Scheduling {
            controlled_by,
            computation_order,
        },
        operation: Operation {
            operation_prepare,
            operation_value,
        },
        types:
            Types {
                encode_error,
                plan,
                plan_lifetime_init,
            },
        runtime,
    } = input;
    let prepare_steps: Vec<_> = fields
        .iter()
        .zip(plans)
        .zip(variants)
        .zip(controlled_by)
        .filter_map(|(((field, plan), variant), controlled_by)| {
            if field.computation.is_some() {
                return None;
            }
            let field_name = &field.name;
            Some(match (&field.kind, controlled_by) {
                (FieldKind::Fixed(codec), Some(bytes_index)) => {
                    let codec = codec_tokens(codec, runtime);
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: #bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::FixedCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Fixed(codec), None) => {
                    let codec = codec_tokens(codec, runtime);
                    quote!(let #plan = <#codec as #runtime::FixedCodec>::plan(#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Nested, _) => {
                    let ty = &field.ty;
                    if field.operation_input.is_some() {
                        quote!(let #plan = <#ty>::#operation_prepare(#field_name, #operation_value).map_err(#encode_error::#variant)?;)
                    } else {
                        quote!(let #plan = <#ty as #runtime::WireEncode>::prepare(#field_name).map_err(#encode_error::#variant)?;)
                    }
                }
                (FieldKind::Prefix(codec), Some(bytes_index)) => {
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: #bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::PrefixCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Prefix(codec), None) => {
                    quote!(let #plan = <#codec as #runtime::PrefixCodec>::plan(#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Bytes { .. } | FieldKind::Rest, _) => quote!(let #plan = #field_name;),
            })
        })
        .collect();
    let computation_steps: Vec<_> = computation_order
        .iter()
        .map(|&index| {
            let field = &fields[index];
            let computation = field.computation.as_ref().expect("computed order");
            let FieldKind::Fixed(codec) = &field.kind else {
                unreachable!("computed fields are fixed codecs")
            };
            let codec = codec_tokens(codec, runtime);
            let plan = &plans[index];
            let variant = &variants[index];
            let source_ty = &computation.value_ty;
            let field_name = &field.name;
            let field_label = field.name.to_string();
            let callback = &computation.callback;
            let mut callback_preparation = Vec::new();
            let callback_arguments: Vec<_> = computation
                .arguments
                .iter()
                .enumerate()
                .map(|(argument_index, argument)| match argument {
                    ComputationArgument::Semantic { index, .. } => {
                        let name = &fields[*index].name;
                        quote!(&#name)
                    }
                    ComputationArgument::Bytes(selection) => {
                        let bytes_name = format_ident!(
                            "__wire_repr_computed_bytes_{index}_{argument_index}"
                        );
                        let source = computation_source(selection, index, fields, plans, gaps, runtime);
                        callback_preparation.push(quote!(let #bytes_name = #source;));
                        quote!(&#bytes_name)
                    }
                })
                .collect();
            let requires_geometry = computation.requires_geometry;
            let step = quote! {
                #(#callback_preparation)*
                let #field_name = <#source_ty>::try_from(#callback(#(#callback_arguments),*)).map_err(|_| {
                    #encode_error::ComputedValueNotRepresentable {
                        field: #field_label,
                    }
                })?;
                let #plan = <#codec as #runtime::FixedCodec>::plan(#field_name)
                    .map_err(#encode_error::#variant)?;
            };
            (requires_geometry, step)
        })
        .collect();
    let early_computation_steps: Vec<_> = computation_steps
        .iter()
        .filter_map(|(needs_geometry, step)| (!needs_geometry).then_some(step))
        .collect();
    let computation_steps: Vec<_> = computation_steps
        .iter()
        .filter_map(|(needs_geometry, step)| needs_geometry.then_some(step))
        .collect();
    let early_computation_steps = &early_computation_steps;
    let computation_steps = &computation_steps;
    let geometry_steps: Vec<_> = fields
        .iter()
        .zip(plans)
        .zip(gaps)
        .map(|((field, plan), gap)| {
            let length = if field.computation.is_some() {
                let FieldKind::Fixed(codec) = &field.kind else {
                    unreachable!("computed fields are fixed codecs")
                };
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            } else {
                match field.kind {
                    FieldKind::Fixed(_)
                    | FieldKind::Prefix(_)
                    | FieldKind::Bytes { .. }
                    | FieldKind::Rest => {
                        quote!(#runtime::ByteSource::byte_len(&#plan))
                    }
                    FieldKind::Nested => {
                        quote!(#runtime::ByteSource::byte_len(&#plan))
                    }
                }
            };
            if let (Some(gap), Some(position)) = (gap, &field.position) {
                let label = field.name.to_string();
                let field_start = match position {
                    FieldPosition::Static(position) => quote!(#position),
                    FieldPosition::Source(source) => quote! {
                        usize::try_from(#source).map_err(|_| {
                            #encode_error::PositionNotRepresentable {
                                field: #label,
                                value: #source as u128,
                            }
                        })?
                    },
                };
                quote! {
                    let field_start = #field_start;
                    if field_start < encoded_len {
                        return Err(#encode_error::PositionBeforeCursor {
                            field: #label,
                            position: field_start,
                            cursor: encoded_len,
                        });
                    }
                    let #gap = field_start - encoded_len;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else if let Some(gap) = gap {
                let padding = field.padding_before;
                let alignment = match field.alignment_before {
                    Some(boundary) => quote!(Some(#boundary)),
                    None => quote!(None::<usize>),
                };
                quote! {
                    let before_gap = encoded_len;
                    let padded = encoded_len.checked_add(#padding).ok_or(#encode_error::LengthOverflow)?;
                    let alignment_padding = match #alignment {
                        Some(boundary) => {
                            let remainder = padded % boundary;
                            if remainder == 0 { 0 } else { boundary - remainder }
                        }
                        None => 0,
                    };
                    let field_start = padded
                        .checked_add(alignment_padding)
                        .ok_or(#encode_error::LengthOverflow)?;
                    let #gap = field_start - before_gap;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else {
                quote! {
                    encoded_len = encoded_len
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            }
        })
        .collect();
    quote! {
        #(#prepare_steps)*
        #(#early_computation_steps)*
        let mut encoded_len = 0usize;
        #(#geometry_steps)*
        #(#computation_steps)*
        Ok(#plan {
            #(#plans,)*
            #(#gap_names,)*
            #plan_lifetime_init
            encoded_len,
        })
    }
}

fn computation_source(
    selection: &ComputationByteSelection,
    own_index: usize,
    fields: &[Field],
    plans: &[Ident],
    gaps: &[Option<Ident>],
    runtime: &TokenStream,
) -> TokenStream {
    let mut components = Vec::new();
    match selection {
        ComputationByteSelection::Exclude(paths) => {
            for (index, plan) in plans.iter().enumerate() {
                if let Some(gap) = &gaps[index] {
                    components.push(quote!(#runtime::ByteSegment::Rest { byte: 0, len: #gap }));
                }
                if index == own_index {
                    continue;
                }
                let selected: Vec<_> = paths
                    .iter()
                    .filter(|path| path.top_level_index == index)
                    .collect();
                if selected.iter().any(|path| path.nested.is_empty()) {
                    continue;
                }
                if selected.is_empty() {
                    components.push(quote!(#runtime::__private::BorrowedSource::new(&#plan)));
                } else {
                    debug_assert!(matches!(fields[index].kind, FieldKind::Nested));
                    let selector = selected
                        .iter()
                        .map(|path| {
                            let nested = &path.nested;
                            quote!(fields #(.#nested)*)
                        })
                        .reduce(|left, right| quote!(#left | #right))
                        .expect("nonempty nested selection");
                    components.push(quote!(#plan.bytes().exclude(|fields| #selector)));
                }
            }
        }
        ComputationByteSelection::Include(paths) => {
            for (index, plan) in plans.iter().enumerate() {
                let selected: Vec<_> = paths
                    .iter()
                    .filter(|path| path.top_level_index == index)
                    .collect();
                if selected.is_empty() {
                    continue;
                }
                if selected.iter().any(|path| path.nested.is_empty()) {
                    components.push(quote!(#runtime::__private::BorrowedSource::new(&#plan)));
                    continue;
                }
                debug_assert!(matches!(fields[index].kind, FieldKind::Nested));
                if let [path] = selected.as_slice() {
                    let nested = &path.nested;
                    components
                        .push(quote!(#plan.bytes().include_direct(|fields| fields #(.#nested)*)));
                    continue;
                }
                let selector = selected
                    .iter()
                    .map(|path| {
                        let nested = &path.nested;
                        quote!(fields #(.#nested)*)
                    })
                    .reduce(|left, right| quote!(#left | #right))
                    .expect("nonempty nested selection");
                components.push(quote!(#plan.bytes().include(|fields| #selector)));
            }
        }
    }
    let _ = own_index;
    chain_sources(components, runtime)
}

fn chain_sources(mut sources: Vec<TokenStream>, runtime: &TokenStream) -> TokenStream {
    let Some(first) = sources.first().cloned() else {
        return quote!(#runtime::__private::EmptySource);
    };
    sources.drain(1..).fold(
        first,
        |left, right| quote!(#runtime::__private::ByteChain::new(#left, #right)),
    )
}
