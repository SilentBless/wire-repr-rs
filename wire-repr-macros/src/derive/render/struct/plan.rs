//! Prepared PLAN representation rendering.

use super::codec_tokens;
use crate::derive::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::Lifetime;

pub(super) struct Input<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) wire_lifetime: Option<&'a Lifetime>,
    pub(super) fields: &'a [Field],
    pub(super) plans: &'a [Ident],
    pub(super) gaps: &'a [Option<Ident>],
    pub(super) gap_names: &'a [&'a Ident],
    pub(super) nested_plan_paths: &'a [Option<TokenStream>],
    pub(super) plan: &'a Ident,
    pub(super) plan_decl_generics: &'a TokenStream,
    pub(super) plan_decl_where: &'a TokenStream,
    pub(super) plan_impl_generics: &'a TokenStream,
    pub(super) plan_impl_type: &'a TokenStream,
    pub(super) field_proxy: &'a Ident,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) declaration: TokenStream,
    pub(super) lifetime_init: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        vis,
        wire_lifetime,
        fields,
        plans,
        gaps,
        gap_names,
        nested_plan_paths,
        plan,
        plan_decl_generics,
        plan_decl_where,
        plan_impl_generics,
        plan_impl_type,
        field_proxy,
        runtime,
    } = input;
    let plan_field_types: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>)
            }
            FieldKind::Nested => {
                let ty = &field.ty;
                if field.operation_input.is_some() {
                    let child_plan = nested_plan_paths[index]
                        .as_ref()
                        .expect("operation-backed nested fields have generated plan paths");
                    quote!(#child_plan)
                } else {
                    quote!(<#ty as #runtime::WireEncode>::Plan<'__wire_repr_value>)
                }
            }
            FieldKind::Prefix(codec) => {
                quote!(<#codec as #runtime::PrefixCodec>::Plan<'__wire_repr_value>)
            }
            FieldKind::Bytes { .. } | FieldKind::Rest => quote!(&'__wire_repr_value [u8]),
        })
        .collect();
    let plan_fields = plans
        .iter()
        .zip(&plan_field_types)
        .map(|(plan, ty)| quote!(#plan: #ty));
    let gap_fields = gap_names.iter().map(|gap| quote!(#gap: usize));
    let lifetime_field = wire_lifetime
        .map(|lifetime| quote!(__wire_repr_lifetime: ::core::marker::PhantomData<&#lifetime ()>,));
    let lifetime_init = wire_lifetime
        .map(|_| quote!(__wire_repr_lifetime: ::core::marker::PhantomData,))
        .unwrap_or_default();
    let emit_steps = fields
        .iter()
        .zip(plans)
        .zip(gaps)
        .map(|((field, plan), gap)| {
            let padding = gap
                .as_ref()
                .map(|gap| quote!(#runtime::ByteSink::fill(sink, 0, self.#gap);));
            let emit = match field.kind {
                FieldKind::Fixed(_)
                | FieldKind::Prefix(_)
                | FieldKind::Bytes { .. }
                | FieldKind::Rest
                | FieldKind::Nested => quote! {
                    #runtime::ByteSource::emit_to(&self.#plan, sink);
                },
            };
            quote! {
                #padding
                #emit
            }
        });
    let plan_cursor_bounds = plan_field_types
        .iter()
        .map(|ty| quote!(#ty: #runtime::ByteSourceCursor));
    let mut plan_segment_types = Vec::new();
    let mut plan_segment_values = Vec::new();
    for ((plan, ty), gap) in plans.iter().zip(&plan_field_types).zip(gaps) {
        if let Some(gap) = gap {
            plan_segment_types
                .push(quote!(::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>>));
            plan_segment_values.push(quote!(::core::iter::once(#runtime::ByteSegment::Rest {
                byte: 0,
                len: self.#gap,
            })));
        }
        plan_segment_types
            .push(quote!(<#ty as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>));
        plan_segment_values.push(quote!(#runtime::ByteSourceCursor::segments(&self.#plan)));
    }
    let plan_segments_type = plan_segment_types
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Chain<#left, #right>))
        .expect("wire structs have fields");
    let plan_segments_value = plan_segment_values
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Iterator::chain(#left, #right)))
        .expect("wire structs have fields");

    Output {
        declaration: quote! {
            /// A prepared encoding for this wire representation.
            #vis struct #plan #plan_decl_generics #plan_decl_where { #(#plan_fields,)* #(#gap_fields,)* #lifetime_field encoded_len: usize }
            impl #plan_impl_generics #plan_impl_type {
                /// Returns the exact encoded byte count.
                #[must_use]
                #vis const fn encoded_len(&self) -> usize { self.encoded_len }
                /// Returns a byte-selection root for this prepared representation.
                #[must_use]
                #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                    #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
                }
            }
            impl #plan_impl_generics #runtime::ByteSource for #plan_impl_type {
                #[inline(always)]
                fn byte_len(&self) -> usize { self.encoded_len }
                #[inline(always)]
                fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { #(#emit_steps)* }
            }
            impl #plan_impl_generics #runtime::ByteSourceCursor for #plan_impl_type
            where
                #(#plan_cursor_bounds,)*
            {
                type Segments<'__wire_repr_source> = #plan_segments_type where Self: '__wire_repr_source;
                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> { #plan_segments_value }
            }
            impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
                type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;
                fn commit_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::OutputTooShortError> {
                    let required = self.encoded_len;
                    if output.len() < required { return Err(#runtime::OutputTooShortError { required, available: output.len() }); }
                    let (bytes, suffix) = output.split_at_mut(required);
                    #runtime::ByteSource::write_into(&self, bytes);
                    Ok((#runtime::Written::new(bytes), suffix))
                }
            }
        },
        lifetime_init,
    }
}
