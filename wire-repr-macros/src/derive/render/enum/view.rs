mod parsing;

use super::super::super::model::{Variant, VariantSelector};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) struct Names<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) view: &'a proc_macro2::Ident,
    pub(super) view_variant: &'a proc_macro2::Ident,
    pub(super) decode_error: &'a proc_macro2::Ident,
    pub(super) validation_error: &'a proc_macro2::Ident,
    pub(super) operation_parse: Option<&'a proc_macro2::Ident>,
}

pub(super) struct Types<'a> {
    pub(super) tag_codec: &'a TokenStream,
    pub(super) tag_type: &'a TokenStream,
    pub(super) view_error: &'a TokenStream,
    pub(super) validation_error: &'a TokenStream,
}

pub(super) struct Geometry<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) framing: Framing,
    pub(super) operation: Operation<'a>,
    pub(super) owned: bool,
}

pub(super) struct Schema<'a> {
    pub(super) variants: &'a [Variant],
    pub(super) body_view_paths: &'a [Option<TokenStream>],
    pub(super) unknown_variant: Option<&'a Variant>,
}

pub(super) struct Framing {
    pub(super) byte_tag_width: Option<usize>,
    pub(super) view_variant_has_lifetime: bool,
}

pub(super) struct Operation<'a> {
    pub(super) uses_input: bool,
    pub(super) operation_input: Option<&'a syn::Path>,
}

pub(super) struct Input<'a> {
    pub(super) names: Names<'a>,
    pub(super) types: Types<'a>,
    pub(super) geometry: Geometry<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let Input {
        names,
        types,
        geometry,
        runtime,
    } = input;
    let Names {
        vis,
        view,
        view_variant,
        validation_error,
        ..
    } = &names;
    let Types {
        tag_type,
        validation_error: validation_error_type,
        ..
    } = &types;
    let Geometry {
        schema,
        framing,
        owned,
        ..
    } = &geometry;
    let Schema {
        variants,
        body_view_paths,
        ..
    } = schema;
    let Framing {
        byte_tag_width,
        view_variant_has_lifetime,
    } = framing;

    let view_variant_decl_generics = view_variant_has_lifetime.then(|| quote!(<'__wire_repr_wire>));
    let view_variant_type = if *view_variant_has_lifetime {
        quote!(#view_variant<'__wire_repr_wire>)
    } else {
        quote!(#view_variant)
    };
    let view_variants = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        if matches!(variant.selector, VariantSelector::Unknown) {
            if byte_tag_width.is_some() {
                if *owned {
                    quote!(#variant_name(#tag_type),)
                } else {
                    quote!(#variant_name(&'__wire_repr_wire #tag_type),)
                }
            } else {
                quote!(#variant_name(#tag_type),)
            }
        } else {
            match &variant.body {
                Some(_) => {
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    if *owned {
                        quote!(#variant_name(<#body_view as #runtime::WireViewType>::View<'static>),)
                    } else {
                        quote!(#variant_name(<#body_view as #runtime::WireViewType>::View<'__wire_repr_wire>),)
                    }
                }
                None => quote!(#variant_name,),
            }
        }
    });
    let view_getters = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        let method = snake_case(variant_name);
        if matches!(variant.selector, VariantSelector::Unknown) {
            if *owned {
                return quote! {
                    #[doc = concat!("Returns the raw tag captured by `", stringify!(#variant_name), "`.")]
                    #[must_use]
                    #vis fn #method(&self) -> Option<#tag_type> {
                        match &self.variant {
                            #view_variant::#variant_name(tag) => Some(*tag),
                            _ => None,
                        }
                    }
                };
            }
            let return_type = if byte_tag_width.is_some() {
                quote!(&'__wire_repr_wire #tag_type)
            } else {
                quote!(#tag_type)
            };
            quote! {
                #[doc = concat!("Returns the raw tag captured by `", stringify!(#variant_name), "`.")]
                #[must_use]
                #vis fn #method(&self) -> Option<#return_type> {
                    match self.variant {
                        #view_variant::#variant_name(tag) => Some(tag),
                        _ => None,
                    }
                }
            }
        } else {
            match &variant.body {
                Some(_) => {
                    let body_view = body_view_paths[index]
                        .as_ref()
                        .expect("body variants have generated view paths");
                    if *owned {
                        return quote! {
                            #[doc = concat!("Returns the `", stringify!(#variant_name), "` body view when selected.")]
                            #[must_use]
                            #vis fn #method(&self) -> Option<<#body_view as #runtime::WireViewType>::View<'static>> {
                                match &self.variant {
                                    #view_variant::#variant_name(body) => Some(body.clone()),
                                    _ => None,
                                }
                            }
                        };
                    }
                    quote! {
                        #[doc = concat!("Returns the `", stringify!(#variant_name), "` body view when selected.")]
                        #[must_use]
                        #vis fn #method(&self) -> Option<<#body_view as #runtime::WireViewType>::View<'__wire_repr_wire>> {
                            match self.variant {
                                #view_variant::#variant_name(body) => Some(body),
                                _ => None,
                            }
                        }
                    }
                }
                None => {
                    let method = format_ident!("is_{}", method);
                    quote! {
                        #[doc = concat!("Returns whether the `", stringify!(#variant_name), "` variant is selected.")]
                        #[must_use]
                        #vis fn #method(&self) -> bool {
                            matches!(self.variant, #view_variant::#variant_name)
                        }
                    }
                }
            }
        }
    });
    let validation_arms = variants.iter().filter_map(|variant| {
        (!matches!(variant.selector, VariantSelector::Unknown))
            .then(|| {
                variant.body.as_ref().map(|_| {
                    let variant_name = &variant.name;
                    if *owned {
                        quote! {
                            #view_variant::#variant_name(body) => <_ as #runtime::WireViewValidation<'static>>::validate(body)
                                .map_err(#validation_error::#variant_name),
                        }
                    } else {
                        quote! {
                            #view_variant::#variant_name(body) => <_ as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&body)
                                .map_err(#validation_error::#variant_name),
                        }
                    }
                })
            })
            .flatten()
    });
    let parsing::Output {
        operation_view_helper,
        view_impl,
    } = parsing::render(parsing::Input {
        names: &names,
        types: &types,
        geometry: &geometry,
        runtime,
    });

    if *owned {
        quote! {
            #[derive(Clone, Debug)]
            enum #view_variant {
                #(#view_variants)*
            }

            /// A bytes-backed validated read view for this tagged wire enum.
            #[derive(Clone, Debug)]
            #vis struct #view {
                bytes: #runtime::__private::Bytes,
                variant: #view_variant,
            }

            impl #view {
                /// Returns this view's exact represented bytes.
                #[must_use]
                #vis fn as_bytes(&self) -> &[u8] {
                    &self.bytes
                }

                #(#view_getters)*
                #operation_view_helper
            }

            #view_impl

            impl #runtime::ByteSource for #view {
                #[inline(always)]
                fn byte_len(&self) -> usize {
                    self.as_bytes().len()
                }

                #[inline(always)]
                fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                    sink.write(self.as_bytes());
                }
            }

            impl #runtime::ByteSourceCursor for #view {
                type Segments<'source> = ::core::iter::Once<#runtime::ByteSegment<'source>>
                where
                    Self: 'source;

                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
                }

                type Bytes<'__wire_repr_source> = ::core::iter::Copied<::core::slice::Iter<'__wire_repr_source, u8>>
                where
                    Self: '__wire_repr_source;

                #[inline(always)]
                fn bytes(&self) -> Self::Bytes<'_> {
                    self.as_bytes().iter().copied()
                }
            }

            impl #runtime::WireViewValidation<'static> for #view {
                type ValidationError = #validation_error_type;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    match &self.variant {
                        #(#validation_arms)*
                        _ => Ok(()),
                    }
                }
            }
        }
    } else {
        quote! {
            #[derive(Clone, Copy, Debug)]
            enum #view_variant #view_variant_decl_generics {
                #(#view_variants)*
            }

            /// A bytes-backed validated read view for this tagged wire enum.
            #[derive(Clone, Copy, Debug)]
            #vis struct #view<'__wire_repr_wire> {
                bytes: &'__wire_repr_wire [u8],
                variant: #view_variant_type,
            }

            impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
                /// Returns this view's exact represented bytes.
                #[must_use]
                #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }

                #(#view_getters)*
                #operation_view_helper
            }

            #view_impl

            impl<'__wire_repr_wire> #runtime::ByteSource for #view<'__wire_repr_wire> {
                #[inline(always)]
                fn byte_len(&self) -> usize {
                    self.as_bytes().len()
                }

                #[inline(always)]
                fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                    sink.write(self.as_bytes());
                }
            }

            impl<'__wire_repr_wire> #runtime::ByteSourceCursor for #view<'__wire_repr_wire> {
                type Segments<'source> = ::core::iter::Once<#runtime::ByteSegment<'source>>
                where
                    Self: 'source;

                #[inline(always)]
                fn segments(&self) -> Self::Segments<'_> {
                    ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
                }

                type Bytes<'__wire_repr_source> = ::core::iter::Copied<::core::slice::Iter<'__wire_repr_source, u8>>
                where
                    Self: '__wire_repr_source;

                #[inline(always)]
                fn bytes(&self) -> Self::Bytes<'_> {
                    self.as_bytes().iter().copied()
                }
            }

            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire>
                for #view<'__wire_repr_wire>
            {
                type ValidationError = #validation_error_type;

                fn validate(&self) -> Result<(), Self::ValidationError> {
                    match self.variant {
                        #(#validation_arms)*
                        _ => Ok(()),
                    }
                }
            }
        }
    }
}

pub(super) fn snake_case(name: &proc_macro2::Ident) -> proc_macro2::Ident {
    let source = name.to_string();
    let mut value = String::new();
    for (index, character) in source.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                value.push('_');
            }
            value.extend(character.to_lowercase());
        } else {
            value.push(character);
        }
    }
    format_ident!("{value}")
}
