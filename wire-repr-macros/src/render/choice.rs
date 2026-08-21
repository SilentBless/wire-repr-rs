//! Tagged-choice Rust token rendering.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::codec_tokens;
use crate::ir::{Choice, ChoiceCase, IntegerType};

struct ParseContext<'a> {
    name: &'a syn::Ident,
    variant: &'a syn::Ident,
    case: &'a syn::Ident,
    error: &'a syn::Ident,
    width: &'a TokenStream,
    raw: &'a TokenStream,
}

struct PrepareContext<'a> {
    selection: &'a syn::Ident,
    prepared: &'a syn::Ident,
    codec: &'a TokenStream,
    case: &'a syn::Ident,
    error: &'a syn::Ident,
    plan: &'a syn::Ident,
    dynamic_prepare: &'a TokenStream,
    dynamic: bool,
}

pub(super) fn render_choice(choice: &Choice) -> TokenStream {
    let docs = &choice.docs;
    let visibility = &choice.visibility;
    let name = &choice.choice_name;
    let request = &choice.view_request_name;
    let variant = &choice.variant_name;
    let built = &choice.built_name;
    let case = &choice.case_name;
    let error = &choice.error_name;
    let builder = &choice.builder_name;
    let plan = &choice.plan_name;
    let write_error = &choice.write_error_name;
    let unknown = &choice.unknown_name;
    let tag_name = &choice.tag.name;
    let raw = raw_type(choice.tag.raw_type);
    let codec = codec_tokens(&crate::ir::Codec::Builtin(choice.tag.codec));
    let tag_width = quote!(<#codec as ::wire_repr::FixedCodec>::WIDTH);
    let selection = format_ident!("__{}Selection", name);
    let prepared = format_ident!("__{}Prepared", name);
    let dynamic = choice.resolver.as_ref();
    let parse_context = ParseContext {
        name,
        variant,
        case,
        error,
        width: &tag_width,
        raw: &raw,
    };

    let case_variants: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| {
            let entry_name = &entry.variant_name;
            let docs = &entry.docs;
            quote!(#[doc = "A declared choice case."] #(#docs)* #entry_name,)
        })
        .collect();
    let variant_variants: Vec<_> = choice.cases.iter().map(variant_variant).collect();
    let selection_variants: Vec<_> = choice.cases.iter().map(selection_variant).collect();
    let prepared_variants: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| prepared_variant(entry, &codec))
        .collect();
    let constructors: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| constructor(entry, visibility, &selection, dynamic.is_some()))
        .collect();
    let unknown_constructor =
        unknown_constructor(visibility, &selection, unknown, dynamic.is_some());
    let static_parse: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| parse_arm(entry, false, &parse_context))
        .collect();
    let dynamic_parse: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| parse_arm(entry, true, &parse_context))
        .collect();
    let commit_arms: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| commit_arm(entry, &prepared, &codec, case, tag_width.clone()))
        .collect();
    let body_errors: Vec<_> = choice.cases.iter().filter_map(body_error).collect();

    let case_from_variant: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| {
            let entry_name = &entry.variant_name;
            if entry.body.is_some() {
                quote!(#variant::#entry_name(_) => #case::#entry_name,)
            } else {
                quote!(#variant::#entry_name => #case::#entry_name,)
            }
        })
        .collect();

    let (
        request_field,
        request_empty,
        request_setter,
        parse_resolution,
        builder_field,
        builder_destructure,
        builder_setter,
        dynamic_prepare,
        resolver_error,
    ) = if let Some(context) = dynamic {
        let context_name = &context.name;
        let ty = &context.referent;
        let docs = &context.docs;
        (
            quote!(resolver: ::core::option::Option<&'context #ty>,),
            quote!(resolver: ::core::option::Option::None,),
            quote!(#[doc = "Supplies the runtime discriminant resolver."] #(#docs)* #[must_use] #visibility fn #context_name<'next>(self, value: &'next #ty) -> #request<'wire, 'next> { #request { input: self.input, unknown: self.unknown, marker: ::core::marker::PhantomData, resolver: ::core::option::Option::Some(value) } }),
            quote! { let resolver: &#ty = match self.resolver { ::core::option::Option::Some(value) => value, ::core::option::Option::None => return Err(#error::MissingContext) }; match <#ty as ::wire_repr::Discriminant<#raw, #case>>::resolve(resolver, raw) { Ok(::core::option::Option::Some(selected)) => match selected { #(#dynamic_parse)* #case::Unknown => unknown(input, raw, self.unknown), }, Ok(::core::option::Option::None) => unknown(input, raw, self.unknown), Err(value) => Err(#error::Resolver(value)), } },
            quote!(resolver: ::core::option::Option<&'value #ty>,),
            quote!(resolver,),
            quote!(#[doc = "Supplies the runtime discriminant resolver used while preparing."] #(#docs)* #[must_use] #visibility fn #context_name(mut self, value: &'value #ty) -> Self { self.resolver = ::core::option::Option::Some(value); self }),
            quote!(let resolver: &#ty = match resolver { ::core::option::Option::Some(value) => value, ::core::option::Option::None => return Err(#write_error::MissingContext) }; let raw = <#ty as ::wire_repr::Discriminant<#raw, #case>>::encode(resolver, selected_case).map_err(#write_error::Resolver)?;),
            quote!(Resolver(<#ty as ::wire_repr::Discriminant<#raw, #case>>::Error),),
        )
    } else {
        (
            quote!(),
            quote!(),
            quote!(),
            quote!(match raw { #(#static_parse)* _ => unknown(input, raw, self.unknown), }),
            quote!(),
            quote!(),
            quote!(),
            quote!(),
            quote!(),
        )
    };

    let parse_context_errors = if dynamic.is_some() {
        quote!(#[doc = "A dynamic resolver was not supplied."] MissingContext, #[doc = "The dynamic resolver failed."] #resolver_error)
    } else {
        quote!()
    };
    let write_context_errors = if dynamic.is_some() {
        quote!(#[doc = "No dynamic resolver was supplied."] MissingContext, #[doc = "The dynamic resolver failed."] #resolver_error)
    } else {
        quote!()
    };
    let write_display_context_arms = if dynamic.is_some() {
        quote!(Self::MissingContext => write!(formatter, "missing dynamic resolver"), Self::Resolver(value) => write!(formatter, "dynamic resolver failed: {value:?}"),)
    } else {
        quote!()
    };

    let prepare_context = PrepareContext {
        selection: &selection,
        prepared: &prepared,
        codec: &codec,
        case,
        error: write_error,
        plan,
        dynamic_prepare: &dynamic_prepare,
        dynamic: dynamic.is_some(),
    };
    let prepare_arms: Vec<_> = choice
        .cases
        .iter()
        .map(|entry| prepare_arm(entry, &prepare_context))
        .collect();

    quote! {
        #[doc = "A semantic tagged-choice case."]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #visibility enum #case { #(#case_variants)* #[doc = "An unrecognized raw tag."] Unknown }
        #[doc = "Exact retained bytes for an unrecognized tagged choice."]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #visibility struct #unknown<'wire> { raw: #raw, bytes: &'wire [u8] }
        impl<'wire> #unknown<'wire> { #[doc = "Creates an unknown choice body from parser-retained bytes."] #[must_use] fn new(raw: #raw, bytes: &'wire [u8]) -> Self { Self { raw, bytes } } #[doc = "Returns the raw tag."] #[must_use] #visibility fn #tag_name(&self) -> #raw { self.raw } #[doc = "Returns the exact retained body bytes."] #[must_use] #visibility fn as_bytes(&self) -> &'wire [u8] { self.bytes } }
        #[doc = "The selected decoded or retained choice body."]
        #visibility enum #variant<'wire> { #(#variant_variants)* #[doc = "An unrecognized retained body."] Unknown(#unknown<'wire>) }
        #[doc = "An immutable byte-backed tagged choice view."] #(#docs)*
        #visibility struct #name<'wire> { bytes: &'wire [u8], raw: #raw, variant: #variant<'wire> }
        #[doc(hidden)] #visibility struct #request<'wire, 'context> { input: &'wire [u8], unknown: ::wire_repr::UnknownBody, marker: ::core::marker::PhantomData<&'context ()>, #request_field }
        #[doc = "Reports why parsing a tagged choice failed."]
        #[derive(Debug)] #visibility enum #error { #[doc = "The tag bytes are unavailable."] TagTooShort { #[doc = "Required tag width."] expected: usize, #[doc = "Available input bytes."] actual: usize }, #parse_context_errors #[doc = "The unknown policy rejects this raw tag."] UnknownTag { #[doc = "The raw tag."] raw: #raw }, #[doc = "An exact unknown body is too short."] UnknownBodyTooShort { #[doc = "Required body bytes."] expected: usize, #[doc = "Available body bytes."] actual: usize }, #(#body_errors)* #[doc = "Bytes trail an otherwise valid choice."] TrailingBytes { #[doc = "Represented bytes."] expected: usize, #[doc = "Input bytes."] actual: usize } }
        impl ::core::fmt::Display for #error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { write!(formatter, "tagged choice parse failed: {self:?}") } } impl ::core::error::Error for #error {}
        impl<'wire> #name<'wire> { #[doc = "Requests a choice view over input."] #[must_use] #visibility fn view(input: &'wire [u8]) -> #request<'wire, 'static> { #request { input, unknown: ::wire_repr::UnknownBody::Reject, marker: ::core::marker::PhantomData, #request_empty } } #[doc = "Returns exact represented bytes."] #[must_use] #visibility fn as_bytes(&self) -> &'wire [u8] { self.bytes } #[doc = "Returns the raw tag."] #[must_use] #visibility fn #tag_name(&self) -> #raw { self.raw } #[doc = "Returns the selected case."] #[must_use] #visibility fn case(&self) -> #case { match &self.variant { #(#case_from_variant)* #variant::Unknown(_) => #case::Unknown } } #[doc = "Returns the selected body variant."] #[must_use] #visibility fn variant(&self) -> &#variant<'wire> { &self.variant } }
        impl<'wire, 'context> #request<'wire, 'context> { #[doc = "Sets unknown-body handling."] #[must_use] #visibility fn unknown_body(mut self, value: ::wire_repr::UnknownBody) -> Self { self.unknown = value; self } #[doc = "Accepts an exact unknown body length."] #[must_use] #visibility fn accept_unknown_exact(self, length: usize) -> Self { self.unknown_body(::wire_repr::UnknownBody::Exact(length)) } #[doc = "Accepts the unknown body remainder."] #[must_use] #visibility fn accept_unknown_remainder(self) -> Self { self.unknown_body(::wire_repr::UnknownBody::Remainder) } #[doc = "Parses a leading choice and returns its suffix."] #[must_use] #visibility fn with_remainder(self) -> ::core::result::Result<(#name<'wire>, &'wire [u8]), #error> { let input=self.input; if input.len() < #tag_width { return Err(#error::TagTooShort { expected: #tag_width, actual: input.len() }); } let raw: #raw = <#codec as ::wire_repr::FixedCodec>::decode(&input[..#tag_width]); fn unknown<'wire>(input: &'wire [u8], raw: #raw, policy: ::wire_repr::UnknownBody) -> ::core::result::Result<(#name<'wire>, &'wire [u8]), #error> { let (body, suffix) = match policy { ::wire_repr::UnknownBody::Reject => return Err(#error::UnknownTag { raw }), ::wire_repr::UnknownBody::Exact(length) => { let available=input.len() - #tag_width; if available < length { return Err(#error::UnknownBodyTooShort { expected: length, actual: available }); } input[#tag_width..].split_at(length) }, ::wire_repr::UnknownBody::Remainder => (&input[#tag_width..], &input[input.len()..]), }; let represented=input.len()-suffix.len(); Ok((#name { bytes: &input[..represented], raw, variant: #variant::Unknown(#unknown::new(raw, body)) }, suffix)) } #parse_resolution } #[doc = "Parses one complete choice with no trailing bytes."] #[must_use] #visibility fn without_trailing(self) -> ::core::result::Result<#name<'wire>, #error> { let actual=self.input.len(); let (view, suffix)=self.with_remainder()?; if suffix.is_empty() { Ok(view) } else { Err(#error::TrailingBytes { expected: actual-suffix.len(), actual }) } } }
        impl<'wire> #request<'wire, 'static> { #request_setter }
        #[doc = "A builder holding an already prepared choice body."] #visibility struct #builder<'value> { selection: #selection<'value>, #builder_field }
        enum #selection<'value> { #(#selection_variants)* Unknown(#unknown<'value>) }
        #[doc = "A prepared tagged-choice encoding."] #visibility struct #plan<'value> { selected: #prepared<'value>, encoded_len: usize }
        enum #prepared<'value> { #(#prepared_variants)* Unknown { raw: #raw, tag: <#codec as ::wire_repr::FixedCodec>::Plan<'value>, body: &'value [u8] } }
        #[doc = "Reports why a tagged choice could not be written."] #[derive(Debug)] #visibility enum #write_error { #write_context_errors #[doc = "Tag planning failed."] TagEncode(<#codec as ::wire_repr::FixedCodec>::EncodeError), #[doc = "Tag plan width is invalid."] InvalidTagPlanLength { #[doc = "Expected width."] expected: usize, #[doc = "Actual plan width."] actual: usize }, #[doc = "Tag and body lengths overflow."] LengthOverflow, #[doc = "Output is too short."] OutputTooShort { #[doc = "Required bytes."] required: usize, #[doc = "Available bytes."] available: usize } }
        impl ::core::fmt::Display for #write_error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self { #write_display_context_arms Self::TagEncode(value) => write!(formatter, "tag encoding failed: {value:?}"), Self::InvalidTagPlanLength { expected, actual } => write!(formatter, "tag plan length: expected {expected} bytes, got {actual}"), Self::LengthOverflow => formatter.write_str("tag and body lengths overflow"), Self::OutputTooShort { required, available } => write!(formatter, "output too short: need {required} bytes, got {available}"), } } }
        impl ::core::error::Error for #write_error {}
        impl<'value> #builder<'value> { #(#constructors)* #unknown_constructor #builder_setter #[doc = "Prepares the tag around the already prepared body."] #visibility fn prepare(self) -> ::core::result::Result<#plan<'value>, #write_error> { let Self { selection, #builder_destructure } = self; match selection { #(#prepare_arms)* #selection::Unknown(unknown) => { let raw = unknown.#tag_name(); let body = unknown.as_bytes(); let tag = <#codec as ::wire_repr::FixedCodec>::plan(raw).map_err(#write_error::TagEncode)?; if ::wire_repr::EncodePlan::encoded_len(&tag) != <#codec as ::wire_repr::FixedCodec>::WIDTH { return Err(#write_error::InvalidTagPlanLength { expected: <#codec as ::wire_repr::FixedCodec>::WIDTH, actual: ::wire_repr::EncodePlan::encoded_len(&tag) }); } let encoded_len = <#codec as ::wire_repr::FixedCodec>::WIDTH.checked_add(body.len()).ok_or(#write_error::LengthOverflow)?; Ok(#plan { selected: #prepared::Unknown { raw, tag, body }, encoded_len }) } } } #[doc = "Prepares then commits into output."] #visibility fn build_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#built<'output>, &'output mut [u8]), #write_error> { self.prepare()?.commit_into(output).map_err(|value| #write_error::OutputTooShort { required: value.required, available: value.available }) } }
        impl<'value> #plan<'value> { #[doc = "Returns exact encoded length."] #[must_use] #visibility fn encoded_len(&self) -> usize { self.encoded_len } #[doc = "Commits into output."] #visibility fn commit_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#built<'output>, &'output mut [u8]), ::wire_repr::OutputTooShortError> { <Self as ::wire_repr::PreparedLayout>::commit_into(self, output) } }
        #[doc = "The immutable bytes produced by a prepared choice."] #visibility struct #built<'output> { bytes: &'output [u8], raw: #raw, case: #case }
        impl<'output> #built<'output> { #[doc = "Returns complete choice bytes."] #[must_use] #visibility fn as_bytes(&self) -> &'output [u8] { self.bytes } #[doc = "Returns encoded raw tag."] #[must_use] #visibility fn #tag_name(&self) -> #raw { self.raw } #[doc = "Returns prepared semantic case."] #[must_use] #visibility fn case(&self) -> #case { self.case } }
        impl<'value> ::wire_repr::PreparedLayout for #plan<'value> { type ViewMut<'output> = #built<'output>; fn encoded_len(&self) -> usize { self.encoded_len } fn commit_into<'output>(self, output: &'output mut [u8]) -> ::core::result::Result<(#built<'output>, &'output mut [u8]), ::wire_repr::OutputTooShortError> { if output.len() < self.encoded_len { return Err(::wire_repr::OutputTooShortError { required: self.encoded_len, available: output.len() }); } let (bytes, suffix)=output.split_at_mut(self.encoded_len); let (raw, case) = match self.selected { #(#commit_arms)* #prepared::Unknown { raw, tag, body } => { let (tag_bytes, body_bytes) = bytes.split_at_mut(#tag_width); ::wire_repr::EncodePlan::write_into(&tag, tag_bytes); body_bytes.copy_from_slice(body); (raw, #case::Unknown) } }; Ok((#built { bytes, raw, case }, suffix)) } }
    }
}

fn raw_type(raw: IntegerType) -> TokenStream {
    match raw {
        IntegerType::U8 => quote!(u8),
        IntegerType::U16 => quote!(u16),
        IntegerType::U32 => quote!(u32),
        IntegerType::U64 => quote!(u64),
        IntegerType::U128 => quote!(u128),
        _ => quote!(u8),
    }
}
fn variant_variant(entry: &ChoiceCase) -> TokenStream {
    let name = &entry.variant_name;
    match &entry.body {
        Some(body) => {
            let layout = &body.layout_name;
            quote!(#[doc = "A decoded declared body."] #name(#layout<'wire>),)
        }
        None => quote!(#[doc = "A unit declared body."] #name,),
    }
}
fn selection_variant(entry: &ChoiceCase) -> TokenStream {
    let name = &entry.variant_name;
    match &entry.body {
        Some(body) => {
            let plan = &body.plan_name;
            quote!(#name(#plan<'value>),)
        }
        None => quote!(#name,),
    }
}
fn prepared_variant(entry: &ChoiceCase, codec: &TokenStream) -> TokenStream {
    let name = &entry.variant_name;
    match &entry.body {
        Some(body) => {
            let plan = &body.plan_name;
            quote!(#name { tag: <#codec as ::wire_repr::FixedCodec>::Plan<'value>, body: #plan<'value> },)
        }
        None => quote!(#name { tag: <#codec as ::wire_repr::FixedCodec>::Plan<'value> },),
    }
}
fn constructor(
    entry: &ChoiceCase,
    visibility: &syn::Visibility,
    selection: &syn::Ident,
    dynamic: bool,
) -> TokenStream {
    let method = &entry.constructor_name;
    let variant = &entry.variant_name;
    let init = if dynamic {
        quote!(resolver: ::core::option::Option::None,)
    } else {
        quote!()
    };
    match &entry.body {
        Some(body) => {
            let plan = &body.plan_name;
            quote!(#[doc = "Selects this prepared body."] #[must_use] #visibility fn #method(body: #plan<'value>) -> Self { Self { selection: #selection::#variant(body), #init } })
        }
        None => {
            quote!(#[doc = "Selects this unit case."] #[must_use] #visibility fn #method() -> Self { Self { selection: #selection::#variant, #init } })
        }
    }
}
fn unknown_constructor(
    visibility: &syn::Visibility,
    selection: &syn::Ident,
    unknown: &syn::Ident,
    dynamic: bool,
) -> TokenStream {
    let init = if dynamic {
        quote!(resolver: ::core::option::Option::None,)
    } else {
        quote!()
    };
    quote!(#[doc = "Selects an exact retained unknown body."] #[must_use] #visibility fn unknown(unknown: #unknown<'value>) -> Self { Self { selection: #selection::Unknown(unknown), #init } })
}

fn parse_arm(entry: &ChoiceCase, dynamic: bool, context: &ParseContext<'_>) -> TokenStream {
    let case = &entry.variant_name;
    let pattern = if dynamic {
        let case_enum = context.case;
        quote!(#case_enum::#case)
    } else {
        let value = entry.value.unwrap_or(0);
        let raw = context.raw;
        quote!(_ if raw == (#value as #raw))
    };
    let name = context.name;
    let variant = context.variant;
    let error = context.error;
    let width = context.width;
    match &entry.body {
        Some(body) => {
            let layout = &body.layout_name;
            quote!(#pattern => { let (body, suffix)=#layout::view(&input[#width..]).with_remainder().map_err(#error::#case)?; let represented=input.len()-suffix.len(); Ok((#name { bytes: &input[..represented], raw, variant: #variant::#case(body) }, suffix)) },)
        }
        None => {
            quote!(#pattern => Ok((#name { bytes: &input[..#width], raw, variant: #variant::#case }, &input[#width..])),)
        }
    }
}
fn body_error(entry: &ChoiceCase) -> Option<TokenStream> {
    let body = entry.body.as_ref()?;
    let case = &entry.variant_name;
    let error = &body.error_name;
    Some(quote!(#[doc = "A declared body failed parsing."] #case(#error),))
}
fn prepare_arm(entry: &ChoiceCase, context: &PrepareContext<'_>) -> TokenStream {
    let variant = &entry.variant_name;
    let select_case = if context.dynamic {
        let case = context.case;
        quote!(let selected_case = #case::#variant;)
    } else {
        quote!()
    };
    let raw = if context.dynamic {
        let dynamic_prepare = context.dynamic_prepare;
        quote!(#dynamic_prepare)
    } else {
        quote!()
    };
    let raw_static = entry.value.unwrap_or(0);
    let choose_raw = if context.dynamic {
        quote!()
    } else {
        quote!(let raw = #raw_static as _;)
    };
    let selection = context.selection;
    let prepared = context.prepared;
    let codec = context.codec;
    let error = context.error;
    let output_plan = context.plan;
    match &entry.body {
        Some(_) => {
            quote!(#selection::#variant(body) => { #select_case #choose_raw #raw let tag=<#codec as ::wire_repr::FixedCodec>::plan(raw).map_err(#error::TagEncode)?; if ::wire_repr::EncodePlan::encoded_len(&tag) != <#codec as ::wire_repr::FixedCodec>::WIDTH { return Err(#error::InvalidTagPlanLength { expected: <#codec as ::wire_repr::FixedCodec>::WIDTH, actual: ::wire_repr::EncodePlan::encoded_len(&tag) }); } let encoded_len=<#codec as ::wire_repr::FixedCodec>::WIDTH.checked_add(::wire_repr::PreparedLayout::encoded_len(&body)).ok_or(#error::LengthOverflow)?; Ok(#output_plan { selected: #prepared::#variant { tag, body }, encoded_len }) },)
        }
        None => {
            quote!(#selection::#variant => { #select_case #choose_raw #raw let tag=<#codec as ::wire_repr::FixedCodec>::plan(raw).map_err(#error::TagEncode)?; if ::wire_repr::EncodePlan::encoded_len(&tag) != <#codec as ::wire_repr::FixedCodec>::WIDTH { return Err(#error::InvalidTagPlanLength { expected: <#codec as ::wire_repr::FixedCodec>::WIDTH, actual: ::wire_repr::EncodePlan::encoded_len(&tag) }); } Ok(#output_plan { selected: #prepared::#variant { tag }, encoded_len: <#codec as ::wire_repr::FixedCodec>::WIDTH }) },)
        }
    }
}
fn commit_arm(
    entry: &ChoiceCase,
    prepared: &syn::Ident,
    codec: &TokenStream,
    case: &syn::Ident,
    width: TokenStream,
) -> TokenStream {
    let variant = &entry.variant_name;
    match &entry.body {
        Some(_) => {
            quote!(#prepared::#variant { tag, body } => { let body_len=::wire_repr::PreparedLayout::encoded_len(&body); let (tag_bytes, body_bytes)=bytes.split_at_mut(#width); ::wire_repr::EncodePlan::write_into(&tag, tag_bytes); match ::wire_repr::PreparedLayout::commit_into(body, &mut body_bytes[..body_len]) { Ok(_) => {}, Err(value) => return Err(value), } (<#codec as ::wire_repr::FixedCodec>::decode(tag_bytes), #case::#variant) },)
        }
        None => {
            quote!(#prepared::#variant { tag } => { ::wire_repr::EncodePlan::write_into(&tag, bytes); (<#codec as ::wire_repr::FixedCodec>::decode(bytes), #case::#variant) },)
        }
    }
}
