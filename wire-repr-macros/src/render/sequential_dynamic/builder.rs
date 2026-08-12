//! Dynamic sequential builder token rendering.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::super::{codec_tokens, effective_fixed_codec_tokens, mapping_raw_type_tokens};
use crate::ir::{Field, FieldKind, PhysicalItem, SequentialLayout};

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
        .filter(|field| !field.is_region_length_source)
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
        .filter(|field| !field.is_region_length_source)
        .map(|field| {
            let name = &field.name;
            quote!(#name: ::core::option::Option::None)
        });
    let destructured = data
        .fields
        .iter()
        .filter(|field| !field.is_region_length_source)
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
    let missing = data.fields.iter().filter(|field| !field.is_region_length_source).map(|field| {
        let name = &field.name;
        let text = field_name(field);
        quote! {
            let #name = match #name {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None => return Err(#write_error_name::MissingField { field: #text }),
            };
        }
    });
    let length_checks = source_length_checks(layout, write_error_name);
    let preflight = data.fields.iter().enumerate().filter_map(|(index, field)| {
        let codec = field.codec()?;
        let plan = plan_ident(index);
        let variant = &field.error_variant;
        let text = field_name(field);
        if field.is_region_length_source {
            let (region, position) = first_region_for_source(layout, index)?;
            let region_name = &data.fields[region].name;
            let source_position = field.placement;
            let convert = if codec.is_prefix() {
                let codec = codec_tokens(codec);
                quote!(<#codec as ::wire_repr::PrefixCodec>::Value<'static>)
            } else {
                let codec = codec_tokens(codec);
                quote!(<#codec as ::wire_repr::FixedCodec>::Value<'static>)
            };
            let source_value = format_ident!("__wire_source_value_{index}");
            let plan_expression = plan_expression(codec, quote!(#source_value));
            let length_check = plan_length_check(codec, &plan, &text, write_error_name);
            Some(quote! {
                let #source_value: #convert = match ::core::convert::TryFrom::<usize>::try_from(#region_name.len()) {
                    Ok(value) => value,
                    Err(_) => return Err(#write_error_name::InvalidRegionLength {
                        position: #position,
                        source_position: #source_position,
                        length: #region_name.len(),
                    }),
                };
                let #plan = #plan_expression.map_err(#write_error_name::#variant)?;
                #length_check
            })
        } else {
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
        }
    });
    let (extent, boundaries) = extent_checks(layout, write_error_name);
    let commits = commits(layout);
    let variants = data.fields.iter().filter_map(encode_variant);
    let displays = data.fields.iter().filter_map(encode_display);
    let fluent = data
        .fields
        .iter()
        .filter(|field| !field.is_region_length_source)
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
                #(#length_checks)*
                #(#preflight)*
                #(#extent)*
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
            #[doc = "Reports a region length that cannot be represented by its source codec."]
            InvalidRegionLength { #[doc = "The one-based physical position of the region."] position: usize, #[doc = "The one-based physical position of its source field."] source_position: usize, #[doc = "The region length in bytes."] length: usize },
            #[doc = "Reports regions sharing a source but having unequal lengths."]
            ConflictingRegionLengths { #[doc = "The one-based physical position of the source field."] source_position: usize, #[doc = "The first region physical position."] first_region_position: usize, #[doc = "The conflicting region physical position."] conflicting_region_position: usize, #[doc = "The first region length in bytes."] expected: usize, #[doc = "The conflicting region length in bytes."] actual: usize },
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
                    Self::InvalidRegionLength { position, source_position, length } => write!(formatter, "region at position {position} has length {length}, not representable by source at position {source_position}"),
                    Self::ConflictingRegionLengths { source_position, first_region_position, conflicting_region_position, expected, actual } => write!(formatter, "regions at positions {first_region_position} and {conflicting_region_position} disagree for source {source_position}: {expected} versus {actual}"),
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

fn source_length_checks(layout: &SequentialLayout, error: &syn::Ident) -> Vec<TokenStream> {
    layout
        .data
        .fields
        .iter()
        .enumerate()
        .filter_map(|(region_index, region)| {
            let FieldKind::Region { length_source, .. } = &region.kind else {
                return None;
            };
            let first = layout.data.fields[..region_index]
                .iter()
                .find(|candidate| {
                    matches!(
                        &candidate.kind,
                        FieldKind::Region {
                            length_source: candidate_source,
                            ..
                        } if candidate_source == length_source
                    )
                })?;
            let source_position = layout.data.fields[*length_source].placement;
            let first_name = &first.name;
            let first_position = first.placement;
            let name = &region.name;
            let position = region.placement;
            Some(quote! {
                if #name.len() != #first_name.len() {
                    return Err(#error::ConflictingRegionLengths {
                        source_position: #source_position,
                        first_region_position: #first_position,
                        conflicting_region_position: #position,
                        expected: #first_name.len(),
                        actual: #name.len(),
                    });
                }
            })
        })
        .collect()
}

fn first_region_for_source(layout: &SequentialLayout, source: usize) -> Option<(usize, usize)> {
    layout
        .data
        .fields
        .iter()
        .enumerate()
        .find_map(|(index, field)| match &field.kind {
            FieldKind::Region { length_source, .. } if *length_source == source => {
                Some((index, field.placement))
            }
            _ => None,
        })
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
                let advance = match field.codec() {
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
            if field.is_prefix() || field.is_region() {
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
            let advance = match field.codec() {
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
            };
            let write = match field.codec() {
                Some(_) => {
                    let plan = plan_ident(*index);
                    quote!(::wire_repr::EncodePlan::write_into(&#plan, &mut bytes[#start..#end]);)
                }
                None => {
                    let name = &field.name;
                    quote!(bytes[#start..#end].copy_from_slice(#name);)
                }
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
