//! Borrowed dynamic struct encoding rendering.

use super::TypeShape;
use crate::derive::render::r#struct::interface::{Input, Operation, Preparation, Surface, Types};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) struct Fragments {
    pub(super) encode_request_declaration: TokenStream,
    pub(super) encode_request_impl: TokenStream,
    pub(super) inherent_methods: TokenStream,
    pub(super) encode_impl: TokenStream,
}

pub(super) fn render(input: &Input<'_>, shape: &TypeShape) -> Fragments {
    let Input {
        identity: _,
        surface: Surface { vis, runtime },
        types:
            Types {
                wire_lifetime,
                plan_type,
                encode_error_type,
                ..
            },
        operation:
            Operation {
                input_ty: operation_input_ty,
                name: operation_name,
                prepare: operation_prepare,
                value: operation_value,
                encode_request,
                ..
            },
        preparation:
            Preparation {
                helper: prepare_helper,
                body: preparation_body,
                field_parameters: prepare_field_parameters,
                destructure: prepare_destructure,
                field_names: prepare_field_names,
                builder_method,
            },
        ..
    } = input;
    let TypeShape {
        impl_generics,
        self_type,
    } = shape;

    if operation_input_ty.is_some() {
        let operation_name = operation_name.expect("operation input");
        let operation_prepare = operation_prepare.expect("operation input");
        let operation_encode_generics = if let Some(lifetime) = wire_lifetime {
            quote!(<#lifetime, '__wire_repr_operation>)
        } else {
            quote!(<'__wire_repr_operation>)
        };
        let operation_encode_request_type = if let Some(lifetime) = wire_lifetime {
            quote!(#encode_request<#lifetime, '_>)
        } else {
            quote!(#encode_request<'_>)
        };
        Fragments {
            encode_request_declaration: quote! {
                #[doc(hidden)]
                #vis struct #encode_request #operation_encode_generics {
                    value: #self_type,
                    operation: &'__wire_repr_operation #operation_input_ty,
                }
            },
            encode_request_impl: quote! {
                #[allow(missing_docs)]
                impl #operation_encode_generics #encode_request #operation_encode_generics {
                    #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type>
                    where
                        #self_type: '__wire_repr_value,
                    {
                        <#self_type>::#operation_prepare(self.value, self.operation)
                    }
                    #vis fn build_into<'__wire_repr_value, '__wire_repr_output>(
                        self,
                        output: &'__wire_repr_output mut [u8],
                    ) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>>
                    where
                        #self_type: '__wire_repr_value,
                    {
                        let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                        #runtime::PreparedLayout::commit_into(plan, output)
                            .map_err(#runtime::BuildIntoError::Output)
                    }
                }
            },
            inherent_methods: quote! {
                #vis fn #operation_name(self, operation: &#operation_input_ty) -> #operation_encode_request_type {
                    #encode_request {
                        value: self,
                        operation,
                    }
                }
                #[doc(hidden)]
                #vis fn #operation_prepare<'__wire_repr_value>(self, #operation_value: &#operation_input_ty) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    let Self { #(#prepare_destructure,)* } = self;
                    Self::#prepare_helper(#(#prepare_field_names,)* #operation_value)
                }
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters,)* #operation_value: &#operation_input_ty) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    #preparation_body
                }
            },
            encode_impl: quote!(),
        }
    } else {
        Fragments {
            encode_request_declaration: quote!(),
            encode_request_impl: quote!(),
            inherent_methods: quote! {
                #builder_method
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters),*) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    #preparation_body
                }
                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type>
                where
                    Self: '__wire_repr_value,
                {
                    <Self as #runtime::WireEncode>::prepare(self)
                }
                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output)
                        .map_err(#runtime::BuildIntoError::Output)
                }
            },
            encode_impl: quote! {
                impl #impl_generics #runtime::WireEncode for #self_type {
                    type EncodeError = #encode_error_type;
                    type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;
                    fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
                    where
                        Self: '__wire_repr_value,
                    {
                        let Self { #(#prepare_destructure,)* } = self;
                        Self::#prepare_helper(#(#prepare_field_names),*)
                    }
                }
            },
        }
    }
}
