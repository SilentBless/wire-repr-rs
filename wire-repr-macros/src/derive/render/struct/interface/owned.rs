//! Owned `bytes` struct interface rendering.

use super::{Capabilities, Identity, Input, Operation, Preparation, Requests, Surface, Types};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn render(input: Input<'_>) -> TokenStream {
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
                fixed_sequence_width,
                has_validation,
            },
    } = input;
    let (impl_generics, self_type) = wire_lifetime.map_or_else(
        || (quote!(), quote!(#name)),
        |lifetime| (quote!(<#lifetime>), quote!(#name<#lifetime>)),
    );
    let validation_mode = has_validation;
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let view_cursor = quote!(#runtime::ValidatedViewCursor);

    let fixed_sequence = fixed_sequence_width.map(|width| {
        if has_validation {
            quote! {
                /// Structurally frames fixed-width items without running semantic validators.
                #vis fn unchecked_views(input: #runtime::__private::Bytes) -> Result<#runtime::FixedViewIterator<'static, #view>, #runtime::FixedViewSequenceError> {
                    #runtime::FixedViewIterator::new(input, #width, #view::from_sequence)
                }
                /// Frames and semantically validates every fixed-width item before returning an infallible iterator.
                #vis fn views(input: #runtime::__private::Bytes) -> Result<#runtime::FixedViewIterator<'static, #view>, #runtime::FixedValidatedViewSequenceError<#validation_error_type>> {
                    let iterator = #runtime::FixedViewIterator::new(input, #width, #view::from_sequence).map_err(#runtime::FixedValidatedViewSequenceError::Framing)?;
                    for view in iterator.clone() {
                        #runtime::WireViewValidation::validate(&view)
                            .map_err(#runtime::FixedValidatedViewSequenceError::Item)?;
                    }
                    Ok(iterator)
                }
            }
        } else {
            quote! {
                /// Validates complete fixed-width sequence framing and returns an infallible iterator.
                #vis fn views(input: #runtime::__private::Bytes) -> Result<#runtime::FixedViewIterator<'static, #view>, #runtime::FixedViewSequenceError> {
                    #runtime::FixedViewIterator::new(input, #width, #view::from_sequence)
                }
            }
        }
    });
    let inherent = if let Some(operation_ty) = operation_input_ty {
        let operation_name = operation_name.expect("operation input");
        let operation_parse = operation_parse.expect("operation input");
        let operation_prepare = operation_prepare.expect("operation input");
        let encode_generics = wire_lifetime.map_or_else(
            || quote!(<'__wire_repr_operation>),
            |lifetime| quote!(<#lifetime, '__wire_repr_operation>),
        );
        let encode_request_type = wire_lifetime.map_or_else(
            || quote!(#encode_request<'_>),
            |lifetime| quote!(#encode_request<#lifetime, '_>),
        );
        quote! {
            #[doc(hidden)]
            #vis struct #view_input_request {
                input: #runtime::__private::Bytes,
            }
            #[doc(hidden)]
            #vis struct #direct_view_request<'operation> {
                input: #runtime::__private::Bytes,
                operation: &'operation #operation_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_view_request<'operation> {
                input: #runtime::__private::Bytes,
                operation: &'operation #operation_ty,
            }
            #[doc(hidden)]
            #vis struct #cursor_input_request {
                input: #runtime::__private::Bytes,
            }
            #[doc(hidden)]
            #vis struct #direct_cursor<'operation> {
                remaining: #runtime::__private::Bytes,
                operation: &'operation #operation_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_cursor<'operation> {
                remaining: #runtime::__private::Bytes,
                operation: &'operation #operation_ty,
            }
            #[doc(hidden)]
            #vis struct #encode_request #encode_generics {
                value: #self_type,
                operation: &'__wire_repr_operation #operation_ty,
            }

            #[allow(missing_docs)]
            impl #view_input_request {
                #[must_use]
                #vis fn #operation_name(self, operation: &#operation_ty) -> #direct_view_request<'_> {
                    #direct_view_request {
                        input: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'operation> #direct_view_request<'operation> {
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_view_request<'operation> {
                    #unchecked_view_request {
                        input: self.input,
                        operation: self.operation,
                    }
                }
                #vis fn with_remainder(self) -> Result<(#view, #runtime::__private::Bytes), #validation_error_type> {
                    let (view, remainder) = #view::#operation_parse(self.input, self.operation)?;
                    #runtime::WireViewValidation::validate(&view)?;
                    Ok((view, remainder))
                }
                #vis fn without_trailing(self) -> Result<#view, #validation_error_type> {
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
            impl<'operation> #unchecked_view_request<'operation> {
                #vis fn with_remainder(self) -> Result<(#view, #runtime::__private::Bytes), #view_error_type> {
                    #view::#operation_parse(self.input, self.operation)
                }
                #vis fn without_trailing(self) -> Result<#view, #view_error_type> {
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
            impl #cursor_input_request {
                #[must_use]
                #vis fn #operation_name(self, operation: &#operation_ty) -> #direct_cursor<'_> {
                    #direct_cursor {
                        remaining: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'operation> #direct_cursor<'operation> {
                #[must_use]
                #vis fn remaining(&self) -> &[u8] {
                    &self.remaining
                }
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_cursor<'operation> {
                    #unchecked_cursor {
                        remaining: self.remaining,
                        operation: self.operation,
                    }
                }
                #vis fn next(&mut self) -> Result<Option<#view>, #runtime::ViewCursorError<#validation_error_type>> {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining.clone(), self.operation)
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
            impl<'operation> #unchecked_cursor<'operation> {
                #[must_use]
                #vis fn remaining(&self) -> &[u8] {
                    &self.remaining
                }
                #vis fn next(&mut self) -> Result<Option<#view>, #runtime::ViewCursorError<#view_error_type>> {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining.clone(), self.operation)
                        .map_err(#runtime::ViewCursorError::Item)?;
                    if suffix.len() == self.remaining.len() {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    self.remaining = suffix;
                    Ok(Some(view))
                }
            }
            #[allow(missing_docs)]
            impl #encode_generics #encode_request #encode_generics {
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type>
                where
                    #self_type: '__wire_repr_value,
                {
                    <#self_type>::#operation_prepare(self.value, self.operation)
                }
                #vis fn build_into<'__wire_repr_value, 'output>(
                    self,
                    output: &'output mut #runtime::__private::BytesMut,
                ) -> Result<#runtime::Written<'output>, #runtime::BuildIntoError<#encode_error_type>>
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
                #vis fn view(input: #runtime::__private::Bytes) -> #view_input_request {
                    #view_input_request { input }
                }
                #vis fn cursor(input: #runtime::__private::Bytes) -> #cursor_input_request {
                    #cursor_input_request { input }
                }
                #vis fn #operation_name(self, operation: &#operation_ty) -> #encode_request_type {
                    #encode_request {
                        value: self,
                        operation,
                    }
                }
                #[doc(hidden)]
                #vis fn #operation_prepare<'__wire_repr_value>(self, #operation_value: &#operation_ty) -> Result<#plan_type, #encode_error_type>
                where
                    Self: '__wire_repr_value,
                {
                    let Self { #(#prepare_destructure,)* } = self;
                    Self::#prepare_helper(#(#prepare_field_names,)* #operation_value)
                }
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters,)* #operation_value: &#operation_ty) -> Result<#plan_type, #encode_error_type>
                where
                    Self: '__wire_repr_value,
                {
                    #preparation_body
                }
            }
        }
    } else {
        quote! {
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #fixed_sequence
                #vis fn view(input: #runtime::__private::Bytes) -> #runtime::ValidatedViewRequest<'static, #view, #validation_mode> {
                    #view_request::new(input)
                }
                #vis fn cursor(input: #runtime::__private::Bytes) -> #view_cursor<'static, #view> {
                    #view_cursor::new(input)
                }
                #builder_method
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters),*) -> Result<#plan_type, #encode_error_type>
                where
                    Self: '__wire_repr_value,
                {
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
                #vis fn build_into<'output>(
                    self,
                    output: &'output mut #runtime::__private::BytesMut,
                ) -> Result<#runtime::Written<'output>, #runtime::BuildIntoError<#encode_error_type>> {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output)
                        .map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    };
    let encode_impl = operation_input_ty.is_none().then(|| quote! {
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
    });
    quote! {
        #inherent
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'view> = #association_error_type;
            type View<'view> = #view;
        }
        #encode_impl
    }
}
