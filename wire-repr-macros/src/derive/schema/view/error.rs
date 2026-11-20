use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::super::model::{ArrayField, FieldKind, Schema};
use super::super::{pascal, value_type_tokens};

/// Generic parameters of the generated error enum and the type this schema instantiates it with.
pub(super) struct Signature {
    pub(super) parameters: Vec<syn::Ident>,
    pub(super) ty: TokenStream,
}

pub(super) fn signature(schema: &Schema, error: &syn::Ident, runtime: &TokenStream) -> Signature {
    let error_fields = schema
        .fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_)))
        .collect::<Vec<_>>();
    let error_parameters = (0..error_fields.len())
        .map(|index| format_ident!("__WireReprError{index}"))
        .collect::<Vec<_>>();
    let error_types = error_fields.iter().map(|field| match &field.kind {
        FieldKind::Nested(nested) => {
            let ty = &nested.ty;
            quote!(<#ty as #runtime::WireView>::Error)
        }
        FieldKind::Array(array) => {
            let item = &array.item;
            quote!(#runtime::ArrayError<<#item as #runtime::WireView>::Error>)
        }
        _ => unreachable!("error fields are nested schemas or arrays"),
    });
    let error_type = if error_fields.is_empty() {
        quote!(#error)
    } else {
        quote!(#error<#(#error_types),*>)
    };
    Signature {
        parameters: error_parameters,
        ty: error_type,
    }
}

pub(super) struct VariantNames {
    pub(super) fields: Vec<FieldVariantNames>,
    pub(super) validators: Vec<syn::Ident>,
}

pub(super) struct FieldVariantNames {
    pub(super) shortage: syn::Ident,
    pub(super) mismatch: Option<syn::Ident>,
    pub(super) conversion: Option<syn::Ident>,
    pub(super) nested: Option<syn::Ident>,
    pub(super) extent: Option<syn::Ident>,
}

pub(super) fn variant_names(schema: &Schema) -> VariantNames {
    let mut used = BTreeSet::from([
        "InvalidFrame".to_owned(),
        "LayoutUnavailable".to_owned(),
        "PositionBeforeCursor".to_owned(),
        "Trailing".to_owned(),
    ]);
    let fields = schema
        .fields
        .iter()
        .map(|field| {
            let base = pascal(&field.name).to_string();
            let mismatch = field
                .kind
                .constant()
                .map(|_| unique_variant(&mut used, &format!("{base}Constant")));
            let conversion = match &field.kind {
                FieldKind::Scalar(scalar)
                    if scalar.value_type.is_converted()
                        || (schema.is_count_controller(&field.name)
                            && scalar.wire_type.is_signed_integer()) =>
                {
                    Some(unique_variant(&mut used, &format!("{base}Value")))
                }
                FieldKind::Scalar(_)
                | FieldKind::Bytes(_)
                | FieldKind::ScalarArray(_)
                | FieldKind::RawBytes(_)
                | FieldKind::Array(_)
                | FieldKind::Recursive(_)
                | FieldKind::Flag(_)
                | FieldKind::BitProjection(_)
                | FieldKind::Nested(_) => None,
            };
            let nested = matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_))
                .then(|| unique_variant(&mut used, &base));
            let extent = matches!(field.kind, FieldKind::Nested(_))
                .then(|| unique_variant(&mut used, &format!("{base}Extent")));
            let decorated_shortage = mismatch.is_some() || conversion.is_some() || nested.is_some();
            let shortage_base = match &field.kind {
                FieldKind::Scalar(scalar) if scalar.is_variable() => {
                    format!("{base}{}", super::super::integer::error_suffix(scalar))
                }
                _ if decorated_shortage => format!("{base}NeedMore"),
                _ => base.clone(),
            };
            let shortage = unique_variant(&mut used, &shortage_base);
            FieldVariantNames {
                shortage,
                mismatch,
                conversion,
                nested,
                extent,
            }
        })
        .collect();
    let validators = schema
        .validators
        .iter()
        .map(|path| {
            let name = path.segments.last().expect("validator path has a segment");
            unique_variant(&mut used, &pascal(&name.ident).to_string())
        })
        .collect();
    VariantNames { fields, validators }
}

fn unique_variant(used: &mut BTreeSet<String>, base: &str) -> syn::Ident {
    if used.insert(base.to_owned()) {
        return format_ident!("{base}");
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}{suffix}");
        if used.insert(candidate.clone()) {
            return format_ident!("{candidate}");
        }
    }
    unreachable!("usize suffix space cannot be exhausted by generated variants")
}

pub(super) fn render_flatten(
    schema: &Schema,
    error: &syn::Ident,
    names: &VariantNames,
    runtime: &TokenStream,
) -> TokenStream {
    let mut arms = Vec::new();
    for (field, names) in schema.fields.iter().zip(&names.fields) {
        let shortage = &names.shortage;
        if let FieldKind::Scalar(scalar) = &field.kind
            && scalar.is_variable()
        {
            let flatten = super::super::integer::flatten_read_error(scalar, runtime);
            arms.push(quote! {
                #error::#shortage(source) => #flatten
            });
        } else {
            arms.push(quote! {
                #error::#shortage(source) => {
                    #runtime::__private::RecursiveError::NeedMore(source)
                }
            });
        }
        if let Some(mismatch) = &names.mismatch {
            arms.push(quote! {
                #error::#mismatch(source) => {
                    #runtime::__private::RecursiveError::Child {
                        offset: source.offset,
                    }
                }
            });
        }
        if let Some(conversion) = &names.conversion {
            arms.push(quote! {
                #error::#conversion(source) => {
                    #runtime::__private::RecursiveError::Child {
                        offset: source.offset,
                    }
                }
            });
        }
        if let Some(nested) = &names.nested {
            match &field.kind {
                FieldKind::Nested(nested_field) => {
                    let ty = &nested_field.ty;
                    arms.push(quote! {
                        #error::#nested(source) => {
                            <#ty as #runtime::WireView>::flatten_recursive_error(
                                source,
                                fallback_offset,
                            )
                        }
                    });
                }
                FieldKind::Array(array) => {
                    arms.push(flatten_array_arm(error, nested, array, runtime));
                }
                FieldKind::Scalar(_)
                | FieldKind::Bytes(_)
                | FieldKind::ScalarArray(_)
                | FieldKind::RawBytes(_)
                | FieldKind::Recursive(_)
                | FieldKind::Flag(_)
                | FieldKind::BitProjection(_) => unreachable!("nested error owner"),
            }
        }
        if let Some(extent) = &names.extent {
            arms.push(quote! {
                #error::#extent(source) => {
                    #runtime::__private::RecursiveError::InvalidFrame(source)
                }
            });
        }
    }
    for variant in &names.validators {
        arms.push(quote! {
            #error::#variant(_) => {
                #runtime::__private::RecursiveError::Child {
                    offset: fallback_offset,
                }
            }
        });
    }
    arms.extend([
        quote! {
            #error::PositionBeforeCursor { field, .. }
            | #error::LayoutUnavailable { field } => {
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field },
                )
            }
        },
        quote! {
            #error::InvalidFrame(source) => {
                #runtime::__private::RecursiveError::InvalidFrame(source)
            }
        },
        quote! {
            #error::Trailing(source) => {
                #runtime::__private::RecursiveError::Trailing(source)
            }
        },
    ]);
    quote! {
        #[inline]
        fn flatten_recursive_error(
            error: Self::Error,
            fallback_offset: usize,
        ) -> #runtime::__private::RecursiveError {
            match error {
                #(#arms),*
            }
        }
    }
}

fn flatten_array_arm(
    error: &syn::Ident,
    nested: &syn::Ident,
    array: &ArrayField,
    runtime: &TokenStream,
) -> TokenStream {
    let item = &array.item;
    quote! {
        #error::#nested(source) => {
            match source {
                #runtime::ArrayError::NeedMore(source) => {
                    #runtime::__private::RecursiveError::NeedMore(source)
                }
                #runtime::ArrayError::Item { source, .. } => {
                    <#item as #runtime::WireView>::flatten_recursive_error(
                        source,
                        fallback_offset,
                    )
                }
                #runtime::ArrayError::InvalidExtent {
                    consumed,
                    available,
                    ..
                } => {
                    #runtime::__private::RecursiveError::InvalidFrame(
                        #runtime::InvalidFrameExtent {
                            offset: fallback_offset,
                            consumed,
                            available,
                        },
                    )
                }
                #runtime::ArrayError::NonProgress { offset, .. } => {
                    #runtime::__private::RecursiveError::Child { offset }
                }
                #runtime::ArrayError::Trailing { offset, trailing } => {
                    #runtime::__private::RecursiveError::Trailing(
                        #runtime::TrailingBytes { offset, trailing },
                    )
                }
            }
        }
    }
}

pub(super) fn render(
    schema: &Schema,
    error: &syn::Ident,
    names: &VariantNames,
    error_parameters: &[syn::Ident],
    runtime: &TokenStream,
) -> TokenStream {
    let mut variants = Vec::new();
    let mut flatten_arms = Vec::new();
    let mut nested_index = 0usize;
    for (field, names) in schema.fields.iter().zip(&names.fields) {
        let field_name = field.name.to_string();
        let shortage = &names.shortage;
        let shortage_message = format!("failed to frame field `{field_name}`: {{0}}");
        if let FieldKind::Scalar(scalar) = &field.kind
            && scalar.is_variable()
        {
            variants.push(super::super::integer::read_error_variant(
                scalar,
                shortage,
                &shortage_message,
                runtime,
            ));
            let flatten = super::super::integer::flatten_read_error(scalar, runtime);
            flatten_arms.push(quote! {
                Self::#shortage(source) => #flatten
            });
        } else {
            variants.push(quote! {
                #[doc = "The field ended before its complete representation."]
                #[error(#shortage_message)]
                #shortage(#[source] #runtime::NeedMore),
            });
            flatten_arms.push(quote! {
                Self::#shortage(source) => {
                    #runtime::__private::RecursiveError::NeedMore(source)
                }
            });
        }
        if let Some(mismatch) = &names.mismatch {
            let mismatch_message = format!("invalid constant field `{field_name}`: {{0}}");
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let ty = value_type_tokens(&scalar.value_type);
                    variants.push(quote! {
                        #[doc = "The stored constant did not match the schema value."]
                        #[error(#mismatch_message)]
                        #mismatch(#[source] #runtime::ConstantMismatch<#ty>),
                    });
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    variants.push(quote! {
                        #[doc = "The stored constant did not match the schema value."]
                        #[error(#mismatch_message)]
                        #mismatch(#[source] #runtime::ConstantMismatch<#ty>),
                    });
                }
                FieldKind::RawBytes(_)
                | FieldKind::ScalarArray(_)
                | FieldKind::Array(_)
                | FieldKind::Recursive(_)
                | FieldKind::Flag(_)
                | FieldKind::BitProjection(_)
                | FieldKind::Nested(_) => unreachable!(),
            }
            flatten_arms.push(quote! {
                Self::#mismatch(source) => {
                    #runtime::__private::RecursiveError::Child {
                        offset: source.offset,
                    }
                }
            });
        }
        if let Some(conversion) = &names.conversion {
            let message = format!("invalid physical value for field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "The physical scalar cannot be represented by the Rust field type."]
                #[error(#message)]
                #conversion(#[source] #runtime::ScalarConversionError),
            });
            flatten_arms.push(quote! {
                Self::#conversion(source) => {
                    #runtime::__private::RecursiveError::Child {
                        offset: source.offset,
                    }
                }
            });
        }
        if let Some(nested) = &names.nested {
            let parameter = &error_parameters[nested_index];
            nested_index += 1;
            let message = format!("failed to frame nested field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "A nested schema or counted array failed structural framing."]
                #[error(#message)]
                #nested(#[source] #parameter),
            });
            flatten_arms.push(quote! {
                Self::#nested(source) => source.flatten_recursive(fallback_offset)
            });
        }
        if let Some(extent) = &names.extent {
            let message = format!("invalid extent for nested field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "The nested schema reported an extent outside its assigned span."]
                #[error(#message)]
                #extent(#[source] #runtime::InvalidFrameExtent),
            });
            flatten_arms.push(quote! {
                Self::#extent(source) => {
                    #runtime::__private::RecursiveError::InvalidFrame(source)
                }
            });
        }
    }
    for (validator, variant) in schema.validators.iter().zip(&names.validators) {
        let validator_name = quote!(#validator).to_string();
        let validator_error =
            crate::validator::error_type(validator).expect("validated validator path");
        let message = format!("schema validator `{validator_name}` failed: {{0}}");
        variants.push(quote! {
            #[doc = "A schema-level validator rejected the framed view."]
            #[error(#message)]
            #variant(#[source] #validator_error),
        });
        flatten_arms.push(quote! {
            Self::#variant(_) => {
                #runtime::__private::RecursiveError::Child {
                    offset: fallback_offset,
                }
            }
        });
    }
    let declaration = render_enum(schema, error, error_parameters, variants, runtime);
    let flatten = render_flatten_impl(error, error_parameters, flatten_arms, runtime);
    quote!(#declaration #flatten)
}

fn render_enum(
    schema: &Schema,
    error: &syn::Ident,
    error_parameters: &[syn::Ident],
    mut variants: Vec<TokenStream>,
    runtime: &TokenStream,
) -> TokenStream {
    variants.push(quote! {
        #[doc = "A requested field position precedes the current physical cursor."]
        #[error("field `{field}` requests position {position}, before cursor {cursor}")]
        PositionBeforeCursor {
            field: &'static str,
            position: usize,
            cursor: usize,
        },
    });
    variants.push(quote! {
        #[doc = "A field offset or fixed width could not be established."]
        #[error("fixed layout is unavailable or overflows before field `{field}`")]
        LayoutUnavailable { field: &'static str },
    });
    variants.push(quote! {
        #[doc = "A child reported an extent outside its supplied input."]
        #[error("invalid child frame: {0}")]
        InvalidFrame(#[source] #runtime::InvalidFrameExtent),
    });
    variants.push(quote! {
        #[doc = "Exact framing found trailing input."]
        #[error("exact framing failed: {0}")]
        Trailing(#[source] #runtime::TrailingBytes),
    });
    let vis = &schema.vis;
    if error_parameters.is_empty() {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error {
                #(#variants)*
            }
        }
    } else {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<#(#error_parameters: ::core::error::Error + 'static),*> {
                #(#variants)*
            }
        }
    }
}

fn render_flatten_impl(
    error: &syn::Ident,
    error_parameters: &[syn::Ident],
    mut flatten_arms: Vec<TokenStream>,
    runtime: &TokenStream,
) -> TokenStream {
    flatten_arms.extend([
        quote! {
            Self::PositionBeforeCursor { field, .. }
            | Self::LayoutUnavailable { field } => {
                #runtime::__private::RecursiveError::Layout(
                    #runtime::LayoutError { field },
                )
            }
        },
        quote! {
            Self::InvalidFrame(source) => {
                #runtime::__private::RecursiveError::InvalidFrame(source)
            }
        },
        quote! {
            Self::Trailing(source) => {
                #runtime::__private::RecursiveError::Trailing(source)
            }
        },
    ]);
    if error_parameters.is_empty() {
        quote! {
            impl #runtime::__private::FlattenRecursiveError for #error {
                fn flatten_recursive(
                    self,
                    fallback_offset: usize,
                ) -> #runtime::__private::RecursiveError {
                    match self {
                        #(#flatten_arms),*
                    }
                }
            }
        }
    } else {
        quote! {
            impl<#(#error_parameters),*> #runtime::__private::FlattenRecursiveError
                for #error<#(#error_parameters),*>
            where
                #(
                    #error_parameters:
                        ::core::error::Error
                        + #runtime::__private::FlattenRecursiveError
                        + 'static,
                )*
            {
                fn flatten_recursive(
                    self,
                    fallback_offset: usize,
                ) -> #runtime::__private::RecursiveError {
                    match self {
                        #(#flatten_arms),*
                    }
                }
            }
        }
    }
}
