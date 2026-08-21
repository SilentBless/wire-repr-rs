//! Dynamic sequential builder token rendering.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::super::{codec_tokens, effective_fixed_codec_tokens, mapping_raw_type_tokens};
use crate::ir::{
    Builtin, ByteRangeEnd, DeriveOperand, Field, FieldKind, FinalizeBoundary, FinalizeOperand,
    PhysicalItem, RangeSource, SequentialLayout,
};

pub(super) fn render_layout(layout: &SequentialLayout) -> TokenStream {
    let data = &layout.data;
    let docs = &data.docs;
    let visibility = &data.visibility;
    let builder_name = &data.builder_name;
    let plan_name = &data.plan_name;
    let range_input_name = &data.range_input_name;
    let view_mut_name = &data.view_mut_name;
    let write_error_name = &data.write_error_name;
    let inputs: Vec<_> = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source() && field.derivation.is_none() && field.finalization.is_none())
        .map(|field| {
            let name = &field.name;
            if field.is_byte_range() {
                return quote!(#name: ::core::option::Option<#range_input_name<'value>>);
            }
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
    let context_inputs: Vec<_> = data
        .contexts
        .iter()
        .map(|context| {
            let name = &context.name;
            let referent = &context.referent;
            quote!(#name: ::core::option::Option<&'value #referent>)
        })
        .collect();
    let needs_lifetime_marker = inputs.is_empty() && context_inputs.is_empty();
    let lifetime_input = needs_lifetime_marker
        .then(|| quote!(__wire_value: ::core::marker::PhantomData<&'value ()>,));
    let lifetime_initializer =
        needs_lifetime_marker.then(|| quote!(__wire_value: ::core::marker::PhantomData,));
    let lifetime_destructured = needs_lifetime_marker.then(|| quote!(__wire_value: _,));
    let context_initializers: Vec<_> = data
        .contexts
        .iter()
        .map(|context| {
            let name = &context.name;
            quote!(#name: ::core::option::Option::None)
        })
        .collect();
    let context_destructured: Vec<_> = data
        .contexts
        .iter()
        .map(|context| context.name.clone())
        .collect();
    let initializers = data
        .fields
        .iter()
        .filter(|field| {
            !field.is_derived_range_source()
                && field.derivation.is_none()
                && field.finalization.is_none()
        })
        .map(|field| {
            let name = &field.name;
            quote!(#name: ::core::option::Option::None)
        });
    let destructured = data
        .fields
        .iter()
        .filter(|field| {
            !field.is_derived_range_source()
                && field.derivation.is_none()
                && field.finalization.is_none()
        })
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
    let missing = data.fields.iter().filter(|field| !field.is_derived_range_source() && field.derivation.is_none() && field.finalization.is_none()).map(|field| {
        let name = &field.name;
        let text = field_name(field);
        quote! {
            let #name = match #name {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None => return Err(#write_error_name::MissingField { field: #text }),
            };
        }
    });
    let missing_contexts = data.contexts.iter().map(|context| {
        let name = &context.name;
        let text = context.name.to_string();
        let text = text.strip_prefix("r#").unwrap_or(&text).to_owned();
        quote! {
            let #name = match #name {
                ::core::option::Option::Some(value) => value,
                ::core::option::Option::None => return Err(#write_error_name::MissingContext { context: #text }),
            };
        }
    });
    let ordinary_preflight = data.fields.iter().enumerate().filter_map(|(index, field)| {
        if field.is_derived_range_source()
            || field.derivation.is_some()
            || field.finalization.is_some()
        {
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
    let automatic_range_preflight =
        derived_source_preflight(layout, write_error_name, range_input_name);
    let explicit_derivations = explicit_derivations(layout, write_error_name, range_input_name);
    let derived_plans = derived_field_plans(layout, write_error_name);
    let (extent, boundaries) = extent_checks(layout, write_error_name, range_input_name);
    let plan_fields = plan_fields(layout, range_input_name);
    let plan_names = plan_names(layout);
    let range_names = range_names(layout);
    let geometry_names = geometry_names(layout);
    let retained_names = retained_names(layout);
    let retained_bindings = ordinary_retained_bindings(layout);
    let copy_bounds = copy_bounds(layout);
    let commits = commits(layout, range_input_name);
    let finalizers = finalizers(layout);
    let variants = data
        .fields
        .iter()
        .filter_map(encode_variant)
        .chain(data.fields.iter().filter_map(derive_variant));
    let displays = data
        .fields
        .iter()
        .filter_map(encode_display)
        .chain(data.fields.iter().filter_map(derive_display));
    let debugs = data
        .fields
        .iter()
        .filter_map(encode_debug)
        .chain(data.fields.iter().filter_map(derive_debug));
    let range_source_variants = transformed_range_source_variants(layout);
    let range_source_debugs = transformed_range_source_debugs(layout);
    let range_source_displays = transformed_range_source_displays(layout);
    let context_fluent = data.contexts.iter().map(|context| {
        let name = &context.name;
        let setter = &context.setter_name;
        let referent = &context.referent;
        let docs = &context.docs;
        quote! { #[doc = "Supplies this borrowed finalizer context to the builder."] #(#docs)* #[must_use] #visibility fn #setter(mut self, value: &'value #referent) -> Self { self.#name = ::core::option::Option::Some(value); self } }
    });
    let fluent = data
        .fields
        .iter()
        .filter(|field| !field.is_derived_range_source() && field.derivation.is_none() && field.finalization.is_none())
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
            if field.is_byte_range() {
                let existing = field.range_existing_name.as_ref().expect("byte range existing builder name");
                return vec![quote! {
                    #[doc = "Supplies borrowed bytes for this range to the builder."] #(#docs)* #[must_use]
                    #visibility fn #name(mut self, value: &'value [u8]) -> Self { self.#name = ::core::option::Option::Some(#range_input_name::Borrowed(value)); self }
                    #[doc = "Declares that this range already exists in the destination."] #[must_use]
                    #visibility fn #existing(mut self, length: usize) -> Self { self.#name = ::core::option::Option::Some(#range_input_name::Existing(length)); self }
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
        #visibility struct #builder_name<'value> { #(#inputs,)* #(#context_inputs,)* #lifetime_input }

        enum #range_input_name<'value> { Borrowed(&'value [u8]), Existing(usize) }

        #[doc = "A prepared dynamic sequential layout encoding."]
        #visibility struct #plan_name<'value> { #(#plan_fields,)* }

        impl<'value> #plan_name<'value> {
            #[doc = "Returns the exact number of bytes required by this prepared layout."]
            #[must_use]
            #[inline]
            #visibility fn encoded_len(&self) -> usize {
                <Self as ::wire_repr::PreparedLayout>::encoded_len(self)
            }

            #[doc = "Commits this prepared layout into the leading output bytes."]
            #[inline]
            #visibility fn commit_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#view_mut_name<'output>, &'output mut [u8]), ::wire_repr::OutputTooShortError> {
                <Self as ::wire_repr::PreparedLayout>::commit_into(self, output)
            }
        }

        impl<'value> ::wire_repr::PreparedLayout for #plan_name<'value> {
            type ViewMut<'output> = #view_mut_name<'output>;

            #[inline]
            fn encoded_len(&self) -> usize { self.__wire_represented_len }

            #[inline]
            fn commit_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#view_mut_name<'output>, &'output mut [u8]), ::wire_repr::OutputTooShortError> {
                let required = self.__wire_represented_len;
                let available = output.len();
                if available < required {
                    return Err(::wire_repr::OutputTooShortError { required, available });
                }
                let (bytes, suffix) = output.split_at_mut(required);
                let Self { #(#plan_names,)* #(#range_names,)* #(#geometry_names,)* #(#retained_names,)* #(#context_destructured,)* __wire_represented_len, __wire_value: _, } = self;
                #(#commits)*
                #(#finalizers)*
                Ok((#view_mut_name { bytes, #(#boundaries,)* }, suffix))
            }
        }

        impl<'value> #builder_name<'value> {
            #[doc = "Creates an empty builder."]
            #[must_use]
            #visibility fn new() -> Self { Self { #(#initializers,)* #(#context_initializers,)* #lifetime_initializer } }
            #(#fluent)*
            #(#context_fluent)*

            #[doc = "Prepares every field and dynamic layout extent for a later output-buffer commit."]
            #[inline]
            #visibility fn prepare(self) -> ::core::result::Result<#plan_name<'value>, #write_error_name>
            where
                #(#copy_bounds,)*
            {
                #(#zero_width)*
                let Self { #(#destructured,)* #(#context_destructured,)* #lifetime_destructured } = self;
                #(#missing)*
                #(#missing_contexts)*
                #(#retained_bindings)*
                #(#ordinary_preflight)*
                #(#extent)*
                #(#automatic_range_preflight)*
                #(#explicit_derivations)*
                #(#derived_plans)*
                Ok(#plan_name { #(#plan_names,)* #(#range_names,)* #(#geometry_names,)* #(#retained_names,)* #(#context_destructured,)* __wire_represented_len: __wire_offset, __wire_value: ::core::marker::PhantomData })
            }

            #[doc = "Preflights the complete layout, then writes it into leading output bytes."]
            #[inline]
            #visibility fn build_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#view_mut_name<'output>, &'output mut [u8]), #write_error_name>
            where
                #(#copy_bounds,)*
            {
                <#plan_name<'value> as ::wire_repr::PreparedLayout>::commit_into(self.prepare()?, output)
                    .map_err(|error| #write_error_name::OutputTooShort { expected: error.required, actual: error.available })
            }
        }

        #[doc = "Reports why an atomic dynamic sequential builder write could not complete."]
        #visibility enum #write_error_name {
            #[doc = "Reports a field not supplied to the builder."]
            MissingField { #[doc = "The missing field name."] field: &'static str },
            #[doc = "Reports a finalizer context not supplied to the builder."]
            MissingContext { #[doc = "The missing context name."] context: &'static str },
            #(#variants)*
            #[doc = "Reports a fixed codec that declares zero width."]
            InvalidCodecWidth { #[doc = "The one-based physical position of the invalid field."] position: usize },
            #[doc = "Reports a range-derived source value that cannot be represented by its physical source type."]
            InvalidRangeSource { #[doc = "The one-based physical position of the range."] position: usize, #[doc = "The one-based physical position of its source field."] source_position: usize, #[doc = "The required source value."] value: usize },
            #(#range_source_variants)*
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

        impl ::core::fmt::Debug for #write_error_name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::MissingField { field } => formatter.debug_struct("MissingField").field("field", field).finish(),
                    Self::MissingContext { context } => formatter.debug_struct("MissingContext").field("context", context).finish(),
                    #(#debugs)*
                    Self::InvalidCodecWidth { position } => formatter.debug_struct("InvalidCodecWidth").field("position", position).finish(),
                    Self::InvalidRangeSource { position, source_position, value } => formatter.debug_struct("InvalidRangeSource").field("position", position).field("source_position", source_position).field("value", value).finish(),
                    #(#range_source_debugs)*
                    Self::ConflictingRangeSources { source_position, first_range_position, conflicting_range_position, expected, actual } => formatter.debug_struct("ConflictingRangeSources").field("source_position", source_position).field("first_range_position", first_range_position).field("conflicting_range_position", conflicting_range_position).field("expected", expected).field("actual", actual).finish(),
                    Self::InvalidPlanLength { field, expected, actual } => formatter.debug_struct("InvalidPlanLength").field("field", field).field("expected", expected).field("actual", actual).finish(),
                    Self::InvalidPrefixPlanLength { field } => formatter.debug_struct("InvalidPrefixPlanLength").field("field", field).finish(),
                    Self::InvalidLayoutExtent { position, offset, advance } => formatter.debug_struct("InvalidLayoutExtent").field("position", position).field("offset", offset).field("advance", advance).finish(),
                    Self::OutputTooShort { expected, actual } => formatter.debug_struct("OutputTooShort").field("expected", expected).field("actual", actual).finish(),
                }
            }
        }

        impl ::core::fmt::Display for #write_error_name {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::MissingField { field } => write!(formatter, "missing field {field}"),
                    Self::MissingContext { context } => write!(formatter, "missing context {context}"),
                    #(#displays)*
                    Self::InvalidCodecWidth { position } => write!(formatter, "fixed codec at position {position} has zero width"),
                    Self::InvalidRangeSource { position, source_position, value } => write!(formatter, "range at position {position} requires source value {value}, not representable by source at position {source_position}"),
                    #(#range_source_displays)*
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

fn finalizer_value_sources(layout: &SequentialLayout) -> Vec<usize> {
    let mut sources = Vec::new();
    for field in &layout.data.fields {
        let Some(finalization) = &field.finalization else {
            continue;
        };
        for operand in &finalization.operands {
            if let FinalizeOperand::Value { source, .. } = operand {
                let source_field = &layout.data.fields[*source];
                if source_field.finalization.is_none() && !sources.contains(source) {
                    sources.push(*source);
                }
            }
        }
    }
    sources
}

fn copied_value_sources(layout: &SequentialLayout) -> Vec<usize> {
    let mut sources = finalizer_value_sources(layout);
    for field in &layout.data.fields {
        let Some(derivation) = &field.derivation else {
            continue;
        };
        for operand in &derivation.operands {
            let DeriveOperand::Value { source, .. } = operand else {
                continue;
            };
            if layout.data.fields[*source].derivation.is_none() && !sources.contains(source) {
                sources.push(*source);
            }
        }
    }
    sources
}

fn retained_ident(index: usize) -> syn::Ident {
    format_ident!("__wire_retained_value_{index}")
}

fn retained_type(field: &Field) -> Option<TokenStream> {
    if field.derivation.is_some() && field.mapping.is_some() {
        return field.mapping.as_ref().map(|mapping| {
            let semantic = &mapping.semantic;
            quote!(#semantic)
        });
    }
    if let Some(raw) = mapping_raw_type_tokens(field) {
        return Some(raw);
    }
    let codec = if field.codec().is_some_and(|codec| codec.is_prefix()) {
        codec_tokens(field.codec()?)
    } else {
        effective_fixed_codec_tokens(field)?
    };
    Some(if field.is_derived_range_source() {
        quote!(<#codec as ::wire_repr::FixedCodec>::Value<'static>)
    } else if field.codec().is_some_and(|codec| codec.is_prefix()) {
        quote!(<#codec as ::wire_repr::PrefixCodec>::Value<'value>)
    } else {
        quote!(<#codec as ::wire_repr::FixedCodec>::Value<'value>)
    })
}

fn retained_names(layout: &SequentialLayout) -> Vec<syn::Ident> {
    finalizer_value_sources(layout)
        .into_iter()
        .map(retained_ident)
        .collect()
}

fn copy_bounds(layout: &SequentialLayout) -> Vec<TokenStream> {
    copied_value_sources(layout)
        .into_iter()
        .filter_map(|index| {
            let ty = retained_type(&layout.data.fields[index])?;
            Some(quote!(#ty: ::core::marker::Copy))
        })
        .collect()
}

fn ordinary_retained_bindings(layout: &SequentialLayout) -> Vec<TokenStream> {
    finalizer_value_sources(layout)
        .into_iter()
        .filter_map(|index| {
            let field = &layout.data.fields[index];
            if field.derivation.is_some() || field.is_derived_range_source() {
                return None;
            }
            let name = &field.name;
            let retained = retained_ident(index);
            Some(quote!(let #retained = #name;))
        })
        .collect()
}

fn plan_names(layout: &SequentialLayout) -> Vec<syn::Ident> {
    layout
        .data
        .fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.codec().is_some() && field.finalization.is_none())
        .map(|(index, _)| plan_ident(index))
        .collect()
}

fn range_names(layout: &SequentialLayout) -> Vec<syn::Ident> {
    layout
        .data
        .fields
        .iter()
        .filter(|field| field.is_byte_range())
        .map(|field| field.name.clone())
        .collect()
}

fn geometry_names(layout: &SequentialLayout) -> Vec<syn::Ident> {
    let mut names = Vec::new();
    for item in &layout.physical_order {
        if let PhysicalItem::Field { index, .. } = item {
            names.push(format_ident!("__wire_start_{index}"));
            names.push(format_ident!("__wire_end_{index}"));
        }
    }
    names
}

fn plan_fields(layout: &SequentialLayout, range_input: &syn::Ident) -> Vec<TokenStream> {
    let mut fields = Vec::new();
    for (index, field) in layout.data.fields.iter().enumerate() {
        if field.codec().is_some() && field.finalization.is_none() {
            let plan = plan_ident(index);
            let codec = if field.codec().is_some_and(|codec| codec.is_prefix()) {
                codec_tokens(field.codec().expect("codec"))
            } else {
                effective_fixed_codec_tokens(field).expect("fixed codec")
            };
            let ty = if field.is_derived_range_source() {
                quote!(<#codec as ::wire_repr::FixedCodec>::Plan<'static>)
            } else if field.codec().is_some_and(|codec| codec.is_prefix()) {
                quote!(<#codec as ::wire_repr::PrefixCodec>::Plan<'value>)
            } else {
                quote!(<#codec as ::wire_repr::FixedCodec>::Plan<'value>)
            };
            fields.push(quote!(#plan: #ty));
        }
        if field.is_byte_range() {
            let name = &field.name;
            fields.push(quote!(#name: #range_input<'value>));
        }
    }
    for item in &layout.physical_order {
        if let PhysicalItem::Field { index, .. } = item {
            let start = format_ident!("__wire_start_{index}");
            let end = format_ident!("__wire_end_{index}");
            fields.push(quote!(#start: usize));
            fields.push(quote!(#end: usize));
        }
    }
    fields.push(quote!(__wire_represented_len: usize));
    fields.push(quote!(__wire_value: ::core::marker::PhantomData<&'value ()>));
    for index in finalizer_value_sources(layout) {
        let retained = retained_ident(index);
        if let Some(ty) = retained_type(&layout.data.fields[index]) {
            fields.push(quote!(#retained: #ty));
        }
    }
    for context in &layout.data.contexts {
        let name = &context.name;
        let referent = &context.referent;
        fields.push(quote!(#name: &'value #referent));
    }
    fields
}

fn derived_field_plans(layout: &SequentialLayout, error: &syn::Ident) -> Vec<TokenStream> {
    layout
        .derived_order
        .iter()
        .filter_map(|index| {
            let field = &layout.data.fields[*index];
            let name = &field.name;
            let plan = plan_ident(*index);
            let variant = &field.error_variant;
            let text = field_name(field);
            let expression = fixed_plan_expression(field, plan_value(field, quote!(#name)))?;
            let length_check = fixed_plan_length_check(field, &plan, &text, error)?;
            let retain = finalizer_value_sources(layout).contains(index).then(|| {
                let retained = retained_ident(*index);
                quote!(let #retained = #name;)
            });
            Some(quote! { #retain let #plan = #expression.map_err(#error::#variant)?; #length_check })
        })
        .collect()
}

fn explicit_derivations(
    layout: &SequentialLayout,
    error: &syn::Ident,
    range_input: &syn::Ident,
) -> Vec<TokenStream> {
    layout
        .derived_order
        .iter()
        .filter_map(|index| {
            let field = &layout.data.fields[*index];
            let derivation = field.derivation.as_ref()?;
            let name = &field.name;
            let function = &derivation.function;
            let variant = derive_variant_ident(field);
            let operands = derivation.operands.iter().map(|operand| match operand {
                DeriveOperand::Value { source, .. } => semantic_value(&layout.data.fields[*source]),
                DeriveOperand::Len { source, .. } => {
                    range_length(&layout.data.fields[*source], range_input)
                }
            });
            Some(quote! { let #name = #function(#(#operands),*).map_err(#error::#variant)?; })
        })
        .collect()
}

fn range_length(field: &Field, range_input: &syn::Ident) -> TokenStream {
    let name = &field.name;
    quote!(match &#name { #range_input::Borrowed(value) => value.len(), #range_input::Existing(length) => *length })
}

fn semantic_value(field: &Field) -> TokenStream {
    let name = &field.name;
    if field.derivation.is_some() {
        quote!(&#name)
    } else if let (Some(mapping), Some(raw)) = (&field.mapping, mapping_raw_type_tokens(field)) {
        let semantic = &mapping.semantic;
        quote!(&<#semantic as ::core::convert::From<#raw>>::from(#name))
    } else {
        quote!(&#name)
    }
}

fn plan_value(field: &Field, value: TokenStream) -> TokenStream {
    if let (Some(_), Some(raw)) = (&field.mapping, mapping_raw_type_tokens(field)) {
        quote!(<#raw as ::core::convert::From<_>>::from(#value))
    } else {
        value
    }
}

fn derive_variant_ident(field: &Field) -> syn::Ident {
    format_ident!("Derive{}", field.error_variant)
}

fn derived_source_preflight(
    layout: &SequentialLayout,
    error: &syn::Ident,
    range_input: &syn::Ident,
) -> Vec<TokenStream> {
    let mut required_values = Vec::new();
    let mut conflicts = Vec::new();
    let mut source_plans = Vec::new();

    for (source, field) in layout.data.fields.iter().enumerate() {
        if !field.is_derived_range_source() {
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
        let first_value = range_source_value(first_index, first_range, error, range_input);
        let required_value = format_ident!("__wire_required_source_value_{source}");
        required_values.push(quote! {
            let #required_value = #first_value;
        });
        conflicts.extend(ranges.iter().skip(1).map(|(index, range)| {
            let declaration_index = range.declaration_index;
            let position = range.placement;
            let value = range_source_value(*index, range, error, range_input);
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
        let source_name = &field.name;
        let source_binding = derived_range_source_value_binding(layout, source)
            .then(|| quote!(let #source_name = #source_value;));
        let retained_binding = finalizer_value_sources(layout).contains(&source).then(|| {
            let retained = retained_ident(source);
            quote!(let #retained = #source_value;)
        });
        let plan = plan_ident(source);
        let text = field_name(field);
        let plan_expression = quote!(<#codec as ::wire_repr::FixedCodec>::plan(#source_value));
        let Some(length_check) = fixed_plan_length_check(field, &plan, &text, error) else {
            continue;
        };
        let conversion = match field.range_source.as_ref() {
            Some(RangeSource::Transformed { adapter }) => {
                let variant = range_source_variant(field);
                quote! {
                    let #source_value: <#codec as ::wire_repr::FixedCodec>::Value<'static> = <#adapter as ::wire_repr::RangeSource<#codec>>::from_bytes(#required_value)
                        .map_err(|error| #error::#variant { position: #first_position, source_position: #source_position, value: #required_value, error })?;
                }
            }
            Some(RangeSource::Direct) | None => quote! {
                let #source_value: <#codec as ::wire_repr::FixedCodec>::Value<'static> = match ::core::convert::TryFrom::<usize>::try_from(#required_value) {
                    Ok(value) => value,
                    Err(_) => return Err(#error::InvalidRangeSource {
                        position: #first_position,
                        source_position: #source_position,
                        value: #required_value,
                    }),
                };
            },
        };
        let field_variant = &field.error_variant;
        source_plans.push(quote! {
            #conversion
            #source_binding
            #retained_binding
            let #plan = #plan_expression.map_err(#error::#field_variant)?;
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

fn range_source_variant(field: &Field) -> syn::Ident {
    format_ident!("RangeSource{}", field.error_variant)
}

fn transformed_range_source_fields(layout: &SequentialLayout) -> impl Iterator<Item = &Field> {
    layout
        .data
        .fields
        .iter()
        .filter(|field| matches!(field.range_source, Some(RangeSource::Transformed { .. })))
}

fn transformed_range_source_variants(layout: &SequentialLayout) -> Vec<TokenStream> {
    transformed_range_source_fields(layout)
        .map(|field| {
            let variant = range_source_variant(field);
            let codec = codec_tokens(field.codec().expect("transformed source codec"));
            let adapter = match field.range_source.as_ref().expect("range source") {
                RangeSource::Transformed { adapter } => adapter,
                RangeSource::Direct => unreachable!("filtered transformed source"),
            };
            let name = field_name(field);
            quote! {
                #[doc = concat!("Reports range-source conversion failure for field `", #name, "`.")]
                #variant {
                    #[doc = "The one-based physical position of the first consuming range."]
                    position: usize,
                    #[doc = "The one-based physical position of the source field."]
                    source_position: usize,
                    #[doc = "The required byte geometry."]
                    value: usize,
                    #[doc = "The range-source adapter error."]
                    error: <#adapter as ::wire_repr::RangeSource<#codec>>::Error,
                },
            }
        })
        .collect()
}

fn transformed_range_source_debugs(layout: &SequentialLayout) -> Vec<TokenStream> {
    transformed_range_source_fields(layout)
        .map(|field| {
            let variant = range_source_variant(field);
            quote! { Self::#variant { position, source_position, value, error } => formatter.debug_struct(stringify!(#variant)).field("position", position).field("source_position", source_position).field("value", value).field("error", error).finish(), }
        })
        .collect()
}

fn transformed_range_source_displays(layout: &SequentialLayout) -> Vec<TokenStream> {
    transformed_range_source_fields(layout)
        .map(|field| {
            let variant = range_source_variant(field);
            quote! { Self::#variant { position, source_position, value, error } => write!(formatter, "range at position {position} failed conversion of byte geometry {value} for source at position {source_position}: {error:?}"), }
        })
        .collect()
}

fn derived_range_source_value_binding(layout: &SequentialLayout, source: usize) -> bool {
    layout.data.fields.iter().any(|field| {
        field.derivation.as_ref().is_some_and(|derivation| {
            derivation.operands.iter().any(|operand| {
                matches!(operand, DeriveOperand::Value { source: candidate, .. } if *candidate == source)
            })
        })
    })
}

fn range_source_value(
    index: usize,
    range: &Field,
    error: &syn::Ident,
    range_input: &syn::Ident,
) -> TokenStream {
    let name = &range.name;
    let length = quote!(match &#name { #range_input::Borrowed(value) => value.len(), #range_input::Existing(length) => *length });
    match &range.kind {
        FieldKind::ByteRange {
            end: ByteRangeEnd::Relative { .. },
        } => length,
        FieldKind::ByteRange {
            end: ByteRangeEnd::Absolute { .. },
        } => {
            let start = format_ident!("__wire_start_{index}");
            let position = range.placement;
            quote!(match #start.checked_add(#length) {
                Some(value) => value,
                None => return Err(#error::InvalidLayoutExtent {
                    position: #position,
                    offset: #start,
                    advance: #length,
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
    range_input: &syn::Ident,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let mut output = vec![quote!(let mut __wire_offset = 0usize;)];
    let mut boundaries = Vec::new();
    for item in &layout.physical_order {
        let (position, advance) = match item {
            PhysicalItem::Field { index, position } => {
                let field = &layout.data.fields[*index];
                let advance = match &field.kind {
                    FieldKind::ByteRange { .. } => {
                        let name = &field.name;
                        quote!(match &#name { #range_input::Borrowed(value) => value.len(), #range_input::Existing(length) => *length })
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
            PhysicalItem::Field { index, .. } => Some((
                format_ident!("__wire_start_{index}"),
                format_ident!("__wire_end_{index}"),
            )),
            _ => None,
        };
        output.push(match start {
            Some((start, end)) => quote! {
                let __wire_advance = #advance;
                let #start = __wire_offset;
                __wire_offset = match __wire_offset.checked_add(__wire_advance) { Some(value) => value, None => return Err(#error::InvalidLayoutExtent { position: #position, offset: #start, advance: __wire_advance }) };
                let #end = __wire_offset;
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
                let end = format_ident!("__wire_end_{}", index);
                boundaries.push(quote!(#boundary: #end));
            }
        }
    }
    (output, boundaries)
}

fn commits(layout: &SequentialLayout, range_input: &syn::Ident) -> Vec<TokenStream> {
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
            let write = if field.finalization.is_some() {
                quote!(bytes[#start..#end].fill(0);)
            } else if field.is_byte_range() {
                let name = &field.name;
                quote!(if let #range_input::Borrowed(value) = &#name { bytes[#start..#end].copy_from_slice(value); })
            } else {
                match field.codec() {
                    Some(_) => {
                        let plan = plan_ident(*index);
                        quote!(::wire_repr::EncodePlan::write_into(&#plan, &mut bytes[#start..#end]);)
                    }
                    None => quote!(),
                }
            };
            Some(quote! { #write })
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

fn derive_variant(field: &Field) -> Option<TokenStream> {
    let derivation = field.derivation.as_ref()?;
    let variant = derive_variant_ident(field);
    let error = &derivation.error;
    let name = field_name(field);
    Some(
        quote!(#[doc = concat!("Reports a pre-write derivation failure for field `", #name, "`.")] #variant(#error),),
    )
}

fn derive_debug(field: &Field) -> Option<TokenStream> {
    field.derivation.as_ref()?;
    let variant = derive_variant_ident(field);
    Some(
        quote!(Self::#variant(_) => formatter.debug_struct(stringify!(#variant)).field("payload", &"<opaque derivation error>").finish(),),
    )
}

fn encode_debug(field: &Field) -> Option<TokenStream> {
    field.codec()?;
    let variant = &field.error_variant;
    Some(
        quote!(Self::#variant(error) => formatter.debug_tuple(stringify!(#variant)).field(error).finish(),),
    )
}

fn derive_display(field: &Field) -> Option<TokenStream> {
    field.derivation.as_ref()?;
    let variant = derive_variant_ident(field);
    let name = field_name(field);
    Some(quote!(Self::#variant(_) => write!(formatter, "field {} failed derivation", #name),))
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

fn retained_semantic_value(field: &Field, index: usize) -> TokenStream {
    let retained = retained_ident(index);
    if field.derivation.is_some() || field.mapping.is_none() {
        quote!(&#retained)
    } else {
        let semantic = &field.mapping.as_ref().expect("mapping").semantic;
        quote!(&<#semantic as ::core::convert::From<_>>::from(#retained))
    }
}

fn finalizers(layout: &SequentialLayout) -> Vec<TokenStream> {
    layout
        .finalizer_order
        .iter()
        .filter_map(|index| {
            let field = &layout.data.fields[*index];
            let finalization = field.finalization.as_ref()?;
            let function = &finalization.function;
            let value = final_value_ident(*index);
            let codec = codec_tokens(field.codec()?);
            let value_type = quote!(<#codec as ::wire_repr::FixedCodec>::Value<'static>);
            let operands = finalization.operands.iter().map(|operand| match operand {
                FinalizeOperand::Bytes { start, end } => {
                    let start = finalizer_boundary(*start);
                    let end = finalizer_boundary(*end);
                    quote!(&bytes[(#start)..(#end)])
                }
                FinalizeOperand::Context { source, span } => {
                    let _ = span;
                    let name = &layout.data.contexts[*source].name;
                    quote!(#name)
                }
                FinalizeOperand::Value { source, span } => {
                    let _ = span;
                    let field = &layout.data.fields[*source];
                    if field.finalization.is_some() {
                        let value = final_value_ident(*source);
                        quote!(&#value)
                    } else {
                        retained_semantic_value(field, *source)
                    }
                }
            });
            let start = format_ident!("__wire_start_{index}");
            let end = format_ident!("__wire_end_{index}");
            let patch = builtin_patch(field.codec()?, &start, &end, &value);
            Some(quote! {
                let #value: #value_type = #function(#(#operands),*);
                #patch
            })
        })
        .collect()
}

fn finalizer_boundary(boundary: FinalizeBoundary) -> TokenStream {
    match boundary {
        FinalizeBoundary::BufStart => quote!(0usize),
        FinalizeBoundary::BufEnd => quote!(__wire_represented_len),
        FinalizeBoundary::FieldStart(index) => {
            let start = format_ident!("__wire_start_{index}");
            quote!(#start)
        }
        FinalizeBoundary::FieldEnd(index) => {
            let end = format_ident!("__wire_end_{index}");
            quote!(#end)
        }
    }
}

fn final_value_ident(index: usize) -> syn::Ident {
    format_ident!("__wire_finalized_value_{index}")
}

fn builtin_patch(
    codec: &crate::ir::Codec,
    start: &syn::Ident,
    end: &syn::Ident,
    value: &syn::Ident,
) -> TokenStream {
    match codec {
        crate::ir::Codec::Builtin(Builtin::U8) | crate::ir::Codec::Builtin(Builtin::I8) => {
            quote!(bytes[#start] = #value as u8;)
        }
        crate::ir::Codec::Builtin(Builtin::BeU16)
        | crate::ir::Codec::Builtin(Builtin::BeI16)
        | crate::ir::Codec::Builtin(Builtin::BeU32)
        | crate::ir::Codec::Builtin(Builtin::BeI32)
        | crate::ir::Codec::Builtin(Builtin::BeU64)
        | crate::ir::Codec::Builtin(Builtin::BeI64)
        | crate::ir::Codec::Builtin(Builtin::BeU128)
        | crate::ir::Codec::Builtin(Builtin::BeI128) => {
            quote!(bytes[#start..#end].copy_from_slice(&#value.to_be_bytes());)
        }
        crate::ir::Codec::Builtin(Builtin::LeU16)
        | crate::ir::Codec::Builtin(Builtin::LeI16)
        | crate::ir::Codec::Builtin(Builtin::LeU32)
        | crate::ir::Codec::Builtin(Builtin::LeI32)
        | crate::ir::Codec::Builtin(Builtin::LeU64)
        | crate::ir::Codec::Builtin(Builtin::LeI64)
        | crate::ir::Codec::Builtin(Builtin::LeU128)
        | crate::ir::Codec::Builtin(Builtin::LeI128) => {
            quote!(bytes[#start..#end].copy_from_slice(&#value.to_le_bytes());)
        }
        _ => quote!(::core::compile_error!(
            "internal wire-repr error: finalizer target is not a supported builtin integer"
        );),
    }
}
