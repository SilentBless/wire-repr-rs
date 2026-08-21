//! Mutable API rendering for dynamic sequential layouts.

use proc_macro2::TokenStream;
use quote::quote;

use super::super::projection_getters;
use super::{codec_tokens, effective_fixed_codec_tokens, field_start, mapping_raw_type_tokens};
use crate::ir::{Codec, Field, FieldKind, SequentialLayout};

pub(super) fn render_layout(
    layout: &SequentialLayout,
    zero_width: &[TokenStream],
    validation: &[TokenStream],
) -> TokenStream {
    let data = &layout.data;
    let docs = &data.docs;
    let visibility = &data.visibility;
    let layout_name = &data.layout_name;
    let view_mut_name = &data.view_mut_name;
    let error_name = &data.error_name;
    let mutation_error_name = &data.mutation_error_name;
    let boundaries: Vec<_> = data
        .fields
        .iter()
        .filter(|field| field.is_prefix() || field.is_byte_range())
        .map(|field| &field.boundary)
        .collect();
    let boundary_initializers = boundaries.clone();
    let as_view_boundaries = boundaries.clone();
    let into_view_boundaries = as_view_boundaries.clone();
    let getters = data.fields.iter().flat_map(|field| {
        let mut methods = render_getters(layout, field);
        methods.extend(projection_getters(field, visibility));
        methods
    });
    let eligible: Vec<_> = data
        .fields
        .iter()
        .filter_map(|field| eligible_codec(field).map(|codec| (field, codec)))
        .collect();
    let variants = eligible
        .iter()
        .filter_map(|(field, codec)| render_encode_variant(field, codec));
    let display_arms = eligible
        .iter()
        .map(|(field, _)| render_encode_display_arm(field));
    let setters = eligible
        .iter()
        .filter_map(|(field, codec)| render_setter(layout, field, codec, mutation_error_name));

    quote! {
        #[doc = "A mutable validated view of a dynamic sequential wire layout."]
        #(#docs)*
        #visibility struct #view_mut_name<'wire> {
            bytes: &'wire mut [u8],
            #(#boundaries: usize,)*
        }

        impl<'wire> #view_mut_name<'wire> {
            #[doc = "Parses and validates leading mutable layout bytes, returning a disjoint suffix."]
            #[inline]
            #[must_use]
            #visibility fn parse_prefix_mut(input: &'wire mut [u8]) -> ::core::result::Result<(Self, &'wire mut [u8]), #error_name> {
                #(#zero_width)*
                let input_bytes: &[u8] = input;
                let mut remaining = input_bytes;
                #(#validation)*
                let represented_len = input_bytes.len() - remaining.len();
                let available = input.len();
                let Some((bytes, suffix)) = input.split_at_mut_checked(represented_len) else {
                    return Err(#error_name::InputTooShort {
                        position: 1,
                        expected: represented_len,
                        available,
                    });
                };
                Ok((Self { bytes, #(#boundary_initializers,)* }, suffix))
            }

            #[doc = "Parses and validates exactly one complete mutable layout with no trailing bytes."]
            #[inline]
            #[must_use]
            #visibility fn parse_exact_mut(input: &'wire mut [u8]) -> ::core::result::Result<Self, #error_name> {
                #(#zero_width)*
                let actual = input.len();
                let input_bytes: &[u8] = input;
                let mut remaining = input_bytes;
                #(#validation)*
                if !remaining.is_empty() {
                    return Err(#error_name::TrailingBytes {
                        expected: actual - remaining.len(),
                        actual,
                    });
                }
                Ok(Self { bytes: input, #(#boundary_initializers,)* })
            }

            #[doc = "Returns the exact validated bytes represented by this mutable view."]
            #[inline]
            #[must_use]
            #visibility fn as_bytes(&self) -> &[u8] { self.bytes }

            #[doc = "Returns an immutable view borrowing these validated bytes and boundaries."]
            #[inline]
            #[must_use]
            #visibility fn as_view(&self) -> #layout_name<'_> {
                #layout_name { bytes: self.bytes, #(#as_view_boundaries: self.#as_view_boundaries,)* }
            }

            #[doc = "Consumes this mutable view and returns an immutable view with the original lifetime."]
            #[inline]
            #[must_use]
            #visibility fn into_view(self) -> #layout_name<'wire> {
                #layout_name { bytes: self.bytes, #(#into_view_boundaries: self.#into_view_boundaries,)* }
            }

            #(#getters)*
            #(#setters)*
        }

        #[derive(Debug)]
        #[doc = "Reports why a dynamic sequential field mutation could not be prepared."]
        #visibility enum #mutation_error_name {
            #(#variants)*
            #[doc = "Reports a successful codec plan with an invalid encoded length."]
            InvalidPlanLength {
                #[doc = "The field whose plan was invalid."] field: &'static str,
                #[doc = "The codec width required for the field."] expected: usize,
                #[doc = "The length reported by the plan."] actual: usize,
            },
        }
        impl ::core::fmt::Display for #mutation_error_name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #(#display_arms)*
                    Self::InvalidPlanLength { field, expected, actual } => write!(formatter, "field {field} plan length: expected {expected} bytes, got {actual}"),
                }
            }
        }
        impl ::core::error::Error for #mutation_error_name {}
    }
}

fn eligible_codec(field: &Field) -> Option<&Codec> {
    let codec = field.codec()?;
    (!codec.is_prefix()
        && !field.is_derived_range_source()
        && field.derivation.is_none()
        && field.finalization.is_none())
    .then_some(codec)
}

fn render_encode_variant(field: &Field, _codec: &Codec) -> Option<TokenStream> {
    let codec = effective_fixed_codec_tokens(field)?;
    let variant = &field.error_variant;
    let name = normalized_field_name(field);
    Some(quote! {
        #[doc = concat!("Reports an encoding failure for field `", #name, "`.")]
        #variant(<#codec as ::wire_repr::FixedCodec>::EncodeError),
    })
}

fn render_encode_display_arm(field: &Field) -> TokenStream {
    let variant = &field.error_variant;
    let name = normalized_field_name(field);
    quote!(Self::#variant(error) => write!(formatter, "field {} failed encoding: {error:?}", #name),)
}

fn render_setter(
    layout: &SequentialLayout,
    field: &Field,
    _codec: &Codec,
    error_name: &syn::Ident,
) -> Option<TokenStream> {
    let codec = effective_fixed_codec_tokens(field)?;
    let name = &field.setter_name;
    let variant = &field.error_variant;
    let field_text = normalized_field_name(field);
    let start = field_start(layout, field.declaration_index);
    let visibility = &layout.data.visibility;
    let physical = quote! {
        let plan = <#codec as ::wire_repr::FixedCodec>::plan(value).map_err(#error_name::#variant)?;
        let actual = ::wire_repr::EncodePlan::encoded_len(&plan);
        let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH;
        if actual != expected { return Err(#error_name::InvalidPlanLength { field: #field_text, expected, actual }); }
        ::wire_repr::EncodePlan::write_into(&plan, &mut self.bytes[(#start)..((#start) + <#codec as ::wire_repr::FixedCodec>::WIDTH)]);
        Ok(())
    };
    if let (Some(mapping), Some(raw_setter), Some(raw)) = (
        &field.mapping,
        field.raw_setter_name.as_ref(),
        mapping_raw_type_tokens(field),
    ) {
        let semantic = &mapping.semantic;
        return Some(quote! {
            #[doc = "Atomically replaces this field from its semantic value."] #visibility fn #name(&mut self, value: #semantic) -> ::core::result::Result<(), #error_name> { self.#raw_setter(<#raw as ::core::convert::From<#semantic>>::from(value)) }
            #[doc = "Atomically replaces this field from its raw fixed representation."] #visibility fn #raw_setter(&mut self, value: #raw) -> ::core::result::Result<(), #error_name> { #physical }
        });
    }
    if field.mapping.is_some() {
        return None;
    }
    Some(
        quote! { #[doc = "Atomically replaces this field after preparing its complete encoding."] #visibility fn #name<'value>(&mut self, value: <#codec as ::wire_repr::FixedCodec>::Value<'value>) -> ::core::result::Result<(), #error_name> { #physical } },
    )
}

fn render_getters(layout: &SequentialLayout, field: &Field) -> Vec<TokenStream> {
    let docs = &field.docs;
    let visibility = &layout.data.visibility;
    let name = &field.name;
    let start = field_start(layout, field.declaration_index);
    match &field.kind {
        FieldKind::Codec(codec) if codec.is_prefix() => {
            let codec = codec_tokens(codec);
            let raw_getter = &field.raw_getter;
            let end = &field.boundary;
            vec![
                quote! { #[doc = "Returns the decoded value of this validated prefix field."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> <#codec as ::wire_repr::PrefixCodec>::Value<'_> { <#codec as ::wire_repr::PrefixCodec>::decode(self.#raw_getter()) } },
                quote! { #[doc = "Returns the exact validated raw wire bytes of this prefix field (the original wire representation)."] #[inline] #[must_use] #visibility fn #raw_getter(&self) -> &[u8] { &self.bytes[(#start)..self.#end] } },
            ]
        }
        FieldKind::Codec(_) => {
            let Some(codec) = effective_fixed_codec_tokens(field) else {
                return Vec::new();
            };
            if let Some(mapping) = &field.mapping {
                let (Some(raw_name), Some(raw)) =
                    (field.raw_name.as_ref(), mapping_raw_type_tokens(field))
                else {
                    return Vec::new();
                };
                let semantic = &mapping.semantic;
                return vec![quote! {
                    #[doc = "Returns the semantic value of this validated fixed field."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> #semantic { <#semantic as ::core::convert::From<#raw>>::from(self.#raw_name()) }
                    #[doc = "Returns the raw fixed representation of this field."] #[inline] #[must_use] #visibility fn #raw_name(&self) -> #raw { <#codec as ::wire_repr::FixedCodec>::decode(&self.bytes[(#start)..((#start) + <#codec as ::wire_repr::FixedCodec>::WIDTH)]) }
                }];
            }
            vec![
                quote! { #[doc = "Returns the decoded value of this validated fixed field."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> <#codec as ::wire_repr::FixedCodec>::Value<'_> { <#codec as ::wire_repr::FixedCodec>::decode(&self.bytes[(#start)..((#start) + <#codec as ::wire_repr::FixedCodec>::WIDTH)]) } },
            ]
        }
        FieldKind::ByteRange { .. } => {
            let Some(range_mut_name) = field.range_mut_name.as_ref() else {
                return Vec::new();
            };
            let terminal = matches!(
                layout.physical_order.last(),
                Some(crate::ir::PhysicalItem::Field { index, .. })
                    if *index == field.declaration_index
            );
            let (immutable_bytes, mutable_bytes) = if terminal {
                (
                    quote!(&self.bytes[(#start)..]),
                    quote!(&mut self.bytes[(#start)..]),
                )
            } else {
                let end = &field.boundary;
                (
                    quote!(&self.bytes[(#start)..self.#end]),
                    quote!(&mut self.bytes[(#start)..self.#end]),
                )
            };
            vec![
                quote! { #[doc = "Returns the exact opaque bytes in this validated range."] #(#docs)* #[inline] #[must_use] #visibility fn #name(&self) -> &[u8] { #immutable_bytes } },
                quote! { #[doc = "Returns mutable access to exactly this validated range without changing its framing."] #(#docs)* #[inline] #visibility fn #range_mut_name(&mut self) -> &mut [u8] { #mutable_bytes } },
            ]
        }
    }
}

fn normalized_field_name(field: &Field) -> String {
    let name = field.name.to_string();
    name.strip_prefix("r#").unwrap_or(&name).to_owned()
}
