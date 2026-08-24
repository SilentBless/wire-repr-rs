//! Struct semantic validation rendering.

use super::super::super::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Path, Type, Visibility};

use super::Validator;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) policy: Policy<'a>,
    pub(super) types: Types<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) vis: &'a Visibility,
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) variants: &'a [Ident],
    pub(super) nested_view_paths: &'a [Option<TokenStream>],
}

pub(super) struct Policy<'a> {
    pub(super) model_validators: &'a [Path],
    pub(super) custom_validation_error: Option<&'a Type>,
    pub(super) has_nested: bool,
    pub(super) inferred: &'a [Validator],
}

pub(super) struct Types<'a> {
    pub(super) view: &'a Ident,
    pub(super) validation_error: &'a Ident,
    pub(super) aggregate_error: &'a Ident,
    pub(super) view_error_type: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) error_type: TokenStream,
    pub(super) declaration: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    if cfg!(feature = "bytes") {
        return render_owned(input);
    }
    let Input {
        schema:
            Schema {
                vis,
                fields,
                labels,
                variants,
                nested_view_paths,
            },
        policy:
            Policy {
                model_validators,
                custom_validation_error,
                has_nested,
                inferred,
            },
        types:
            Types {
                view,
                validation_error,
                aggregate_error,
                view_error_type,
            },
        runtime,
    } = input;
    if custom_validation_error.is_none() && has_nested {
        return render_borrowed_nested(
            vis,
            fields,
            labels,
            variants,
            nested_view_paths,
            model_validators,
            inferred,
            view,
            validation_error,
            view_error_type,
            runtime,
        );
    }
    let generated_validation_error =
        custom_validation_error.is_none() && (has_nested || !inferred.is_empty());
    let validation_error_type = if has_nested {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#validation_error)
    };
    let aggregate_error_type = if has_nested {
        quote!(#aggregate_error<'__wire_repr_wire>)
    } else {
        quote!(#aggregate_error)
    };
    let error_type = if let Some(error) = custom_validation_error {
        quote!(#error)
    } else if generated_validation_error {
        aggregate_error_type.clone()
    } else {
        quote!(#view_error_type)
    };
    let inferred_validators = inferred;
    let mut inferred = inferred_validators.iter();
    let mut field_validators = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let name = &field.name;
        if matches!(field.kind, FieldKind::Nested) {
            let child_view = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            let variant = &variants[index];
            let child = if generated_validation_error {
                let nested_variant = format_ident!("Nested{variant}");
                quote! {
                    <<#child_view as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(
                        &self.#name()
                    )
                    .map_err(#validation_error::#nested_variant)?;
                }
            } else {
                quote! {
                    <<#child_view as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(
                        &self.#name()
                    )
                    .map_err(<Self::ValidationError as From<_>>::from)?;
                }
            };
            field_validators.push(child);
        }
        for validator in &field.validators {
            if generated_validation_error {
                let metadata = inferred.next().expect("inferred field validator metadata");
                let callback = &metadata.callback;
                let variant = &metadata.variant;
                field_validators.push(quote! {
                    #callback(self.#name()).map_err(#validation_error::#variant)?;
                });
            } else {
                field_validators.push(quote!(#validator(self.#name())?;));
            }
        }
    }
    let mut model_validation = Vec::new();
    for validator in model_validators {
        if generated_validation_error {
            let metadata = inferred.next().expect("inferred model validator metadata");
            let callback = &metadata.callback;
            let variant = &metadata.variant;
            model_validation.push(quote! {
                #callback(self).map_err(#validation_error::#variant)?;
            });
        } else {
            model_validation.push(quote!(#validator(self)?;));
        }
    }
    debug_assert!(inferred.next().is_none());
    let validation_impl = quote! {
        impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
            type ValidationError = #error_type;
            fn validate(&self) -> Result<(), Self::ValidationError> {
                #(#field_validators)*
                #(#model_validation)*
                Ok(())
            }
        }
    };
    let generated_validation_error_decl = generated_validation_error.then(|| {
        let error_assertions = super::validator_error_assertions(inferred_validators);
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
                quote! {
                    #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                    #[error("nested validation failed in field `{field}`: {0}", field = #label)]
                    #variant(
                        #[source]
                        <<#child_view as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError,
                    ),
                }
            });
        let validator_variants = validator_variants(inferred_validators);
        let decl_generics = has_nested.then(|| quote!(<'__wire_repr_wire>));
        quote! {
            #error_assertions

            /// Semantic validation failures for this wire representation.
            #[allow(missing_docs)]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #validation_error #decl_generics {
                #(#nested_variants)*
                #validator_variants
            }

            /// Typed read failures for this wire representation.
            #[allow(missing_docs)]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #aggregate_error #decl_generics {
                #[error(transparent)]
                Decode(#[from] #view_error_type),
                #[error(transparent)]
                Validate(#[from] #validation_error_type),
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

#[allow(clippy::too_many_arguments)]
fn render_borrowed_nested(
    vis: &Visibility,
    fields: &[Field],
    labels: &[String],
    variants: &[Ident],
    nested_view_paths: &[Option<TokenStream>],
    model_validators: &[Path],
    inferred_validators: &[Validator],
    view: &Ident,
    validation_error: &Ident,
    view_error_type: &TokenStream,
    runtime: &TokenStream,
) -> Output {
    let mut inferred = inferred_validators.iter();
    let mut validation_steps = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let name = &field.name;
        if matches!(field.kind, FieldKind::Nested) {
            let child = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            let variant = format_ident!("Nested{}", variants[index]);
            validation_steps.push(quote! {
                <<#child as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name())
                    .map_err(#validation_error::#variant)?;
            });
        }
        for _ in &field.validators {
            let metadata = inferred.next().expect("inferred field validator metadata");
            let callback = &metadata.callback;
            let variant = &metadata.variant;
            validation_steps.push(quote! {
                #callback(self.#name()).map_err(#validation_error::#variant)?;
            });
        }
    }
    for _ in model_validators {
        let metadata = inferred.next().expect("inferred model validator metadata");
        let callback = &metadata.callback;
        let variant = &metadata.variant;
        validation_steps.push(quote! {
            #callback(self).map_err(#validation_error::#variant)?;
        });
    }
    debug_assert!(inferred.next().is_none());

    let nested_variants = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
        .map(|(index, _)| {
            let child = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            let variant = format_ident!("Nested{}", variants[index]);
            let label = &labels[index];
            quote! {
                #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                #variant(
                    <<#child as #runtime::WireViewType>::View<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError,
                ),
            }
        });
    let nested_arms = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
        .map(|(index, _)| {
            let variant = format_ident!("Nested{}", variants[index]);
            let label = &labels[index];
            quote! {
                Self::#variant(error) => write!(
                    formatter,
                    "nested validation failed in field `{}`: {error}",
                    #label,
                ),
            }
        });
    let validator_variants = inferred_validators.iter().map(|validator| {
        let variant = &validator.variant;
        let error = &validator.error;
        quote!(#variant(#error),)
    });
    let validator_arms = inferred_validators.iter().map(|validator| {
        let variant = &validator.variant;
        let callback = &validator.label;
        if let Some(field) = &validator.field {
            let field = field.to_string();
            quote! {
                Self::#variant(error) => write!(
                    formatter,
                    "validator `{}` rejected field `{}`: {error}",
                    #callback,
                    #field,
                ),
            }
        } else {
            quote! {
                Self::#variant(error) => write!(
                    formatter,
                    "validator `{}` rejected the model: {error}",
                    #callback,
                ),
            }
        }
    });
    let error_assertions = super::validator_error_assertions(inferred_validators);

    Output {
        error_type: quote!(#validation_error<'__wire_repr_wire>),
        declaration: quote! {
            #error_assertions

            /// Semantic validation failures for this wire representation.
            #[allow(missing_docs)]
            #[derive(Debug)]
            #vis enum #validation_error<'__wire_repr_wire> {
                Decode(#view_error_type),
                #(#nested_variants)*
                #(#validator_variants)*
            }
            impl ::core::fmt::Display for #validation_error<'_> {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::Decode(error) => error.fmt(formatter),
                        #(#nested_arms)*
                        #(#validator_arms)*
                    }
                }
            }
            impl ::core::error::Error for #validation_error<'_> {}
            impl<'__wire_repr_wire> From<#view_error_type> for #validation_error<'__wire_repr_wire> {
                fn from(error: #view_error_type) -> Self {
                    Self::Decode(error)
                }
            }
            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type ValidationError = #validation_error<'__wire_repr_wire>;
                fn validate(&self) -> Result<(), Self::ValidationError> {
                    #(#validation_steps)*
                    Ok(())
                }
            }
        },
    }
}

fn render_owned(input: Input<'_>) -> Output {
    let Input {
        schema:
            Schema {
                vis,
                fields,
                labels,
                variants,
                nested_view_paths,
            },
        policy:
            Policy {
                model_validators,
                custom_validation_error,
                has_nested,
                inferred,
            },
        types:
            Types {
                view,
                validation_error,
                aggregate_error,
                view_error_type,
            },
        runtime,
    } = input;
    let generated = custom_validation_error.is_none() && (has_nested || !inferred.is_empty());
    let error_type = if let Some(error) = custom_validation_error {
        quote!(#error)
    } else if generated {
        quote!(#aggregate_error)
    } else {
        quote!(#view_error_type)
    };
    let inferred_validators = inferred;
    let mut inferred = inferred_validators.iter();
    let mut field_validators = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let name = &field.name;
        if matches!(field.kind, FieldKind::Nested) {
            let child = nested_view_paths[index]
                .as_ref()
                .expect("nested fields have generated view paths");
            let nested = if generated {
                let variant = format_ident!("Nested{}", variants[index]);
                quote! {
                    <<#child as #runtime::WireViewType>::View<'static> as #runtime::WireViewValidation<'static>>::validate(&self.#name())
                        .map_err(#validation_error::#variant)?;
                }
            } else {
                quote! {
                    <<#child as #runtime::WireViewType>::View<'static> as #runtime::WireViewValidation<'static>>::validate(&self.#name())
                        .map_err(<Self::ValidationError as From<_>>::from)?;
                }
            };
            field_validators.push(nested);
        }
        for validator in &field.validators {
            if generated {
                let metadata = inferred.next().expect("inferred field validator metadata");
                let callback = &metadata.callback;
                let variant = &metadata.variant;
                field_validators.push(quote! {
                    #callback(self.#name()).map_err(#validation_error::#variant)?;
                });
            } else {
                field_validators.push(quote!(#validator(self.#name())?;));
            }
        }
    }
    let mut model_validation = Vec::new();
    for validator in model_validators {
        if generated {
            let metadata = inferred.next().expect("inferred model validator metadata");
            let callback = &metadata.callback;
            let variant = &metadata.variant;
            model_validation.push(quote! {
                #callback(self).map_err(#validation_error::#variant)?;
            });
        } else {
            model_validation.push(quote!(#validator(self)?;));
        }
    }
    debug_assert!(inferred.next().is_none());
    let validation_impl = quote! {
        impl #runtime::WireViewValidation<'static> for #view {
            type ValidationError = #error_type;
            fn validate(&self) -> Result<(), Self::ValidationError> {
                #(#field_validators)*
                #(#model_validation)*
                Ok(())
            }
        }
    };
    let error_decl = generated.then(|| {
        let error_assertions = super::validator_error_assertions(inferred_validators);
        let nested_variants = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let child = nested_view_paths[index]
                    .as_ref()
                    .expect("nested fields have generated view paths");
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote! {
                    #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                    #[error("nested validation failed in field `{field}`: {0}", field = #label)]
                    #variant(
                        #[source]
                        <<#child as #runtime::WireViewType>::View<'static> as #runtime::WireViewValidation<'static>>::ValidationError,
                    ),
                }
            });
        let validator_variants = validator_variants(inferred_validators);
        quote! {
            #error_assertions

            /// Semantic validation failures for this wire representation.
            #[allow(missing_docs)]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #validation_error {
                #(#nested_variants)*
                #validator_variants
            }

            /// Typed read failures for this wire representation.
            #[allow(missing_docs)]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #aggregate_error {
                #[error(transparent)]
                Decode(#[from] #view_error_type),
                #[error(transparent)]
                Validate(#[from] #validation_error),
            }
        }
    });
    Output {
        error_type,
        declaration: quote!(#error_decl #validation_impl),
    }
}

fn validator_variants(validators: &[Validator]) -> TokenStream {
    let variants = validators.iter().map(|validator| {
        let variant = &validator.variant;
        let error = &validator.error;
        let callback = &validator.label;
        if let Some(field) = &validator.field {
            let field = field.to_string();
            quote! {
                #[error(
                    "validator `{callback}` rejected field `{field}`: {0}",
                    callback = #callback,
                    field = #field,
                )]
                #variant(#[source] #error),
            }
        } else {
            quote! {
                #[error(
                    "validator `{callback}` rejected the model: {0}",
                    callback = #callback,
                )]
                #variant(#[source] #error),
            }
        }
    });
    quote!(#(#variants)*)
}
