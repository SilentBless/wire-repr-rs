//! Fixed-struct field-selection token rendering.

use super::super::super::super::model::{Field, FieldKind};
use super::geometry::{view_getter_cursor_step, view_getter_geometry};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) types: Types<'a>,
    pub(super) surface: Surface<'a>,
}

pub(super) struct Schema<'a> {
    pub(super) fields: &'a [Field],
    pub(super) markers: &'a [Ident],
    pub(super) plans: &'a [Ident],
    pub(super) gaps: &'a [Option<Ident>],
}

pub(super) struct Types<'a> {
    pub(super) field_proxy: &'a Ident,
    pub(super) view: &'a Ident,
    pub(super) plan: &'a Ident,
}

pub(super) struct Surface<'a> {
    pub(super) runtime: &'a TokenStream,
    pub(super) vis: &'a syn::Visibility,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        schema:
            Schema {
                fields,
                markers,
                plans,
                gaps,
            },
        types: Types {
            field_proxy,
            view,
            plan,
        },
        surface: Surface { runtime, vis },
    } = input;
    let proxy_fields = fields.iter().zip(markers).map(|(field, marker)| {
        let field = &field.name;
        quote!(#vis #field: <S as #runtime::MarkerScope>::Wrap<#marker>)
    });
    let proxy_values = fields.iter().zip(markers).map(|(field, marker)| {
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
            FieldKind::Fixed(codec) => super::super::codec_tokens(codec, runtime),
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

    quote! {
        #[allow(missing_docs)]
        #[doc(hidden)]
        #vis struct #field_proxy<S: #runtime::MarkerScope = #runtime::RootScope> {
            #(#proxy_fields,)*
        }

        impl<S: #runtime::MarkerScope> Copy for #field_proxy<S> {}

        impl<S: #runtime::MarkerScope> Clone for #field_proxy<S> {
            fn clone(&self) -> Self {
                *self
            }
        }

        #[allow(missing_docs)]
        impl<S: #runtime::MarkerScope> #field_proxy<S> {
            #[doc(hidden)]
            #vis fn __wire_repr_new() -> Self {
                Self {
                    #(#proxy_values,)*
                }
            }
        }
        #(#marker_impls)*
    }
}
