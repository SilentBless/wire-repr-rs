//! Generated PLAN/VIEW typed field-selection rendering.

use super::super::super::model::{Field, FieldKind, FieldPosition};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) geometry: Geometry<'a>,
    pub(super) types: Types<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) name: &'a Ident,
    pub(super) vis: &'a syn::Visibility,
    pub(super) fields: &'a [Field],
}

pub(super) struct Geometry<'a> {
    pub(super) plans: &'a [Ident],
    pub(super) gaps: &'a [Option<Ident>],
    pub(super) nested_fields_paths: &'a [Option<TokenStream>],
    pub(super) nested_view_paths: &'a [Option<TokenStream>],
    pub(super) nested_plan_paths: &'a [Option<TokenStream>],
}

pub(super) struct Types<'a> {
    pub(super) selection_impl_generics: &'a TokenStream,
    pub(super) selection_plan_type: &'a TokenStream,
    pub(super) view: &'a Ident,
}

pub(super) struct Output {
    pub(super) field_proxy: Ident,
    pub(super) declaration: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        schema: Schema { name, vis, fields },
        geometry:
            Geometry {
                plans,
                gaps,
                nested_fields_paths,
                nested_view_paths,
                nested_plan_paths,
            },
        types:
            Types {
                selection_impl_generics,
                selection_plan_type,
                view,
            },
        runtime,
    } = input;
    let field_proxy = format_ident!("{name}Fields");
    let markers: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("__WireRepr{name}Field{index}"))
        .collect();
    let proxy_fields = fields
        .iter()
        .enumerate()
        .zip(&markers)
        .map(|((index, field), marker)| {
            let field_name = &field.name;
            if matches!(field.kind, FieldKind::Nested) {
                let child_fields = nested_fields_paths[index]
                    .as_ref()
                    .expect("nested fields have generated field proxy paths");
                quote!(
                    #vis #field_name: #runtime::NestedField<
                        <S as #runtime::MarkerScope>::Wrap<#marker>,
                        #child_fields<#runtime::Through<S, #marker>>
                    >
                )
            } else {
                quote!(#vis #field_name: <S as #runtime::MarkerScope>::Wrap<#marker>)
            }
        });
    let proxy_values = fields
        .iter()
        .enumerate()
        .zip(&markers)
        .map(|((index, field), marker)| {
            let field_name = &field.name;
            if matches!(field.kind, FieldKind::Nested) {
                let child_fields = nested_fields_paths[index]
                    .as_ref()
                    .expect("nested fields have generated field proxy paths");
                quote!(
                    #field_name: #runtime::NestedField::new(
                        <S as #runtime::MarkerScope>::wrap(#marker),
                        #child_fields::<#runtime::Through<S, #marker>>::__wire_repr_new()
                    )
                )
            } else {
                quote!(#field_name: <S as #runtime::MarkerScope>::wrap(#marker))
            }
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
        let current_plan = &plans[index];
        let current_gap = &gaps[index];
        let gap = current_gap.as_ref().map(|gap| quote! {
            cursor = cursor.checked_add(target.#gap).expect("prepared field geometry overflow");
        });
        let selection = if matches!(field.kind, FieldKind::Nested) {
            let child_plan = if field.operation_input.is_some() {
                let child_plan = nested_plan_paths[index]
                    .as_ref()
                    .expect("operation-backed nested fields have generated plan paths");
                quote!(#child_plan)
            } else {
                let ty = &field.ty;
                quote!(<#ty as #runtime::WireEncode>::Plan<'__wire_repr_value>)
            };
            quote! {
                impl #selection_impl_generics #runtime::FieldProjection<#selection_plan_type> for #marker {
                    type Inner = #child_plan;
                    fn project<'a>(target: &'a #selection_plan_type) -> (::core::ops::Range<usize>, &'a Self::Inner) {
                        let mut cursor = 0usize;
                        #(#prior_steps)*
                        #gap
                        let end = cursor.checked_add(#runtime::ByteSource::byte_len(&target.#current_plan)).expect("prepared field geometry overflow");
                        (cursor..end, &target.#current_plan)
                    }
                }
                impl #selection_impl_generics #runtime::FieldSelection<#selection_plan_type> for #marker {
                    #[inline(always)]
                    fn visit_ranges<V>(&self, target: &#selection_plan_type, visitor: &mut V)
                    where V: FnMut(::core::ops::Range<usize>) {
                        let (span, _) = <Self as #runtime::FieldProjection<#selection_plan_type>>::project(target);
                        visitor(span);
                    }
                    #[inline(always)]
                    fn direct_len(&self, target: &#selection_plan_type) -> Option<usize> {
                        Some(#runtime::ByteSource::byte_len(&target.#current_plan))
                    }
                    #[inline(always)]
                    fn emit_direct<S: #runtime::ByteSink>(&self, target: &#selection_plan_type, sink: &mut S) -> bool {
                        #runtime::ByteSource::emit_to(&target.#current_plan, sink);
                        true
                    }
                }
            }
        } else {
            quote! {
                impl #selection_impl_generics #runtime::FieldSelection<#selection_plan_type> for #marker {
                    #[inline(always)]
                    fn visit_ranges<V>(&self, target: &#selection_plan_type, visitor: &mut V)
                    where V: FnMut(::core::ops::Range<usize>) {
                        let mut cursor = 0usize;
                        #(#prior_steps)*
                        #gap
                        let end = cursor.checked_add(#runtime::ByteSource::byte_len(&target.#current_plan)).expect("prepared field geometry overflow");
                        visitor(cursor..end);
                    }
                    #[inline(always)]
                    fn direct_len(&self, target: &#selection_plan_type) -> Option<usize> {
                        Some(#runtime::ByteSource::byte_len(&target.#current_plan))
                    }
                    #[inline(always)]
                    fn emit_direct<S: #runtime::ByteSink>(&self, target: &#selection_plan_type, sink: &mut S) -> bool {
                        #runtime::ByteSource::emit_to(&target.#current_plan, sink);
                        true
                    }
                }
            }
        };
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
            #selection
        }
    });
    let borrowed_view_marker_impls: Vec<_> = fields.iter().enumerate().map(|(index, field)| {
        let marker = &markers[index];
        let prior_steps = (0..index).map(|prior_index| {
            let prior = &fields[prior_index];
            let prior_geometry = view_geometry(prior, view);
            let stored = format_ident!("field_{prior_index}");
            let length = if matches!(prior.kind, FieldKind::Nested) {
                quote!(target.#stored.as_bytes().len())
            } else {
                quote!(target.#stored.len())
            };
            quote! {
                #prior_geometry
                cursor = cursor.checked_add(#length).expect("view field geometry overflow");
            }
        });
        let geometry = view_geometry(field, view);
        let stored = format_ident!("field_{index}");
        let length = if matches!(field.kind, FieldKind::Nested) {
            quote!(target.#stored.as_bytes().len())
        } else {
            quote!(target.#stored.len())
        };
        let selection = if matches!(field.kind, FieldKind::Nested) {
            let child_view = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            quote! {
                impl<'__wire_repr_wire> #runtime::FieldProjection<#view<'__wire_repr_wire>> for #marker {
                    type Inner = #child_view<'__wire_repr_wire>;
                    fn project<'a>(target: &'a #view<'__wire_repr_wire>) -> (::core::ops::Range<usize>, &'a Self::Inner) {
                        let mut cursor = 0usize;
                        #(#prior_steps)*
                        #geometry
                        let end = cursor.checked_add(#length).expect("view field geometry overflow");
                        (cursor..end, &target.#stored)
                    }
                }
                impl<'__wire_repr_wire> #runtime::FieldSelection<#view<'__wire_repr_wire>> for #marker {
                    #[inline(always)]
                    fn visit_ranges<V>(&self, target: &#view<'__wire_repr_wire>, visitor: &mut V)
                    where V: FnMut(::core::ops::Range<usize>) {
                        let (span, _) = <Self as #runtime::FieldProjection<#view<'__wire_repr_wire>>>::project(target);
                        visitor(span);
                    }
                }
            }
        } else {
            quote! {
                impl<'__wire_repr_wire> #runtime::FieldSelection<#view<'__wire_repr_wire>> for #marker {
                    #[inline(always)]
                    fn visit_ranges<V>(&self, target: &#view<'__wire_repr_wire>, visitor: &mut V)
                    where V: FnMut(::core::ops::Range<usize>) {
                        let mut cursor = 0usize;
                        #(#prior_steps)*
                        #geometry
                        let end = cursor.checked_add(#length).expect("view field geometry overflow");
                        visitor(cursor..end);
                    }
                }
            }
        };
        quote!(#selection)
    }).collect();
    let view_marker_impls = if cfg!(feature = "bytes") {
        fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let marker = &markers[index];
                let stored = format_ident!("field_{index}");
                if matches!(field.kind, FieldKind::Nested) {
                    let child_view = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    let range = format_ident!("field_{index}_range");
                    quote! {
                        impl #runtime::FieldProjection<#view> for #marker {
                            type Inner = #child_view;
                            fn project<'a>(target: &'a #view) -> (::core::ops::Range<usize>, &'a Self::Inner) {
                                (target.#range.clone(), &target.#stored)
                            }
                        }
                        impl #runtime::FieldSelection<#view> for #marker {
                            #[inline(always)]
                            fn visit_ranges<V>(&self, target: &#view, visitor: &mut V) where V: FnMut(::core::ops::Range<usize>) {
                                let (span, _) = <Self as #runtime::FieldProjection<#view>>::project(target);
                                visitor(span);
                            }
                        }
                    }
                } else {
                    quote! {
                        impl #runtime::FieldSelection<#view> for #marker {
                            #[inline(always)]
                            fn visit_ranges<V>(&self, target: &#view, visitor: &mut V) where V: FnMut(::core::ops::Range<usize>) {
                                visitor(target.#stored.clone());
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
    } else {
        borrowed_view_marker_impls
    };
    let declaration = quote! {
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
        #(#view_marker_impls)*
    };

    Output {
        field_proxy,
        declaration,
    }
}

fn view_geometry(field: &Field, view: &Ident) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => quote! {
                cursor = usize::try_from(#view::#source(target))
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
