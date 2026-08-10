//! Dynamic-sequential Rust token rendering.

use proc_macro2::TokenStream;
use quote::quote;

mod builder;
mod mutation;

use super::{codec_tokens, projection_getters};
use crate::ir::{Field, FieldKind, PhysicalItem, SequentialLayout};

pub(super) fn render_layout(layout: &SequentialLayout) -> TokenStream {
    let data = &layout.data;
    let docs = &data.docs;
    let visibility = &data.visibility;
    let view_name = &data.view_name;
    let error_name = &data.error_name;
    let boundaries = data
        .fields
        .iter()
        .filter(|field| field.is_prefix() || field.is_region())
        .map(|field| &field.boundary);
    let boundary_initializers = boundaries.clone();
    let error_variants = data.fields.iter().filter_map(|field| {
        let codec = field.codec()?;
        if !field.is_prefix() {
            return None;
        }
        let variant = &field.error_variant;
        let codec = codec_tokens(codec);
        let name = field.name.to_string();
        Some(quote! {
            #[doc = concat!("Reports a validation failure for prefix field `", #name, "`.")]
            #variant(<#codec as ::wire_repr::PrefixCodec>::DecodeError),
        })
    });
    let display_arms = data.fields.iter().filter_map(|field| {
        if !field.is_prefix() { return None; }
        let variant = &field.error_variant;
        let name = field.name.to_string();
        Some(quote! { Self::#variant(error) => write!(formatter, "field {} failed validation: {error:?}", #name), })
    });
    let zero_width: Vec<TokenStream> = layout
        .physical_order
        .iter()
        .filter_map(|item| {
            let PhysicalItem::Field { index, position } = item else {
                return None;
            };
            let field = &data.fields[*index];
            let codec = field.codec()?;
            if codec.is_prefix() {
                return None;
            }
            let codec = codec_tokens(codec);
            Some(quote! {
                if <#codec as ::wire_repr::FixedCodec>::WIDTH == 0 {
                    return Err(#error_name::InvalidCodecWidth { position: #position });
                }
            })
        })
        .collect();
    let validation: Vec<TokenStream> = layout
        .physical_order
        .iter()
        .map(|item| match item {
            PhysicalItem::Field { index, position } => {
                render_field_validation(layout, *index, *position, error_name)
            }
            PhysicalItem::Padding { position, length } => quote! {
                let expected = #length;
                let available = remaining.len();
                if available < expected {
                    return Err(#error_name::InputTooShort {
                        position: #position,
                        expected,
                        available,
                    });
                }
                let (_, suffix) = remaining.split_at(expected);
                remaining = suffix;
            },
            PhysicalItem::Alignment { boundary: 1, .. } => quote! {},
            PhysicalItem::Alignment { position, boundary } => quote! {
                let offset = input_bytes.len() - remaining.len();
                let expected = (#boundary - (offset % #boundary)) % #boundary;
                let available = remaining.len();
                if available < expected {
                    return Err(#error_name::InputTooShort {
                        position: #position,
                        expected,
                        available,
                    });
                }
                let (_, suffix) = remaining.split_at(expected);
                remaining = suffix;
            },
        })
        .collect();
    let getters = data.fields.iter().flat_map(|field| {
        let mut methods = render_getters(layout, field);
        methods.extend(projection_getters(field, visibility));
        methods
    });
    let mutation = mutation::render_layout(layout, &zero_width, &validation);
    let builder = builder::render_layout(layout);

    quote! {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[doc = "An immutable validated view of a dynamic sequential wire layout."]
        #(#docs)*
        #visibility struct #view_name<'wire> {
            bytes: &'wire [u8],
            #(#boundaries: usize,)*
        }

        #[derive(Debug)]
        #[doc = "Reports why parsing this sequential layout failed."]
        #visibility enum #error_name {
            #[doc = "Reports insufficient bytes for a physical layout entry."]
            InputTooShort {
                #[doc = "The one-based position of the physical entry."]
                position: usize,
                #[doc = "The bytes required by the physical entry."]
                expected: usize,
                #[doc = "The bytes available at this physical position."]
                available: usize,
            },
            #[doc = "Reports bytes remaining after an otherwise valid complete layout."]
            TrailingBytes {
                #[doc = "The validated represented length in bytes."]
                expected: usize,
                #[doc = "The provided input length in bytes."]
                actual: usize,
            },
            #[doc = "Reports a fixed codec that declares zero width."]
            InvalidCodecWidth {
                #[doc = "The one-based physical position of the invalid field."]
                position: usize,
            },
            #[doc = "Reports a region length that cannot be represented as usize."]
            InvalidRegionLength {
                #[doc = "The one-based physical position of the region."]
                position: usize,
                #[doc = "The one-based physical position of its length field."]
                source_position: usize,
            },
            #[doc = "Reports a prefix codec extent that exceeds the available input."]
            InvalidPrefixExtent {
                #[doc = "The one-based physical position of the invalid field."]
                position: usize,
                #[doc = "The encoded extent claimed by the prefix codec."]
                claimed: usize,
                #[doc = "The bytes available at this physical position."]
                available: usize,
            },
            #(#error_variants)*
        }

        impl ::core::fmt::Display for #error_name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { position, expected, available } => write!(
                        formatter,
                        "input too short at physical position {position}: expected {expected} bytes, got {available}",
                    ),
                    Self::TrailingBytes { expected, actual } => write!(
                        formatter,
                        "trailing bytes: expected {expected} bytes, got {actual}",
                    ),
                    Self::InvalidCodecWidth { position } => write!(
                        formatter,
                        "fixed codec at position {position} has zero width",
                    ),
                    Self::InvalidRegionLength { position, source_position } => write!(
                        formatter,
                        "region at position {position} has a length from position {source_position} that does not fit usize",
                    ),
                    Self::InvalidPrefixExtent { position, claimed, available } => write!(
                        formatter,
                        "prefix codec at position {position} claimed {claimed} bytes with {available} available",
                    ),
                    #(#display_arms)*
                }
            }
        }

        impl ::core::error::Error for #error_name {}

        impl<'wire> #view_name<'wire> {
            #[doc = "Parses and validates the leading layout bytes, returning its suffix."]
            #[inline]
            #[must_use]
            #visibility fn parse_prefix(
                input: &'wire [u8],
            ) -> ::core::result::Result<(Self, &'wire [u8]), #error_name> {
                #(#zero_width)*
                let input_bytes = input;
                let mut remaining = input_bytes;
                #(#validation)*
                let represented_len = input_bytes.len() - remaining.len();
                let (bytes, suffix) = input_bytes.split_at(represented_len);
                Ok((Self { bytes, #(#boundary_initializers,)* }, suffix))
            }

            #[doc = "Parses and validates exactly one complete layout with no trailing bytes."]
            #[inline]
            #[must_use]
            #visibility fn parse_exact(
                input: &'wire [u8],
            ) -> ::core::result::Result<Self, #error_name> {
                let (view, suffix) = Self::parse_prefix(input)?;
                if !suffix.is_empty() {
                    return Err(#error_name::TrailingBytes {
                        expected: view.bytes.len(),
                        actual: input.len(),
                    });
                }
                Ok(view)
            }

            #[doc = "Returns the exact validated bytes represented by this view."]
            #[inline]
            #[must_use]
            #visibility fn as_bytes(&self) -> &'wire [u8] {
                self.bytes
            }

            #(#getters)*
        }

        #mutation
        #builder
    }
}

fn render_field_validation(
    layout: &SequentialLayout,
    field_index: usize,
    position: usize,
    error_name: &syn::Ident,
) -> TokenStream {
    let field = &layout.data.fields[field_index];
    match &field.kind {
        FieldKind::Codec(codec) if codec.is_prefix() => {
            let codec = codec_tokens(codec);
            let variant = &field.error_variant;
            let boundary = &field.boundary;
            quote! {
                let available = remaining.len();
                let extent = <#codec as ::wire_repr::PrefixCodec>::validate_prefix(remaining)
                    .map_err(#error_name::#variant)?;
                let claimed = extent.encoded_len().get();
                let Some((_, suffix)) = extent.split_input(remaining) else {
                    return Err(#error_name::InvalidPrefixExtent {
                        position: #position,
                        claimed,
                        available,
                    });
                };
                remaining = suffix;
                let #boundary = input_bytes.len() - remaining.len();
            }
        }
        FieldKind::Codec(codec) => {
            let codec = codec_tokens(codec);
            quote! {
                let expected = <#codec as ::wire_repr::FixedCodec>::WIDTH;
                let available = remaining.len();
                if available < expected {
                    return Err(#error_name::InputTooShort {
                        position: #position,
                        expected,
                        available,
                    });
                }
                let (_, suffix) = remaining.split_at(expected);
                remaining = suffix;
            }
        }
        FieldKind::Region { length_source, .. } => {
            let source = &layout.data.fields[*length_source];
            let Some(source_codec) = source.codec() else {
                return TokenStream::new();
            };
            let source_codec = codec_tokens(source_codec);
            let source_start = parse_field_start(layout, *length_source);
            let source_value = if source.is_prefix() {
                let source_end = &source.boundary;
                quote! {
                    <#source_codec as ::wire_repr::PrefixCodec>::decode(
                        &input_bytes[(#source_start)..#source_end],
                    )
                }
            } else {
                quote! {
                    <#source_codec as ::wire_repr::FixedCodec>::decode(
                        &input_bytes[(#source_start)..((#source_start) + <#source_codec as ::wire_repr::FixedCodec>::WIDTH)],
                    )
                }
            };
            let source_position = source.placement;
            let boundary = &field.boundary;
            quote! {
                let source_value = #source_value;
                let Ok(expected) = ::core::convert::TryInto::<usize>::try_into(source_value) else {
                    return Err(#error_name::InvalidRegionLength {
                        position: #position,
                        source_position: #source_position,
                    });
                };
                let region_start = input_bytes.len() - remaining.len();
                let available = remaining.len();
                if available < expected {
                    return Err(#error_name::InputTooShort {
                        position: #position,
                        expected,
                        available,
                    });
                }
                let (_, suffix) = remaining.split_at(expected);
                remaining = suffix;
                let #boundary = region_start + expected;
            }
        }
    }
}

pub(super) fn render_getters(layout: &SequentialLayout, field: &Field) -> Vec<TokenStream> {
    let docs = &field.docs;
    let visibility = &layout.data.visibility;
    let name = &field.name;
    let start = field_start(layout, field.declaration_index);
    match &field.kind {
        FieldKind::Codec(codec) if codec.is_prefix() => {
            let codec = codec_tokens(codec);
            let encoded_getter = &field.encoded_getter;
            let end = &field.boundary;
            vec![
                quote! {
                    #[doc = "Returns the decoded value of this validated prefix field."]
                    #(#docs)*
                    #[inline]
                    #[must_use]
                    #visibility fn #name(&self) -> <#codec as ::wire_repr::PrefixCodec>::Value<'_> {
                        <#codec as ::wire_repr::PrefixCodec>::decode(self.#encoded_getter())
                    }
                },
                quote! {
                    #[doc = "Returns the exact validated encoding of this prefix field."]
                    #[inline]
                    #[must_use]
                    #visibility fn #encoded_getter(&self) -> &'wire [u8] {
                        let bytes: &'wire [u8] = self.bytes;
                        &bytes[(#start)..self.#end]
                    }
                },
            ]
        }
        FieldKind::Codec(codec) => {
            let codec = codec_tokens(codec);
            vec![quote! {
                #[doc = "Returns the decoded value of this validated fixed field."]
                #(#docs)*
                #[inline]
                #[must_use]
                #visibility fn #name(&self) -> <#codec as ::wire_repr::FixedCodec>::Value<'_> {
                    <#codec as ::wire_repr::FixedCodec>::decode(
                        &self.bytes[(#start)..((#start) + <#codec as ::wire_repr::FixedCodec>::WIDTH)],
                    )
                }
            }]
        }
        FieldKind::Region { .. } => {
            let end = &field.boundary;
            vec![quote! {
                #[doc = "Returns the exact opaque bytes in this validated region."]
                #(#docs)*
                #[inline]
                #[must_use]
                #visibility fn #name(&self) -> &'wire [u8] {
                    let bytes: &'wire [u8] = self.bytes;
                    &bytes[(#start)..self.#end]
                }
            }]
        }
    }
}

fn field_start(layout: &SequentialLayout, field_index: usize) -> TokenStream {
    let preceding = &layout.physical_order[..layout.data.fields[field_index].placement - 1];
    let last_dynamic = preceding
        .iter()
        .enumerate()
        .rev()
        .find_map(|(position, item)| match item {
            PhysicalItem::Field { index, .. }
                if layout.data.fields[*index].is_prefix()
                    || layout.data.fields[*index].is_region() =>
            {
                Some((position, *index))
            }
            _ => None,
        });
    let (base, trailing) = match last_dynamic {
        Some((dynamic_position, dynamic_index)) => {
            let boundary = &layout.data.fields[dynamic_index].boundary;
            (quote!(self.#boundary), &preceding[dynamic_position + 1..])
        }
        None => (quote!(0usize), preceding),
    };
    trailing
        .iter()
        .fold(base, |offset, item| advance_offset(layout, offset, item))
}

fn parse_field_start(layout: &SequentialLayout, field_index: usize) -> TokenStream {
    let preceding = &layout.physical_order[..layout.data.fields[field_index].placement - 1];
    let last_dynamic = preceding
        .iter()
        .enumerate()
        .rev()
        .find_map(|(position, item)| match item {
            PhysicalItem::Field { index, .. }
                if layout.data.fields[*index].is_prefix()
                    || layout.data.fields[*index].is_region() =>
            {
                Some((position, *index))
            }
            _ => None,
        });
    let (base, trailing) = match last_dynamic {
        Some((dynamic_position, dynamic_index)) => {
            let boundary = &layout.data.fields[dynamic_index].boundary;
            (quote!(#boundary), &preceding[dynamic_position + 1..])
        }
        None => (quote!(0usize), preceding),
    };
    trailing
        .iter()
        .fold(base, |offset, item| advance_offset(layout, offset, item))
}

fn advance_offset(
    layout: &SequentialLayout,
    offset: TokenStream,
    item: &PhysicalItem,
) -> TokenStream {
    match item {
        PhysicalItem::Field { index, .. } => match layout.data.fields[*index].codec() {
            Some(codec) if !codec.is_prefix() => {
                let codec = codec_tokens(codec);
                quote!((#offset) + <#codec as ::wire_repr::FixedCodec>::WIDTH)
            }
            Some(_) | None => offset,
        },
        PhysicalItem::Padding { length, .. } => quote!((#offset) + #length),
        PhysicalItem::Alignment { boundary: 1, .. } => offset,
        PhysicalItem::Alignment { boundary, .. } => quote!({
            let offset = #offset;
            offset + ((#boundary - (offset % #boundary)) % #boundary)
        }),
    }
}
