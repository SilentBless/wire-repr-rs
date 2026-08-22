//! Struct semantic validation rendering.

use super::super::super::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Path, Type, Visibility};

pub(super) struct Input<'a> {
    pub(super) vis: &'a Visibility,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) variants: &'a [Ident],
    pub(super) nested_view_paths: &'a [Option<TokenStream>],
    pub(super) model_validators: &'a [Path],
    pub(super) custom_validation_error: Option<&'a Type>,
    pub(super) has_nested: bool,
    pub(super) view: &'a Ident,
    pub(super) validation_error: &'a Ident,
    pub(super) view_error_type: &'a TokenStream,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) error_type: TokenStream,
    pub(super) declaration: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        vis,
        fields,
        labels,
        variants,
        nested_view_paths,
        model_validators,
        custom_validation_error,
        has_nested,
        view,
        validation_error,
        view_error_type,
        runtime,
    } = input;
    let generated_validation_error = custom_validation_error.is_none() && has_nested;
    let error_type = if let Some(error) = custom_validation_error {
        quote!(#error)
    } else if generated_validation_error {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#view_error_type)
    };
    let field_validators = fields.iter().enumerate().flat_map(|(index, field)| {
        let name = &field.name;
        let own = field
            .validators
            .iter()
            .map(move |validator| quote!(#validator(self.#name())?;));
        if matches!(field.kind, FieldKind::Nested) {
            let child_view = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            let variant = &variants[index];
            let child = if generated_validation_error {
                let nested_variant = format_ident!("Nested{variant}");
                quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(#validation_error::#nested_variant)?;)
            } else {
                quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(<Self::ValidationError as From<_>>::from)?;)
            };
            quote!(#child #(#own)*)
        } else {
            quote!(#(#own)*)
        }
    });
    let validation_impl = quote! {
        impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type ValidationError = #error_type;
            fn validate(&self) -> Result<(), Self::ValidationError> {
                #(#field_validators)*
                #(#model_validators(self)?;)*
                Ok(())
            }
        }
    };
    let generated_validation_error_decl = generated_validation_error.then(|| {
        let nested_variants = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let child_view = nested_view_paths[index]
                    .as_ref()
                    .expect("nested fields have generated view paths");
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(
                    #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                    #variant(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError),
                )
            });
        let nested_arms = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(Self::#variant(error) => write!(formatter, "nested validation failed in field `{}`: {error}", #label),)
            });
        quote! {
            /// Semantic validation failures for this wire representation.
            #[derive(Debug)]
            #vis enum #validation_error<'__wire_repr_wire> {
                /// Structural framing failed.
                Decode(#view_error_type),
                #(#nested_variants)*
            }
            impl ::core::fmt::Display for #validation_error<'_> {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::Decode(error) => error.fmt(formatter),
                        #(#nested_arms)*
                    }
                }
            }
            impl ::core::error::Error for #validation_error<'_> {}
            impl<'__wire_repr_wire> From<#view_error_type> for #validation_error<'__wire_repr_wire> {
                fn from(error: #view_error_type) -> Self { Self::Decode(error) }
            }
        }
    });

    Output {
        error_type,
        declaration: quote! {
            #generated_validation_error_decl
            #validation_impl
        },
    }
}
