//! Fixed absolute-offset Rust token rendering.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{codec_tokens, projection_getters};
use crate::ir::{AbsoluteLayout, Field};

pub(super) fn render_layout(layout: &AbsoluteLayout) -> TokenStream {
    let data = &layout.data;
    let docs = &data.docs;
    let visibility = &data.visibility;
    let view_name = &data.view_name;
    let error_name = &data.error_name;
    let view_mut_name = &data.view_mut_name;
    let builder_name = &data.builder_name;
    let mutation_error_name = &data.mutation_error_name;
    let write_error_name = &data.write_error_name;
    let widths = layout.offset_order.iter().filter_map(|&index| {
        let field = &data.fields[index];
        let codec = codec_tokens(field.codec()?);
        let offset = field.placement;
        Some(quote! {
            let extent = #offset.saturating_add(<#codec as ::wire_repr::FixedCodec>::WIDTH);
            if extent > width { width = extent; }
        })
    });
    let zero_width: Vec<_> = width_checks(layout, error_name).collect();
    let extents: Vec<_> = extent_checks(layout, error_name).collect();
    let overlaps: Vec<_> = overlap_checks(layout, error_name).collect();
    let getters = data.fields.iter().flat_map(|field| {
        let mut methods = render_getter(field, visibility)
            .into_iter()
            .collect::<Vec<_>>();
        methods.extend(projection_getters(field, visibility));
        methods
    });
    let mutable_getters = data.fields.iter().flat_map(|field| {
        let mut methods = render_getter(field, visibility)
            .into_iter()
            .collect::<Vec<_>>();
        methods.extend(projection_getters(field, visibility));
        methods
    });
    let mutation_variants = encode_error_variants(data.fields.iter());
    let mutation_display = encode_display_arms(data.fields.iter());
    let setters = data
        .fields
        .iter()
        .filter_map(|field| render_setter(field, visibility, mutation_error_name));
    let builder_fields = data.fields.iter().filter_map(|field| {
        let codec = codec_tokens(field.codec()?);
        let name = &field.name;
        Some(quote! { #name: ::core::option::Option<<#codec as ::wire_repr::FixedCodec>::Value<'value>> })
    });
    let builder_initializers = data.fields.iter().filter_map(|field| {
        field.codec()?;
        let name = &field.name;
        Some(quote! { #name: ::core::option::Option::None })
    });
    let fluent_methods = data
        .fields
        .iter()
        .filter_map(|field| render_fluent_method(field, visibility, builder_name));
    let destructured = data
        .fields
        .iter()
        .filter_map(|field| field.codec().map(|_| &field.name));
    let write_width: Vec<_> = width_checks(layout, write_error_name).collect();
    let write_extents: Vec<_> = extent_checks(layout, write_error_name).collect();
    let write_overlaps: Vec<_> = overlap_checks(layout, write_error_name).collect();
    let missing = data.fields.iter().filter_map(|field| {
        field.codec()?;
        let name = &field.name;
        let field_text = normalized_field_name(field);
        Some(quote! {
            let #name = match #name {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None => return Err(#write_error_name::MissingField { field: #field_text }),
            };
        })
    });
    let preflight: Vec<_> = data.fields.iter().enumerate().filter_map(|(index, field)| {
        let codec = codec_tokens(field.codec()?);
        let value = &field.name;
        let plan = plan_ident(index);
        let variant = &field.error_variant;
        let field_text = normalized_field_name(field);
        Some(quote! {
            let #plan = <#codec as ::wire_repr::FixedCodec>::plan(#value)
                .map_err(#write_error_name::#variant)?;
            let actual = ::wire_repr::EncodePlan::encoded_len(&#plan);
            let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH;
            if actual != expected {
                return Err(#write_error_name::InvalidPlanLength { field: #field_text, expected, actual });
            }
        })
    }).collect();
    let commits = layout.offset_order.iter().filter_map(|&index| {
        let field = &data.fields[index];
        let codec = codec_tokens(field.codec()?);
        let plan = plan_ident(index);
        let offset = field.placement;
        Some(quote! {
            ::wire_repr::EncodePlan::write_into(
                &#plan,
                &mut bytes[(#offset)..((#offset) + <#codec as ::wire_repr::FixedCodec>::WIDTH)],
            );
        })
    });
    let write_variants = encode_error_variants(data.fields.iter());
    let write_display = encode_display_arms(data.fields.iter());

    quote! {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[doc = "An immutable byte-backed view of a fixed absolute-offset wire layout."]
        #(#docs)*
        #visibility struct #view_name<'wire> { bytes: &'wire [u8] }

        #[derive(Debug)]
        #[doc = "Reports why parsing this fixed absolute-offset layout failed."]
        #visibility enum #error_name {
            #[doc = "Reports that the input ended before the complete layout."]
            InputTooShort { #[doc = "The required layout width in bytes."] expected: usize, #[doc = "The provided input length in bytes."] actual: usize },
            #[doc = "Reports bytes remaining after an otherwise valid complete layout."]
            TrailingBytes { #[doc = "The required layout width in bytes."] expected: usize, #[doc = "The provided input length in bytes."] actual: usize },
            #[doc = "Reports a fixed codec that declares zero width."]
            InvalidCodecWidth { #[doc = "The zero-based offset of the invalid field."] offset: usize },
            #[doc = "Reports a fixed codec whose extent overflows usize."]
            InvalidCodecExtent { #[doc = "The zero-based field offset."] offset: usize, #[doc = "The codec width that overflowed its extent."] width: usize },
            #[doc = "Reports two fields whose nonzero extents overlap."]
            OverlappingFields { #[doc = "The earlier field's zero-based offset."] earlier_offset: usize, #[doc = "The later field's zero-based offset."] later_offset: usize },
        }
        impl ::core::fmt::Display for #error_name { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self {
            Self::InputTooShort { expected, actual } => write!(formatter, "input too short: expected {expected} bytes, got {actual}"),
            Self::TrailingBytes { expected, actual } => write!(formatter, "trailing bytes: expected {expected} bytes, got {actual}"),
            Self::InvalidCodecWidth { offset } => write!(formatter, "fixed codec at offset {offset} has zero width"),
            Self::InvalidCodecExtent { offset, width } => write!(formatter, "fixed codec at offset {offset} with width {width} overflows its extent"),
            Self::OverlappingFields { earlier_offset, later_offset } => write!(formatter, "fields at offsets {earlier_offset} and {later_offset} overlap"),
        } } }
        impl ::core::error::Error for #error_name {}

        impl<'wire> #view_name<'wire> {
            #[doc = "The maximum saturated field extent in bytes."]
            #visibility const WIDTH: usize = { let mut width = 0usize; #(#widths)* width };
            #[doc = "Parses the leading layout bytes, returning its suffix."]
            #[must_use]
            #visibility fn parse_prefix(input: &'wire [u8]) -> ::core::result::Result<(Self, &'wire [u8]), #error_name> {
                #(#zero_width)* #(#extents)* #(#overlaps)*
                if input.len() < Self::WIDTH { return Err(#error_name::InputTooShort { expected: Self::WIDTH, actual: input.len() }); }
                let (bytes, suffix) = input.split_at(Self::WIDTH);
                Ok((Self { bytes }, suffix))
            }
            #[doc = "Parses exactly one complete layout with no trailing bytes."]
            #[must_use]
            #visibility fn parse_exact(input: &'wire [u8]) -> ::core::result::Result<Self, #error_name> {
                let (view, suffix) = Self::parse_prefix(input)?;
                if !suffix.is_empty() { return Err(#error_name::TrailingBytes { expected: Self::WIDTH, actual: input.len() }); }
                Ok(view)
            }
            #[doc = "Returns the exact bytes represented by this view, including gaps."]
            #[must_use]
            #visibility fn as_bytes(&self) -> &'wire [u8] { self.bytes }
            #(#getters)*
        }

        #[doc = "A mutable byte-backed view of a fixed absolute-offset wire layout."]
        #(#docs)*
        #visibility struct #view_mut_name<'wire> { bytes: &'wire mut [u8] }
        impl<'wire> #view_mut_name<'wire> {
            #[doc = "Parses leading mutable layout bytes, returning a disjoint suffix."]
            #[must_use]
            #visibility fn parse_prefix_mut(input: &'wire mut [u8]) -> ::core::result::Result<(Self, &'wire mut [u8]), #error_name> {
                #(#zero_width)* #(#extents)* #(#overlaps)*
                if input.len() < #view_name::WIDTH { return Err(#error_name::InputTooShort { expected: #view_name::WIDTH, actual: input.len() }); }
                let (bytes, suffix) = input.split_at_mut(#view_name::WIDTH);
                Ok((Self { bytes }, suffix))
            }
            #[doc = "Parses exactly one complete mutable layout with no trailing bytes."]
            #[must_use]
            #visibility fn parse_exact_mut(input: &'wire mut [u8]) -> ::core::result::Result<Self, #error_name> {
                let actual = input.len();
                let (view, suffix) = Self::parse_prefix_mut(input)?;
                if !suffix.is_empty() { return Err(#error_name::TrailingBytes { expected: #view_name::WIDTH, actual }); }
                Ok(view)
            }
            #[doc = "Returns the exact bytes represented by this mutable view, including gaps."]
            #[must_use]
            #visibility fn as_bytes(&self) -> &[u8] { self.bytes }
            #[doc = "Returns an immutable view borrowing these bytes."]
            #[must_use]
            #visibility fn as_view(&self) -> #view_name<'_> { #view_name { bytes: self.bytes } }
            #[doc = "Consumes this mutable view and returns an immutable view with the original lifetime."]
            #[must_use]
            #visibility fn into_view(self) -> #view_name<'wire> { #view_name { bytes: self.bytes } }
            #(#mutable_getters)*
            #(#setters)*
        }

        #[derive(Debug)]
        #[doc = "Reports why a fixed absolute-offset field mutation could not be prepared."]
        #visibility enum #mutation_error_name {
            #(#mutation_variants)*
            #[doc = "Reports a successful codec plan with an invalid encoded length."]
            InvalidPlanLength { #[doc = "The field whose plan was invalid."] field: &'static str, #[doc = "The codec width required for the field."] expected: usize, #[doc = "The length reported by the plan."] actual: usize },
        }
        impl ::core::fmt::Display for #mutation_error_name { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self {
            #(#mutation_display)*
            Self::InvalidPlanLength { field, expected, actual } => write!(formatter, "field {field} plan length: expected {expected} bytes, got {actual}"),
        } } }
        impl ::core::error::Error for #mutation_error_name {}

        #[doc = "An atomic fluent builder for a fixed absolute-offset wire layout."]
        #(#docs)*
        #visibility struct #builder_name<'value> { #(#builder_fields,)* }
        impl<'value> #builder_name<'value> {
            #[doc = "Creates an empty builder."]
            #[must_use]
            #visibility fn new() -> Self { Self { #(#builder_initializers,)* } }
            #(#fluent_methods)*
            #[doc = "Preflights every field, then writes this layout into the leading output bytes."]
            #visibility fn build_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#view_mut_name<'output>, &'output mut [u8]), #write_error_name> {
                let Self { #(#destructured,)* } = self;
                #(#write_width)* #(#write_extents)* #(#write_overlaps)*
                #(#missing)* #(#preflight)*
                let needed = #view_name::WIDTH;
                if output.len() < needed { return Err(#write_error_name::OutputTooShort { needed, available: output.len() }); }
                let (bytes, suffix) = output.split_at_mut(needed);
                #(#commits)*
                Ok((#view_mut_name { bytes }, suffix))
            }
        }

        #[derive(Debug)]
        #[doc = "Reports why an atomic fixed absolute-offset builder write could not complete."]
        #visibility enum #write_error_name {
            #[doc = "Reports a fixed codec that declares zero width."]
            InvalidCodecWidth { #[doc = "The zero-based offset of the invalid field."] offset: usize },
            #[doc = "Reports a fixed codec whose extent overflows usize."]
            InvalidCodecExtent { #[doc = "The zero-based field offset."] offset: usize, #[doc = "The codec width that overflowed its extent."] width: usize },
            #[doc = "Reports two fields whose nonzero extents overlap."]
            OverlappingFields { #[doc = "The earlier field's zero-based offset."] earlier_offset: usize, #[doc = "The later field's zero-based offset."] later_offset: usize },
            #[doc = "Reports a field not supplied to the builder."]
            MissingField { #[doc = "The missing field name."] field: &'static str },
            #(#write_variants)*
            #[doc = "Reports a successful codec plan with an invalid encoded length."]
            InvalidPlanLength { #[doc = "The field whose plan was invalid."] field: &'static str, #[doc = "The codec width required for the field."] expected: usize, #[doc = "The length reported by the plan."] actual: usize },
            #[doc = "Reports that output ended before the complete layout."]
            OutputTooShort { #[doc = "The required layout width in bytes."] needed: usize, #[doc = "The provided output length in bytes."] available: usize },
        }
        impl ::core::fmt::Display for #write_error_name { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self {
            Self::InvalidCodecWidth { offset } => write!(formatter, "fixed codec at offset {offset} has zero width"),
            Self::InvalidCodecExtent { offset, width } => write!(formatter, "fixed codec at offset {offset} with width {width} overflows its extent"),
            Self::OverlappingFields { earlier_offset, later_offset } => write!(formatter, "fields at offsets {earlier_offset} and {later_offset} overlap"),
            Self::MissingField { field } => write!(formatter, "missing field {field}"),
            #(#write_display)*
            Self::InvalidPlanLength { field, expected, actual } => write!(formatter, "field {field} plan length: expected {expected} bytes, got {actual}"),
            Self::OutputTooShort { needed, available } => write!(formatter, "output too short: needed {needed} bytes, got {available}"),
        } } }
        impl ::core::error::Error for #write_error_name {}
    }
}

fn encode_error_variants<'a>(
    fields: impl Iterator<Item = &'a Field> + 'a,
) -> impl Iterator<Item = TokenStream> + 'a {
    fields.filter_map(|field| {
        let codec = codec_tokens(field.codec()?);
        let variant = &field.error_variant;
        let name = field.name.to_string();
        Some(quote! { #[doc = concat!("Reports an encoding failure for field `", #name, "`.")] #variant(<#codec as ::wire_repr::FixedCodec>::EncodeError), })
    })
}

fn encode_display_arms<'a>(
    fields: impl Iterator<Item = &'a Field> + 'a,
) -> impl Iterator<Item = TokenStream> + 'a {
    fields.filter_map(|field| {
        field.codec()?;
        let variant = &field.error_variant;
        let name = field.name.to_string();
        Some(quote! { Self::#variant(error) => write!(formatter, "field {} failed encoding: {error:?}", #name), })
    })
}

fn width_checks<'a>(
    layout: &'a AbsoluteLayout,
    error_name: &'a syn::Ident,
) -> impl Iterator<Item = TokenStream> + 'a {
    layout.offset_order.iter().filter_map(move |&index| {
        let field = &layout.data.fields[index];
        let codec = codec_tokens(field.codec()?);
        let offset = field.placement;
        Some(quote! { if <#codec as ::wire_repr::FixedCodec>::WIDTH == 0 { return Err(#error_name::InvalidCodecWidth { offset: #offset }); } })
    })
}

fn extent_checks<'a>(
    layout: &'a AbsoluteLayout,
    error_name: &'a syn::Ident,
) -> impl Iterator<Item = TokenStream> + 'a {
    layout.offset_order.iter().filter_map(move |&index| {
        let field = &layout.data.fields[index];
        let codec = codec_tokens(field.codec()?);
        let offset = field.placement;
        Some(quote! { if #offset.checked_add(<#codec as ::wire_repr::FixedCodec>::WIDTH).is_none() { return Err(#error_name::InvalidCodecExtent { offset: #offset, width: <#codec as ::wire_repr::FixedCodec>::WIDTH }); } })
    })
}

fn overlap_checks<'a>(
    layout: &'a AbsoluteLayout,
    error_name: &'a syn::Ident,
) -> impl Iterator<Item = TokenStream> + 'a {
    layout.offset_order.windows(2).filter_map(move |pair| {
        let earlier = &layout.data.fields[pair[0]];
        let later = &layout.data.fields[pair[1]];
        let codec = codec_tokens(earlier.codec()?);
        later.codec()?;
        let earlier_offset = earlier.placement;
        let later_offset = later.placement;
        Some(quote! { if #earlier_offset + <#codec as ::wire_repr::FixedCodec>::WIDTH > #later_offset { return Err(#error_name::OverlappingFields { earlier_offset: #earlier_offset, later_offset: #later_offset }); } })
    })
}

fn render_getter(field: &Field, visibility: &syn::Visibility) -> Option<TokenStream> {
    let codec = codec_tokens(field.codec()?);
    let docs = &field.docs;
    let name = &field.name;
    let offset = field.placement;
    Some(
        quote! { #[doc = "Returns the decoded value of this field."] #(#docs)* #[must_use] #visibility fn #name(&self) -> <#codec as ::wire_repr::FixedCodec>::Value<'_> { <#codec as ::wire_repr::FixedCodec>::decode(&self.bytes[(#offset)..((#offset) + <#codec as ::wire_repr::FixedCodec>::WIDTH)]) } },
    )
}

fn render_setter(
    field: &Field,
    visibility: &syn::Visibility,
    error_name: &syn::Ident,
) -> Option<TokenStream> {
    let codec = codec_tokens(field.codec()?);
    let name = &field.setter_name;
    let variant = &field.error_variant;
    let field_text = normalized_field_name(field);
    let offset = field.placement;
    Some(
        quote! { #[doc = "Atomically replaces this field after preparing its complete encoding."] #visibility fn #name<'value>(&mut self, value: <#codec as ::wire_repr::FixedCodec>::Value<'value>) -> ::core::result::Result<(), #error_name> { let plan = <#codec as ::wire_repr::FixedCodec>::plan(value).map_err(#error_name::#variant)?; let actual = ::wire_repr::EncodePlan::encoded_len(&plan); let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH; if actual != expected { return Err(#error_name::InvalidPlanLength { field: #field_text, expected, actual }); } ::wire_repr::EncodePlan::write_into(&plan, &mut self.bytes[(#offset)..((#offset) + <#codec as ::wire_repr::FixedCodec>::WIDTH)]); Ok(()) } },
    )
}

fn render_fluent_method(
    field: &Field,
    visibility: &syn::Visibility,
    builder_name: &syn::Ident,
) -> Option<TokenStream> {
    let codec = codec_tokens(field.codec()?);
    let name = &field.name;
    let docs = &field.docs;
    Some(
        quote! { #[doc = "Supplies this field to the builder."] #(#docs)* #[must_use] #visibility fn #name(mut self, value: <#codec as ::wire_repr::FixedCodec>::Value<'value>) -> #builder_name<'value> { self.#name = ::core::option::Option::Some(value); self } },
    )
}

fn normalized_field_name(field: &Field) -> String {
    let name = field.name.to_string();
    name.strip_prefix("r#").unwrap_or(&name).to_owned()
}

fn plan_ident(index: usize) -> syn::Ident {
    format_ident!("__wire_plan_{index}")
}
