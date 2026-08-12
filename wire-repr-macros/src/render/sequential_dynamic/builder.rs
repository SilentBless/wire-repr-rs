//! Dynamic sequential builder token rendering.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::super::{codec_tokens, effective_fixed_codec_tokens, mapping_raw_type_tokens};
use crate::ir::{ByteRangeEnd, Field, FieldKind, PhysicalItem, SequentialLayout};

pub(super) fn render_layout(layout: &SequentialLayout) -> TokenStream {
    let data = &layout.data;
    let docs = &data.docs;
    let visibility = &data.visibility;
    let builder_name = &data.builder_name;
    let view_mut_name = &data.view_mut_name;
    let write_error_name = &data.write_error_name;
    let inputs: Vec<_> = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source)
        .map(|field| {
            let name = &field.name;
            match field.codec() {
                Some(codec) if codec.is_prefix() => {
                    let codec = codec_tokens(codec);
                    quote!(#name: ::core::option::Option<<#codec as ::wire_repr::PrefixCodec>::Value<'value>>)
                }
                Some(_) => {
                    let Some(codec) = effective_fixed_codec_tokens(field) else { return quote!(); };
                    quote!(#name: ::core::option::Option<<#codec as ::wire_repr::FixedCodec>::Value<'value>>)
                }
                None => quote!(#name: ::core::option::Option<&'value [u8]>),
            }
        })
        .collect();
    let initializers = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source)
        .map(|field| {
            let name = &field.name;
            quote!(#name: ::core::option::Option::None)
        });
    let destructured = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source)
        .map(|field| &field.name);
    let zero_width = layout.physical_order.iter().filter_map(|item| {
        let PhysicalItem::Field { index, position } = item else {
            return None;
        };
        let field = &data.fields[*index];
        let codec = field.codec()?;
        if codec.is_prefix() {
            return None;
        }
        let codec = effective_fixed_codec_tokens(field)?;
        Some(quote! {
            if <#codec as ::wire_repr::FixedCodec>::WIDTH == 0 {
                return Err(#write_error_name::InvalidCodecWidth { position: #position });
            }
        })
    });
    let missing = data.fields.iter().filter(|field| !field.is_derived_range_source).map(|field| {
        let name = &field.name;
        let text = field_name(field);
        quote! {
            let #name = match #name {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None => return Err(#write_error_name::MissingField { field: #text }),
            };
        }
    });
    let ordinary_preflight = data.fields.iter().enumerate().filter_map(|(index, field)| {
        if field.is_derived_range_source {
            return None;
        }
        let codec = field.codec()?;
        let plan = plan_ident(index);
        let variant = &field.error_variant;
        let text = field_name(field);
        let name = &field.name;
        let (plan_expression, length_check) = if codec.is_prefix() {
            (
                plan_expression(codec, quote!(#name)),
                plan_length_check(codec, &plan, &text, write_error_name),
            )
        } else {
            (
                fixed_plan_expression(field, quote!(#name))?,
                fixed_plan_length_check(field, &plan, &text, write_error_name)?,
            )
        };
        Some(quote! {
            let #plan = #plan_expression.map_err(#write_error_name::#variant)?;
            #length_check
        })
    });
    let derived_preflight = derived_source_preflight(layout, write_error_name);
    let (extent, boundaries) = extent_checks(layout, write_error_name);
    let commits = commits(layout);
    let variants = data.fields.iter().filter_map(encode_variant);
    let displays = data.fields.iter().filter_map(encode_display);
    let fluent = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source)
        .flat_map(|field| {
            let name = &field.name;
            let docs = &field.docs;
            if field.mapping.is_some() && (field.raw_name.is_none() || mapping_raw_type_tokens(field).is_none()) {
                return Vec::new();
            }
            if let (Some(mapping), Some(raw), Some(raw_name)) = (&field.mapping, mapping_raw_type_tokens(field), field.raw_name.as_ref()) {
                let semantic = &mapping.semantic;
                return vec![quote! {
                    #[doc = "Supplies this field's semantic value to the builder."] #(#docs)* #[must_use]
                    #visibility fn #name(mut self, value: #semantic) -> Self { self.#name = ::core::option::Option::Some(<#raw as ::core::convert::From<#semantic>>::from(value)); self }
                    #[doc = "Supplies this field's raw fixed representation to the builder."] #[must_use]
                    #visibility fn #raw_name(mut self, value: #raw) -> Self { self.#name = ::core::option::Option::Some(value); self }
                }];
            }
            let value = match field.codec() {
                Some(codec) if codec.is_prefix() => { let codec = codec_tokens(codec); quote!(<#codec as ::wire_repr::PrefixCodec>::Value<'value>) }
                Some(codec) => { let codec = codec_tokens(codec); quote!(<#codec as ::wire_repr::FixedCodec>::Value<'value>) }
                None => quote!(&'value [u8]),
            };
            vec![quote! { #[doc = "Supplies this field to the builder."] #(#docs)* #[must_use] #visibility fn #name(mut self, value: #value) -> Self { self.#name = ::core::option::Option::Some(value); self } }]
        });

    quote! {
        #[doc = "An atomic fluent builder for a dynamic sequential wire layout."]
        #(#docs)*
        #visibility struct #builder_name<'value> { #(#inputs,)* }

        impl<'value> #builder_name<'value> {
            #[doc = "Creates an empty builder."]
            #[must_use]
            #visibility fn new() -> Self { Self { #(#initializers,)* } }
            #(#fluent)*

            #[doc = "Preflights the complete layout, then writes it into leading output bytes."]
            #[inline]
            #visibility fn build_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#view_mut_name<'output>, &'output mut [u8]), #write_error_name> {
                #(#zero_width)*
                let Self { #(#destructured,)* .. } = self;
                #(#missing)*
                #(#ordinary_preflight)*
                #(#extent)*
                #(#derived_preflight)*
                let expected = __wire_offset;
                let actual = output.len();
                if actual < expected {
                    return Err(#write_error_name::OutputTooShort { expected, actual });
                }
                let (bytes, suffix) = output.split_at_mut(expected);
                #(#commits)*
                Ok((#view_mut_name { bytes, #(#boundaries,)* }, suffix))
            }
        }

        #[derive(Debug)]
        #[doc = "Reports why an atomic dynamic sequential builder write could not complete."]
        #visibility enum #write_error_name {
            #[doc = "Reports a field not supplied to the builder."]
            MissingField { #[doc = "The missing field name."] field: &'static str },
            #(#variants)*
            #[doc = "Reports a fixed codec that declares zero width."]
            InvalidCodecWidth { #[doc = "The one-based physical position of the invalid field."] position: usize },
            #[doc = "Reports a range-derived source value that cannot be represented by its physical source type."]
            InvalidRangeSource { #[doc = "The one-based physical position of the range."] position: usize, #[doc = "The one-based physical position of its source field."] source_position: usize, #[doc = "The required source value."] value: usize },
            #[doc = "Reports ranges sharing a source but requiring unequal source values."]
            ConflictingRangeSources { #[doc = "The one-based physical position of the source field."] source_position: usize, #[doc = "The first range physical position."] first_range_position: usize, #[doc = "The conflicting range physical position."] conflicting_range_position: usize, #[doc = "The first required source value."] expected: usize, #[doc = "The conflicting required source value."] actual: usize },
            #[doc = "Reports a successful fixed codec plan with an invalid encoded length."]
            InvalidPlanLength { #[doc = "The field whose plan was invalid."] field: &'static str, #[doc = "The codec width required for the field."] expected: usize, #[doc = "The length reported by the plan."] actual: usize },
            #[doc = "Reports a successful prefix codec plan with zero encoded length."]
            InvalidPrefixPlanLength { #[doc = "The field whose plan was invalid."] field: &'static str },
            #[doc = "Reports a physical layout extent that cannot be represented in `usize`."]
            InvalidLayoutExtent { #[doc = "The one-based physical position of the overflowing item."] position: usize, #[doc = "The represented offset before this item."] offset: usize, #[doc = "The byte advance requested by this item."] advance: usize },
            #[doc = "Reports output shorter than the represented layout."]
            OutputTooShort { #[doc = "The represented layout length in bytes."] expected: usize, #[doc = "The provided output length in bytes."] actual: usize },
        }

        impl ::core::fmt::Display for #write_error_name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::MissingField { field } => write!(formatter, "missing field {field}"),
                    #(#displays)*
                    Self::InvalidCodecWidth { position } => write!(formatter, "fixed codec at position {position} has zero width"),
                    Self::InvalidRangeSource { position, source_position, value } => write!(formatter, "range at position {position} requires source value {value}, not representable by source at position {source_position}"),
                    Self::ConflictingRangeSources { source_position, first_range_position, conflicting_range_position, expected, actual } => write!(formatter, "ranges at positions {first_range_position} and {conflicting_range_position} disagree for source {source_position}: {expected} versus {actual}"),
                    Self::InvalidPlanLength { field, expected, actual } => write!(formatter, "field {field} plan length: expected {expected} bytes, got {actual}"),
                    Self::InvalidPrefixPlanLength { field } => write!(formatter, "prefix field {field} plan has zero length"),
                    Self::InvalidLayoutExtent { position, offset, advance } => write!(formatter, "layout extent at position {position} overflows: offset {offset} plus advance {advance}"),
                    Self::OutputTooShort { expected, actual } => write!(formatter, "output too short: need {expected} bytes, got {actual}"),
                }
            }
        }
        impl ::core::error::Error for #write_error_name {}
    }
}

fn derived_source_preflight(layout: &SequentialLayout, error: &syn::Ident) -> Vec<TokenStream> {
    let mut required_values = Vec::new();
    let mut conflicts = Vec::new();
    let mut source_plans = Vec::new();

    for (source, field) in layout.data.fields.iter().enumerate() {
        if !field.is_derived_range_source {
            continue;
        }
        let Some(codec) = field.codec() else {
            continue;
        };
        if codec.is_prefix() {
            continue;
        }
        let Some(codec) = effective_fixed_codec_tokens(field) else {
            continue;
        };
        let ranges: Vec<_> = layout
            .data
            .fields
            .iter()
            .enumerate()
            .filter_map(|(index, range)| match &range.kind {
                FieldKind::ByteRange {
                    end:
                        ByteRangeEnd::Relative {
                            source: candidate, ..
                        },
                }
                | FieldKind::ByteRange {
                    end:
                        ByteRangeEnd::Absolute {
                            source: candidate, ..
                        },
                } if *candidate == source => Some((index, range)),
                _ => None,
            })
            .collect();
        let Some(&(first_index, first_range)) = ranges.first() else {
            continue;
        };
        let first_position = first_range.placement;
        let source_position = field.placement;
        let first_value = range_source_value(first_index, first_range, error);
        let required_value = format_ident!("__wire_required_source_value_{source}");
        required_values.push(quote! {
            let #required_value = #first_value;
        });
        conflicts.extend(ranges.iter().skip(1).map(|(index, range)| {
            let declaration_index = range.declaration_index;
            let position = range.placement;
            let value = range_source_value(*index, range, error);
            let check = quote! {
                let __wire_range_value = #value;
                if __wire_range_value != #required_value {
                    return Err(#error::ConflictingRangeSources {
                        source_position: #source_position,
                        first_range_position: #first_position,
                        conflicting_range_position: #position,
                        expected: #required_value,
                        actual: __wire_range_value,
                    });
                }
            };
            (declaration_index, check)
        }));
        let source_value = format_ident!("__wire_source_value_{source}");
        let plan = plan_ident(source);
        let variant = &field.error_variant;
        let text = field_name(field);
        let plan_expression = quote!(<#codec as ::wire_repr::FixedCodec>::plan(#source_value));
        let Some(length_check) = fixed_plan_length_check(field, &plan, &text, error) else {
            continue;
        };
        source_plans.push(quote! {
            let #source_value: <#codec as ::wire_repr::FixedCodec>::Value<'static> = match ::core::convert::TryFrom::<usize>::try_from(#required_value) {
                Ok(value) => value,
                Err(_) => return Err(#error::InvalidRangeSource {
                    position: #first_position,
                    source_position: #source_position,
                    value: #required_value,
                }),
            };
            let #plan = #plan_expression.map_err(#error::#variant)?;
            #length_check
        });
    }

    conflicts.sort_by_key(|(declaration_index, _)| *declaration_index);
    required_values
        .into_iter()
        .chain(conflicts.into_iter().map(|(_, check)| check))
        .chain(source_plans)
        .collect()
}

fn range_source_value(index: usize, range: &Field, error: &syn::Ident) -> TokenStream {
    let name = &range.name;
    match &range.kind {
        FieldKind::ByteRange {
            end: ByteRangeEnd::Relative { .. },
        } => quote!(#name.len()),
        FieldKind::ByteRange {
            end: ByteRangeEnd::Absolute { .. },
        } => {
            let start = format_ident!("__wire_start_{index}");
            let position = range.placement;
            quote!(match #start.checked_add(#name.len()) {
                Some(value) => value,
                None => return Err(#error::InvalidLayoutExtent {
                    position: #position,
                    offset: #start,
                    advance: #name.len(),
                }),
            })
        }
        FieldKind::ByteRange {
            end: ByteRangeEnd::BufEnd,
        }
        | FieldKind::Codec(_) => quote!(0usize),
    }
}

fn plan_expression(codec: &crate::ir::Codec, value: TokenStream) -> TokenStream {
    let is_prefix = codec.is_prefix();
    let codec = codec_tokens(codec);
    if is_prefix {
        quote!(<#codec as ::wire_repr::PrefixCodec>::plan(#value))
    } else {
        quote!(<#codec as ::wire_repr::FixedCodec>::plan(#value))
    }
}

fn fixed_plan_expression(field: &Field, value: TokenStream) -> Option<TokenStream> {
    let codec = effective_fixed_codec_tokens(field)?;
    Some(quote!(<#codec as ::wire_repr::FixedCodec>::plan(#value)))
}

fn fixed_plan_length_check(
    field: &Field,
    plan: &syn::Ident,
    field_name: &str,
    error: &syn::Ident,
) -> Option<TokenStream> {
    let codec = effective_fixed_codec_tokens(field)?;
    let field_name = field_name.to_owned();
    Some(
        quote! { let actual = ::wire_repr::EncodePlan::encoded_len(&#plan); let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH; if actual != expected { return Err(#error::InvalidPlanLength { field: #field_name, expected, actual }); } },
    )
}

fn plan_length_check(
    codec: &crate::ir::Codec,
    plan: &syn::Ident,
    field: &str,
    error: &syn::Ident,
) -> TokenStream {
    let field = field.to_owned();
    if codec.is_prefix() {
        quote! { if ::wire_repr::EncodePlan::encoded_len(&#plan) == 0 { return Err(#error::InvalidPrefixPlanLength { field: #field }); } }
    } else {
        let codec = codec_tokens(codec);
        quote! { let actual = ::wire_repr::EncodePlan::encoded_len(&#plan); let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH; if actual != expected { return Err(#error::InvalidPlanLength { field: #field, expected, actual }); } }
    }
}

fn extent_checks(
    layout: &SequentialLayout,
    error: &syn::Ident,
) -> (Vec<TokenStream>, Vec<syn::Ident>) {
    let mut output = vec![quote!(let mut __wire_offset = 0usize;)];
    let mut boundaries = Vec::new();
    for item in &layout.physical_order {
        let (position, advance) = match item {
            PhysicalItem::Field { index, position } => {
                let field = &layout.data.fields[*index];
                let advance = match &field.kind {
                    FieldKind::ByteRange {
                        end: ByteRangeEnd::BufEnd,
                    } => {
                        let name = &field.name;
                        quote!(#name.len())
                    }
                    _ => match field.codec() {
                        Some(codec) if codec.is_prefix() => {
                            let plan = plan_ident(*index);
                            quote!(::wire_repr::EncodePlan::encoded_len(&#plan))
                        }
                        Some(_) => {
                            let Some(codec) = effective_fixed_codec_tokens(field) else {
                                continue;
                            };
                            quote!(<#codec as ::wire_repr::FixedCodec>::WIDTH)
                        }
                        None => {
                            let name = &field.name;
                            quote!(#name.len())
                        }
                    },
                };
                (*position, advance)
            }
            PhysicalItem::Padding { position, length } => (*position, quote!(#length)),
            PhysicalItem::Alignment { position, boundary } => (
                *position,
                quote!((#boundary - (__wire_offset % #boundary)) % #boundary),
            ),
        };
        let start = match item {
            PhysicalItem::Field { index, .. } => Some(format_ident!("__wire_start_{index}")),
            _ => None,
        };
        output.push(match start {
            Some(start) => quote! {
                let __wire_advance = #advance;
                let #start = __wire_offset;
                __wire_offset = match __wire_offset.checked_add(__wire_advance) { Some(value) => value, None => return Err(#error::InvalidLayoutExtent { position: #position, offset: #start, advance: __wire_advance }) };
            },
            None => quote! {
                let __wire_advance = #advance;
                let __wire_start = __wire_offset;
                __wire_offset = match __wire_offset.checked_add(__wire_advance) { Some(value) => value, None => return Err(#error::InvalidLayoutExtent { position: #position, offset: __wire_start, advance: __wire_advance }) };
            },
        });
        if let PhysicalItem::Field { index, .. } = item {
            let field = &layout.data.fields[*index];
            if field.is_prefix() || field.is_byte_range() {
                let boundary = &field.boundary;
                boundaries.push(boundary.clone());
                output.push(quote!(let #boundary = __wire_offset;));
            }
        }
    }
    (output, boundaries)
}

fn commits(layout: &SequentialLayout) -> Vec<TokenStream> {
    layout
        .physical_order
        .iter()
        .filter_map(|item| {
            let PhysicalItem::Field { index, .. } = item else {
                return None;
            };
            let field = &layout.data.fields[*index];
            let start = format_ident!("__wire_start_{index}");
            let end = format_ident!("__wire_end_{index}");
            let advance = match &field.kind {
                FieldKind::ByteRange { end: ByteRangeEnd::BufEnd } => {
                    let name = &field.name;
                    quote!(#name.len())
                }
                _ => match field.codec() {
                    Some(codec) if codec.is_prefix() => {
                        let plan = plan_ident(*index);
                        quote!(::wire_repr::EncodePlan::encoded_len(&#plan))
                    }
                    Some(_) => {
                        let codec = effective_fixed_codec_tokens(field)?;
                        quote!(<#codec as ::wire_repr::FixedCodec>::WIDTH)
                    }
                    None => {
                        let name = &field.name;
                        quote!(#name.len())
                    }
                },
            };
            let write = match &field.kind {
                FieldKind::ByteRange { end: ByteRangeEnd::BufEnd } => {
                    let name = &field.name;
                    quote!(bytes[#start..#end].copy_from_slice(#name);)
                }
                _ => match field.codec() {
                    Some(_) => {
                        let plan = plan_ident(*index);
                        quote!(::wire_repr::EncodePlan::write_into(&#plan, &mut bytes[#start..#end]);)
                    }
                    None => {
                        let name = &field.name;
                        quote!(bytes[#start..#end].copy_from_slice(#name);)
                    }
                },
            };
            Some(quote! { let #end = #start + #advance; #write })
        })
        .collect()
}

fn encode_variant(field: &Field) -> Option<TokenStream> {
    let codec = field.codec()?;
    let codec = if codec.is_prefix() {
        codec_tokens(codec)
    } else {
        effective_fixed_codec_tokens(field)?
    };
    let variant = &field.error_variant;
    let name = field_name(field);
    if field.is_prefix() {
        Some(
            quote!(#[doc = concat!("Reports an encoding failure for field `", #name, "`.")] #variant(<#codec as ::wire_repr::PrefixCodec>::EncodeError),),
        )
    } else {
        Some(
            quote!(#[doc = concat!("Reports an encoding failure for field `", #name, "`.")] #variant(<#codec as ::wire_repr::FixedCodec>::EncodeError),),
        )
    }
}

fn encode_display(field: &Field) -> Option<TokenStream> {
    field.codec()?;
    let variant = &field.error_variant;
    let name = field_name(field);
    Some(
        quote!(Self::#variant(error) => write!(formatter, "field {} failed encoding: {error:?}", #name),),
    )
}
fn field_name(field: &Field) -> String {
    let name = field.name.to_string();
    name.strip_prefix("r#").unwrap_or(&name).to_owned()
}
fn plan_ident(index: usize) -> syn::Ident {
    format_ident!("__wire_plan_{index}")
}
