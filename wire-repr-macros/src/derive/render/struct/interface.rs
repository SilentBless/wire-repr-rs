//! Generated struct public interface and trait wiring rendering.

#[path = "interface/owned.rs"]
mod owned;

use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) identity: Identity<'a>,
    pub(super) surface: Surface<'a>,
    pub(super) types: Types<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) requests: Requests<'a>,
    pub(super) preparation: Preparation<'a>,
    pub(super) capabilities: Capabilities<'a>,
}

pub(super) struct Identity<'a> {
    pub(super) name: &'a Ident,
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
}

pub(super) struct Surface<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Types<'a> {
    pub(super) wire_lifetime: Option<&'a syn::Lifetime>,
    pub(super) view_error_type: &'a TokenStream,
    pub(super) validation_error_type: &'a TokenStream,
    pub(super) association_error_type: &'a TokenStream,
    pub(super) plan_type: &'a TokenStream,
    pub(super) encode_error_type: &'a TokenStream,
}

pub(super) struct Operation<'a> {
    pub(super) input_ty: Option<&'a syn::Path>,
    pub(super) name: Option<&'a Ident>,
    pub(super) parse: Option<&'a Ident>,
    pub(super) prepare: Option<&'a Ident>,
    pub(super) value: &'a Ident,
    pub(super) encode_request: &'a Ident,
}

pub(super) struct Requests<'a> {
    pub(super) view_input: &'a Ident,
    pub(super) direct_view: &'a Ident,
    pub(super) unchecked_view: &'a Ident,
    pub(super) cursor_input: &'a Ident,
    pub(super) direct_cursor: &'a Ident,
    pub(super) unchecked_cursor: &'a Ident,
}

pub(super) struct Preparation<'a> {
    pub(super) helper: &'a Ident,
    pub(super) body: &'a TokenStream,
    pub(super) field_parameters: &'a [TokenStream],
    pub(super) destructure: &'a [TokenStream],
    pub(super) field_names: &'a [&'a Ident],
    pub(super) builder_method: &'a TokenStream,
}

pub(super) struct Capabilities<'a> {
    pub(super) fixed_sequence_width: Option<&'a TokenStream>,
    pub(super) has_validation: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    if cfg!(feature = "bytes") {
        return owned::render(input);
    }
    let Input {
        identity: Identity {
            name,
            view,
            decode_error,
        },
        surface: Surface { vis, runtime },
        types:
            Types {
                wire_lifetime,
                view_error_type,
                validation_error_type,
                association_error_type,
                plan_type,
                encode_error_type,
            },
        operation:
            Operation {
                input_ty: operation_input_ty,
                name: operation_name,
                parse: operation_parse,
                prepare: operation_prepare,
                value: operation_value,
                encode_request,
            },
        requests:
            Requests {
                view_input: view_input_request,
                direct_view: direct_view_request,
                unchecked_view: unchecked_view_request,
                cursor_input: cursor_input_request,
                direct_cursor,
                unchecked_cursor,
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
        capabilities:
            Capabilities {
                fixed_sequence_width: _,
                has_validation: _,
            },
    } = input;
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let cursor_method = quote! {
        /// Returns a fail-closed cursor over consecutive representations.
        #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
            #runtime::ValidatedViewCursor::new(input)
        }
    };
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_request<'__wire_repr_view, #view<'__wire_repr_view>>),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>),
        )
    };

    let inherent_impl = if operation_input_ty.is_some() {
        let operation_name = operation_name.expect("operation input");
        let operation_parse = operation_parse.expect("operation input");
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
        quote! {
            #[doc(hidden)]
            #vis struct #view_input_request<'__wire_repr_wire> {
                input: &'__wire_repr_wire [u8],
            }
            #[doc(hidden)]
            #vis struct #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                input: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                input: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #cursor_input_request<'__wire_repr_wire> {
                input: &'__wire_repr_wire [u8],
            }
            #[doc(hidden)]
            #vis struct #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                remaining: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                remaining: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #encode_request #operation_encode_generics {
                value: #self_type,
                operation: &'__wire_repr_operation #operation_input_ty,
            }

            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #view_input_request<'__wire_repr_wire> {
                #[must_use]
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #direct_view_request<'__wire_repr_wire, '_> {
                    #direct_view_request {
                        input: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                    #unchecked_view_request {
                        input: self.input,
                        operation: self.operation,
                    }
                }
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #validation_error_type> {
                    let (view, remainder) = #view::#operation_parse(self.input, self.operation)?;
                    #runtime::WireViewValidation::validate(&view)?;
                    Ok((view, remainder))
                }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #validation_error_type> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(<#validation_error_type as From<#view_error_type>>::from(
                            #decode_error::TrailingBytes {
                                expected: view.as_bytes().len(),
                                actual: input_len,
                            },
                        ))
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #view_error_type> {
                    #view::#operation_parse(self.input, self.operation)
                }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #view_error_type> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(#decode_error::TrailingBytes {
                            expected: view.as_bytes().len(),
                            actual: input_len,
                        })
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #cursor_input_request<'__wire_repr_wire> {
                #[must_use]
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #direct_cursor<'__wire_repr_wire, '_> {
                    #direct_cursor {
                        remaining: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use]
                #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] {
                    self.remaining
                }
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                    #unchecked_cursor {
                        remaining: self.remaining,
                        operation: self.operation,
                    }
                }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#validation_error_type>> {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining, self.operation)
                        .map_err(|error| #runtime::ViewCursorError::Item(error.into()))?;
                    if suffix.len() == self.remaining.len() {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    #runtime::WireViewValidation::validate(&view)
                        .map_err(#runtime::ViewCursorError::Item)?;
                    self.remaining = suffix;
                    Ok(Some(view))
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use]
                #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] {
                    self.remaining
                }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#view_error_type>> {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining, self.operation)
                        .map_err(#runtime::ViewCursorError::Item)?;
                    if suffix.len() == self.remaining.len() {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    self.remaining = suffix;
                    Ok(Some(view))
                }
            }
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
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #builder_method
                #vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_input_request<'__wire_repr_view> {
                    #view_input_request { input }
                }
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor_input_request<'__wire_repr_view> {
                    #cursor_input_request { input }
                }
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
            }
        }
    } else {
        quote! {
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #view_signature {
                    #view_request::new(input)
                }
                #cursor_method
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
            }
        }
    };
    let encode_impl = if operation_input_ty.is_some() {
        quote!()
    } else {
        quote! {
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
        }
    };

    quote! {
        #inherent_impl
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
        #encode_impl
    }
}
