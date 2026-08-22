//! Fixed-only struct derive rendering.

use super::super::super::model::{Codec, FieldKind, FieldPosition, WireStruct};
use super::{codec_tokens, variant_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireStruct {
        vis,
        name,
        wire_lifetime,
        operation_input: _,
        validators: model_validators,
        validation_error,
        fields,
        preparation,
    } = model;
    let position_sources = preparation.position_sources;
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let plan = format_ident!("{name}Plan");
    let field_proxy = format_ident!("{name}Fields");
    let markers: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("__WireRepr{name}Field{index}"))
        .collect();
    let labels: Vec<_> = fields.iter().map(|field| field.name.to_string()).collect();
    let variants: Vec<_> = fields
        .iter()
        .map(|field| variant_name(&field.name.to_string()))
        .collect();
    let plans: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}"))
        .collect();
    let gaps: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            (field.position.is_some()
                || field.padding_before != 0
                || field.alignment_before.is_some())
            .then(|| format_ident!("gap_{index}"))
        })
        .collect();
    let gap_names: Vec<_> = gaps.iter().flatten().collect();
    let has_geometry = !gap_names.is_empty();
    let has_positions = fields.iter().any(|field| field.position.is_some());
    let validation_error_type = validation_error
        .as_ref()
        .map(|error| quote!(#error))
        .unwrap_or_else(|| quote!(#decode_error));
    let validation_impl = {
        let field_validators = fields.iter().flat_map(|field| {
            let name = &field.name;
            field
                .validators
                .iter()
                .map(move |validator| quote!(#validator(self.#name())?;))
        });
        quote! { impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> { type ValidationError = #validation_error_type; fn validate(&self) -> Result<(), Self::ValidationError> { #(#field_validators)* #(#model_validators(self)?;)* Ok(()) } } }
    };
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let cursor_method = quote! {
        /// Returns a fail-closed cursor over consecutive representations.
        #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
            #runtime::ValidatedViewCursor::new(input)
        }
    };
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_request<'__wire_repr_view, #view<'__wire_repr_view>>),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>),
        )
    };

    let plain_fixed_sequence = fields.iter().all(|field| {
        field.position.is_none() && field.padding_before == 0 && field.alignment_before.is_none()
    });
    let fixed_widths: Vec<_> = fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            }
            _ => unreachable!(),
        })
        .collect();
    let fixed_view_constructor = plain_fixed_sequence.then(|| {
        quote! {
            fn from_sequence(bytes: &'__wire_repr_wire [u8]) -> Self {
                Self { bytes }
            }
        }
    });
    let fixed_sequence_method = plain_fixed_sequence.then(|| {
        if validation_error.is_some() {
            quote! {
                /// Structurally frames fixed-width items without running semantic validators.
                #vis fn unchecked_views<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> Result<#runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>, #runtime::FixedViewSequenceError> {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                }
                /// Frames and semantically validates every fixed-width item before returning an infallible iterator.
                #vis fn views<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> Result<#runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>, #runtime::FixedValidatedViewSequenceError<#validation_error_type>> {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    let iterator = #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                        .map_err(#runtime::FixedValidatedViewSequenceError::Framing)?;
                    for view in iterator.clone() {
                        #runtime::WireViewValidation::validate(&view)
                            .map_err(#runtime::FixedValidatedViewSequenceError::Item)?;
                    }
                    Ok(iterator)
                }
            }
        } else {
            quote! {
                /// Validates complete fixed-width sequence framing and returns an infallible iterator.
                #vis fn views<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> Result<#runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>, #runtime::FixedViewSequenceError> {
                    let item_width = 0usize #(+ #fixed_widths)*;
                    #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
                }
            }
        }
    });

    let decode_steps = fields.iter().enumerate().map(|(index, field)| {
        let label = &labels[index];
        let field_name = &field.name;
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        let geometry = decode_geometry(field, label, &decode_error);
        let decode_source = position_sources[index].then(|| quote!(
            let #field_name = <#codec as #runtime::FixedCodec>::decode(bytes);
        ));
        quote! {
            #geometry
            let width = <#codec as #runtime::FixedCodec>::WIDTH;
            let available = remaining.len();
            let Some((bytes, suffix)) = remaining.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort { field: #label, required: width, available });
            };
            #decode_source
            remaining = suffix;
        }
    });
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        let prior = fields
            .iter()
            .take(index)
            .map(|prior| getter_cursor_step(prior, runtime));
        let geometry = getter_geometry(field, runtime);
        let return_type = match &field.kind {
            FieldKind::Fixed(Codec::OwnedBytes(length)) => quote!(&'__wire_repr_wire [u8; #length]),
            FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::Value<'__wire_repr_wire>)
            }
            _ => unreachable!(),
        };
        let value = match &field.kind {
            FieldKind::Fixed(Codec::OwnedBytes(length)) => quote! {
                match <&'__wire_repr_wire [u8; #length]>::try_from(bytes) {
                    Ok(bytes) => bytes,
                    Err(_) => unreachable!("validated fixed byte array has its declared width"),
                }
            },
            FieldKind::Fixed(_) => quote!(<#codec as #runtime::FixedCodec>::decode(bytes)),
            _ => unreachable!(),
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            #vis fn #field_name(&self) -> #return_type {
                let mut cursor = 0usize;
                #(#prior)*
                #geometry
                let width = <#codec as #runtime::FixedCodec>::WIDTH;
                let bytes = &self.bytes[cursor..cursor + width];
                #value
            }
        }
    });
    let plan_fields = fields.iter().zip(&plans).map(|(field, plan)| {
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        quote!(#plan: <#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>)
    });
    let proxy_fields = fields.iter().zip(&markers).map(|(field, marker)| {
        let field = &field.name;
        quote!(#vis #field: <S as #runtime::MarkerScope>::Wrap<#marker>)
    });
    let proxy_values = fields.iter().zip(&markers).map(|(field, marker)| {
        let field = &field.name;
        quote!(#field: <S as #runtime::MarkerScope>::wrap(#marker))
    });
    let marker_impls = fields.iter().enumerate().map(|(index, field)| {
        let marker = &markers[index];
        let prior_steps = (0..index).map(|prior_index| {
            let prior_plan = &plans[prior_index];
            let prior_gap = &gaps[prior_index];
            let gap = prior_gap.as_ref().map(|gap| quote! {
                cursor = cursor.checked_add(target.#gap).expect("prepared field geometry overflow");
            });
            quote! {
                #gap
                cursor = cursor.checked_add(#runtime::ByteSource::byte_len(&target.#prior_plan)).expect("prepared field geometry overflow");
            }
        });
        let view_prior_steps = fields
            .iter()
            .take(index)
            .map(|prior| view_getter_cursor_step(prior, runtime));
        let view_geometry = view_getter_geometry(field, runtime);
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        let current_plan = &plans[index];
        let current_gap = &gaps[index];
        let gap = current_gap.as_ref().map(|gap| quote! {
            cursor = cursor.checked_add(target.#gap).expect("prepared field geometry overflow");
        });
        quote! {
            #[allow(missing_docs)]
            #[doc(hidden)]
            #[derive(Clone, Copy)]
            #vis struct #marker;
            impl<R> ::core::ops::BitOr<R> for #marker {
                type Output = #runtime::FieldUnion<Self, R>;
                fn bitor(self, right: R) -> Self::Output {
                    #runtime::FieldUnion::new(self, right)
                }
            }
            impl<'__wire_repr_value> #runtime::FieldSelection<#plan<'__wire_repr_value>> for #marker {
                #[inline(always)]
                fn visit_ranges<V>(&self, target: &#plan<'__wire_repr_value>, visitor: &mut V)
                where
                    V: FnMut(::core::ops::Range<usize>),
                {
                    let mut cursor = 0usize;
                    #(#prior_steps)*
                    #gap
                    let end = cursor.checked_add(#runtime::ByteSource::byte_len(&target.#current_plan)).expect("prepared field geometry overflow");
                    visitor(cursor..end);
                }
                #[inline(always)]
                fn direct_len(&self, target: &#plan<'__wire_repr_value>) -> Option<usize> {
                    Some(#runtime::ByteSource::byte_len(&target.#current_plan))
                }
                #[inline(always)]
                fn emit_direct<S: #runtime::ByteSink>(&self, target: &#plan<'__wire_repr_value>, sink: &mut S) -> bool {
                    #runtime::ByteSource::emit_to(&target.#current_plan, sink);
                    true
                }
            }
            impl<'__wire_repr_wire> #runtime::FieldSelection<#view<'__wire_repr_wire>> for #marker {
                #[inline(always)]
                fn visit_ranges<V>(&self, target: &#view<'__wire_repr_wire>, visitor: &mut V)
                where
                    V: FnMut(::core::ops::Range<usize>),
                {
                    let mut cursor = 0usize;
                    #(#view_prior_steps)*
                    #view_geometry
                    let end = cursor.checked_add(<#codec as #runtime::FixedCodec>::WIDTH).expect("view field geometry overflow");
                    visitor(cursor..end);
                }
            }
        }
    });
    let prepare_steps = fields.iter().zip(&plans).zip(&variants).map(|((field, plan), variant)| {
        let field_name = &field.name;
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        quote!(let #plan = <#codec as #runtime::FixedCodec>::plan(self.#field_name).map_err(#encode_error::#variant)?;)
    });
    let geometry_steps = fields.iter().zip(&plans).zip(&gaps).map(|((field, plan), gap)| {
        let length = quote!(#runtime::ByteSource::byte_len(&#plan));
        if let (Some(gap), Some(position)) = (gap, &field.position) {
            let label = field.name.to_string();
            let field_start = match position {
                FieldPosition::Static(position) => quote!(#position),
                FieldPosition::Source(source) => quote! { usize::try_from(self.#source).map_err(|_| #encode_error::PositionNotRepresentable { field: #label, value: self.#source as u128 })? },
            };
            quote! { let field_start = #field_start; if field_start < encoded_len { return Err(#encode_error::PositionBeforeCursor { field: #label, position: field_start, cursor: encoded_len }); } let #gap = field_start - encoded_len; encoded_len = field_start.checked_add(#length).ok_or(#encode_error::LengthOverflow)?; }
        } else if let Some(gap) = gap {
            let padding = field.padding_before;
            let alignment = match field.alignment_before { Some(boundary) => quote!(Some(#boundary)), None => quote!(None::<usize>) };
            quote! { let before_gap = encoded_len; let padded = encoded_len.checked_add(#padding).ok_or(#encode_error::LengthOverflow)?; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; let field_start = padded.checked_add(alignment_padding).ok_or(#encode_error::LengthOverflow)?; let #gap = field_start - before_gap; encoded_len = field_start.checked_add(#length).ok_or(#encode_error::LengthOverflow)?; }
        } else { quote!(encoded_len = encoded_len.checked_add(#length).ok_or(#encode_error::LengthOverflow)?;) }
    });
    let emit_steps = fields
        .iter()
        .zip(&plans)
        .zip(&gaps)
        .map(|((_, plan), gap)| {
            let padding = gap
                .as_ref()
                .map(|gap| quote!(#runtime::ByteSink::fill(sink, 0, self.#gap);));
            quote!(#padding #runtime::ByteSource::emit_to(&self.#plan, sink);)
        });
    let plan_cursor_bounds: Vec<_> = fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>: #runtime::ByteSourceCursor)
            }
            _ => unreachable!(),
        })
        .collect();
    let mut plan_segment_types = Vec::new();
    let mut plan_segment_values = Vec::new();
    for ((field, plan), gap) in fields.iter().zip(&plans).zip(&gaps) {
        if let Some(gap) = gap {
            plan_segment_types
                .push(quote!(::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>>));
            plan_segment_values.push(
                quote!(::core::iter::once(#runtime::ByteSegment::Rest { byte: 0, len: self.#gap })),
            );
        }
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        plan_segment_types.push(quote!(<<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>));
        plan_segment_values.push(quote!(#runtime::ByteSourceCursor::segments(&self.#plan)));
    }
    let plan_segments_type = plan_segment_types
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Chain<#left, #right>))
        .expect("fixed structs have fields");
    let plan_segments_value = plan_segment_values
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Iterator::chain(#left, #right)))
        .expect("fixed structs have fields");
    let encode_variants = fields.iter().zip(&variants).zip(&labels).map(|((field, variant), label)| {
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        quote!(#[doc = concat!("Preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::FixedCodec>::EncodeError),)
    });
    let encode_display_arms = fields.iter().zip(&variants).zip(&labels).map(|((_, variant), label)| quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error:?}", #label),));
    let decode_position_variants = has_positions.then(|| quote! { PositionNotRepresentable { field: &'static str, value: u128 }, PositionBeforeCursor { field: &'static str, position: usize, cursor: usize }, });
    let decode_position_arms = has_positions.then(|| quote! { Self::PositionNotRepresentable { field, value } => write!(formatter, "position {value} for field `{field}` does not fit in usize"), Self::PositionBeforeCursor { field, position, cursor } => write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}"), });
    let encode_position_variants = decode_position_variants.clone();
    let encode_position_arms = decode_position_arms.clone();
    let decode_geometry_variant =
        has_geometry.then(|| quote!(GeometryOverflow { field: &'static str },));
    let decode_geometry_arm = has_geometry.then(|| quote!(Self::GeometryOverflow { field } => write!(formatter, "placement before field `{field}` does not fit in usize"),));

    Ok(quote! {
        /// Typed decoding failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #decode_error { InputTooShort { field: &'static str, required: usize, available: usize }, #decode_position_variants #decode_geometry_variant TrailingBytes { expected: usize, actual: usize }, }
        impl ::core::fmt::Display for #decode_error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self { Self::InputTooShort { field, required, available } => { let required_unit = if *required == 1 { "byte" } else { "bytes" }; let available_unit = if *available == 1 { "byte" } else { "bytes" }; let available_verb = if *available == 1 { "remains" } else { "remain" }; write!(formatter, "field `{field}` needs {required} {required_unit}, but only {available} {available_unit} {available_verb}") }, #decode_position_arms #decode_geometry_arm Self::TrailingBytes { expected, actual } => { let trailing = actual.saturating_sub(*expected); let unit = if trailing == 1 { "byte" } else { "bytes" }; write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation") } } } }
        impl ::core::error::Error for #decode_error {}
        /// Typed encoding-preparation failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #encode_error { #encode_position_variants #(#encode_variants)* LengthOverflow, }
        impl ::core::fmt::Display for #encode_error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self { #encode_position_arms #(#encode_display_arms)* Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"), } } }
        impl ::core::error::Error for #encode_error {}
        /// A bytes-backed validated read view for this wire representation.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> { bytes: &'__wire_repr_wire [u8] }
        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            #[doc = "Returns this view's exact represented bytes."]
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes }
            #[doc = "Returns a byte-selection root for this exact source representation."]
            #[must_use]
            #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }
            #fixed_view_constructor
            #(#getters)*
        }
        impl<'__wire_repr_wire> #runtime::ByteSource for #view<'__wire_repr_wire> {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.as_bytes().len() }
            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { sink.write(self.as_bytes()); }
        }
        impl<'__wire_repr_wire> #runtime::ByteSourceCursor for #view<'__wire_repr_wire> { type Segments<'__wire_repr_source> = ::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>> where Self: '__wire_repr_source; #[inline(always)] fn segments(&self) -> Self::Segments<'_> { ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes())) } }
        impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> { type DecodeError = #decode_error; fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> { let mut remaining = input; #(#decode_steps)* let represented = &input[..input.len() - remaining.len()]; Ok((Self { bytes: represented }, remaining)) } fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError { #decode_error::TrailingBytes { expected: represented, actual: input } } fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes } }
        #validation_impl
        impl #impl_generics #runtime::WireViewType for #self_type { type DecodeError<'__wire_repr_view> = #decode_error; type View<'__wire_repr_view> = #view<'__wire_repr_view>; }
        #[allow(missing_docs)]
        #[doc(hidden)]
        #vis struct #field_proxy<S: #runtime::MarkerScope = #runtime::RootScope> { #(#proxy_fields,)* }
        impl<S: #runtime::MarkerScope> Copy for #field_proxy<S> {}
        impl<S: #runtime::MarkerScope> Clone for #field_proxy<S> { fn clone(&self) -> Self { *self } }
        #[allow(missing_docs)]
        impl<S: #runtime::MarkerScope> #field_proxy<S> { #[doc(hidden)] #vis fn __wire_repr_new() -> Self { Self { #(#proxy_values,)* } } }
        #(#marker_impls)*
        /// A prepared encoding for this wire representation.
        #vis struct #plan<'__wire_repr_value> { #(#plan_fields,)* #(#gap_names: usize,)* encoded_len: usize }
        #[allow(missing_docs)]
        impl #plan<'_> {
            #[must_use]
            #vis const fn encoded_len(&self) -> usize { self.encoded_len }
            #[doc = "Returns a byte-selection root for this prepared representation."]
            #[must_use]
            #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }
        }
        impl<'__wire_repr_value> #runtime::ByteSource for #plan<'__wire_repr_value> { #[inline(always)] fn byte_len(&self) -> usize { self.encoded_len } #[inline(always)] fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { #(#emit_steps)* } }
        impl<'__wire_repr_value> #runtime::ByteSourceCursor for #plan<'__wire_repr_value> where #(#plan_cursor_bounds,)* { type Segments<'__wire_repr_source> = #plan_segments_type where Self: '__wire_repr_source; #[inline(always)] fn segments(&self) -> Self::Segments<'_> { #plan_segments_value } }
        impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> { type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>; fn commit_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::OutputTooShortError> { let required = self.encoded_len; if output.len() < required { return Err(#runtime::OutputTooShortError { required, available: output.len() }); } let (bytes, suffix) = output.split_at_mut(required); #runtime::ByteSource::write_into(&self, bytes); Ok((#runtime::Written::new(bytes), suffix)) } }
        impl #impl_generics #self_type { #[doc = "Starts validating a bytes-backed read view from the supplied input."] #view_signature { #view_request::new(input) } #fixed_sequence_method #cursor_method #[doc = "Consumes this value and prepares an atomic encoding."] #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan<'__wire_repr_value>, #encode_error> where Self: '__wire_repr_value { <Self as #runtime::WireEncode>::prepare(self) } #[doc = "Consumes this value, prepares it, and commits it into `output`."] #vis fn build_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error>> { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) } }
        impl #impl_generics #runtime::WireEncode for #self_type { type EncodeError = #encode_error; type Plan<'__wire_repr_value> = #plan<'__wire_repr_value> where Self: '__wire_repr_value; fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value { #(#prepare_steps)* let mut encoded_len = 0usize; #(#geometry_steps)* Ok(#plan { #(#plans,)* #(#gap_names,)* encoded_len }) } }
    })
}

fn decode_geometry(
    field: &super::super::super::model::Field,
    label: &str,
    decode_error: &proc_macro2::Ident,
) -> TokenStream {
    if let Some(position) = &field.position {
        let (position, conversion) = match position {
            FieldPosition::Static(position) => (quote!(#position), quote!()),
            FieldPosition::Source(source) => (
                quote!(position),
                quote!(let position = usize::try_from(#source).map_err(|_| #decode_error::PositionNotRepresentable { field: #label, value: #source as u128 })?;),
            ),
        };
        quote! { #conversion let represented = input.len() - remaining.len(); if #position < represented { return Err(#decode_error::PositionBeforeCursor { field: #label, position: #position, cursor: represented }); } let gap = #position - represented; let available = remaining.len(); let Some((_, suffix)) = remaining.split_at_checked(gap) else { return Err(#decode_error::InputTooShort { field: #label, required: gap, available }); }; remaining = suffix; }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! { let represented = input.len() - remaining.len(); let padded = represented.checked_add(#padding).ok_or(#decode_error::GeometryOverflow { field: #label })?; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; let gap = #padding.checked_add(alignment_padding).ok_or(#decode_error::GeometryOverflow { field: #label })?; let available = remaining.len(); let Some((_, suffix)) = remaining.split_at_checked(gap) else { return Err(#decode_error::InputTooShort { field: #label, required: gap, available }); }; remaining = suffix; }
    }
}

fn view_getter_cursor_step(
    field: &super::super::super::model::Field,
    runtime: &TokenStream,
) -> TokenStream {
    let codec = match &field.kind {
        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
        _ => unreachable!(),
    };
    let geometry = view_getter_geometry(field, runtime);
    quote! {
        #geometry
        cursor = cursor.checked_add(<#codec as #runtime::FixedCodec>::WIDTH)
            .expect("view field geometry overflow");
    }
}

fn view_getter_geometry(
    field: &super::super::super::model::Field,
    _runtime: &TokenStream,
) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => quote! {
                cursor = usize::try_from(target.#source())
                    .expect("validated position source fits usize");
            },
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! {
            let padded = cursor.checked_add(#padding).expect("view field geometry overflow");
            let alignment_padding = match #alignment {
                Some(boundary) => {
                    let remainder = padded % boundary;
                    if remainder == 0 { 0 } else { boundary - remainder }
                }
                None => 0,
            };
            cursor = padded.checked_add(alignment_padding).expect("view field geometry overflow");
        }
    }
}

fn getter_cursor_step(
    field: &super::super::super::model::Field,
    runtime: &TokenStream,
) -> TokenStream {
    let codec = match &field.kind {
        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
        _ => unreachable!(),
    };
    let geometry = getter_geometry(field, runtime);
    quote! { #geometry cursor += <#codec as #runtime::FixedCodec>::WIDTH; }
}

fn getter_geometry(
    field: &super::super::super::model::Field,
    _runtime: &TokenStream,
) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => {
                quote!(cursor = usize::try_from(self.#source()).expect("validated position source fits usize");)
            }
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! { let padded = cursor + #padding; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; cursor = padded + alignment_padding; }
    }
}
