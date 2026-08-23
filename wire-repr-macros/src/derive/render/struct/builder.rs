//! Generated struct builder rendering.

use super::super::super::model::{Field, FieldKind};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) struct Input<'a> {
    pub(super) has_builder: bool,
    pub(super) identity: Identity<'a>,
    pub(super) types: Types<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) preparation: Preparation<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Identity<'a> {
    pub(super) name: &'a Ident,
    pub(super) vis: &'a syn::Visibility,
}

pub(super) struct Types<'a> {
    pub(super) wire_lifetime: Option<&'a syn::Lifetime>,
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
    pub(super) self_type: &'a TokenStream,
}

pub(super) struct Operation<'a> {
    pub(super) operation_input_ty: Option<&'a syn::Path>,
    pub(super) operation_name: Option<&'a Ident>,
}

pub(super) struct Preparation<'a> {
    pub(super) prepare_fields: &'a [&'a Field],
    pub(super) prepare_field_names: &'a [&'a Ident],
    pub(super) prepare_helper: &'a Ident,
}

pub(super) struct Output {
    pub(super) declaration: TokenStream,
    pub(super) method: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        has_builder,
        identity: Identity { name, vis },
        types:
            Types {
                wire_lifetime,
                plan,
                encode_error,
                self_type,
            },
        operation: Operation {
            operation_input_ty,
            operation_name,
        },
        preparation:
            Preparation {
                prepare_fields,
                prepare_field_names,
                prepare_helper,
            },
        runtime,
    } = input;
    let declaration = has_builder.then(|| {
        let builder_name = format_ident!("{name}Builder");
        let (builder_decl_generics, builder_type, builder_plan_type, builder_error_type) =
            if let Some(lifetime) = wire_lifetime {
                (
                    quote!(<#lifetime>),
                    quote!(#builder_name<#lifetime>),
                    quote!(#plan<#lifetime, #lifetime>),
                    quote!(#encode_error<#lifetime>),
                )
            } else {
                (
                    quote!(),
                    quote!(#builder_name),
                    quote!(#plan<'static>),
                    quote!(#encode_error),
                )
            };
        let builder_fields = prepare_fields.iter().map(|field| {
            let field_name = &field.name;
            let ty = match (&field.kind, wire_lifetime) {
                (FieldKind::Bytes { .. } | FieldKind::Rest, Some(lifetime)) => {
                    quote!(&#lifetime [u8])
                }
                _ => {
                    let ty = &field.ty;
                    quote!(#ty)
                }
            };
            quote!(#field_name: ::core::option::Option<#ty>)
        });
        let setters = prepare_fields.iter().map(|field| {
            let field_name = &field.name;
            let ty = match (&field.kind, wire_lifetime) {
                (FieldKind::Bytes { .. } | FieldKind::Rest, Some(lifetime)) => {
                    quote!(&#lifetime [u8])
                }
                _ => {
                    let ty = &field.ty;
                    quote!(#ty)
                }
            };
            quote! {
                #[doc = concat!("Sets the `", stringify!(#field_name), "` field.")]
                #vis fn #field_name(mut self, value: #ty) -> Self {
                    self.#field_name = Some(value);
                    self
                }
            }
        });
        let missing = prepare_fields.iter().map(|field| {
            let field_name = &field.name;
            let label = field_name.to_string();
            quote!(let #field_name = builder.#field_name.ok_or(#encode_error::MissingField { field: #label })?;)
        });
        let build_into = if cfg!(feature = "bytes") {
            quote! {
                /// Prepares and atomically writes this encoding into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut #runtime::__private::BytesMut,
                ) -> Result<
                    #runtime::Written<'__wire_repr_output>,
                    #runtime::BuildIntoError<#builder_error_type>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        } else {
            quote! {
                /// Prepares and atomically writes this encoding into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<
                    (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                    #runtime::BuildIntoError<#builder_error_type>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        };
        if let (Some(operation_input_ty), Some(operation_name)) = (operation_input_ty, operation_name)
        {
            let request_name = format_ident!("{name}Builder{}Request", operation_name);
            let request_decl_generics = if let Some(lifetime) = wire_lifetime {
                quote!(<#lifetime, '__wire_repr_operation>)
            } else {
                quote!(<'__wire_repr_operation>)
            };
            let request_type = if let Some(lifetime) = wire_lifetime {
                quote!(#request_name<#lifetime, '_>)
            } else {
                quote!(#request_name<'_>)
            };
            quote! {
                /// A no-allocation builder for this computed wire representation.
                #vis struct #builder_name #builder_decl_generics {
                    #(#builder_fields,)*
                }
                #[doc(hidden)]
                #vis struct #request_name #request_decl_generics {
                    builder: #builder_type,
                    #operation_name: &'__wire_repr_operation #operation_input_ty,
                }
                impl #builder_decl_generics #builder_type {
                    #(#setters)*
                    /// Binds the schema-named input for encoding.
                    #[must_use]
                    #vis fn #operation_name(self, #operation_name: &#operation_input_ty) -> #request_type {
                        #request_name { builder: self, #operation_name }
                    }
                }
                #[allow(missing_docs)]
                impl #request_decl_generics #request_name #request_decl_generics {
                    /// Prepares this encoding after checking all caller-owned fields.
                    #vis fn prepare(self) -> Result<#builder_plan_type, #builder_error_type> {
                        let Self { builder, #operation_name } = self;
                        #(#missing)*
                        <#self_type>::#prepare_helper(#(#prepare_field_names,)* #operation_name)
                    }
                    #build_into
                }
            }
        } else {
            quote! {
                /// A no-allocation builder for this computed wire representation.
                #vis struct #builder_name #builder_decl_generics {
                    #(#builder_fields,)*
                }
                impl #builder_decl_generics #builder_type {
                    #(#setters)*
                    /// Prepares this encoding after checking all caller-owned fields.
                    #vis fn prepare(self) -> Result<#builder_plan_type, #builder_error_type> {
                        let builder = self;
                        #(#missing)*
                        <#self_type>::#prepare_helper(#(#prepare_field_names),*)
                    }
                    #build_into
                }
            }
        }
    });

    let method = has_builder.then(|| {
        let builder_name = format_ident!("{name}Builder");
        let builder_type = if let Some(lifetime) = wire_lifetime {
            quote!(#builder_name<#lifetime>)
        } else {
            quote!(#builder_name)
        };
        let builder_empty_fields = prepare_fields.iter().map(|field| {
            let name = &field.name;
            quote!(#name: ::core::option::Option::None)
        });
        quote! {
            /// Starts a no-allocation builder for this computed representation.
            #vis fn builder() -> #builder_type {
                #builder_name { #(#builder_empty_fields,)* }
            }
        }
    });

    Output {
        declaration: declaration.unwrap_or_default(),
        method: method.unwrap_or_default(),
    }
}
