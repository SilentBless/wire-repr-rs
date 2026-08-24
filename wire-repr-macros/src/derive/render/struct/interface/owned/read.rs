//! Owned `bytes` dynamic struct view and cursor rendering.

use super::TypeShape;
use crate::derive::render::r#struct::interface::{
    Capabilities, Identity, Input, Operation, ReadFragments, Requests, Surface, Types,
};
use quote::quote;

pub(super) fn render(input: &Input<'_>, shape: &TypeShape) -> ReadFragments {
    let Input {
        identity: Identity {
            view, decode_error, ..
        },
        surface: Surface { vis, runtime },
        types:
            Types {
                view_error_type,
                validation_error_type,
                association_error_type,
                ..
            },
        operation:
            Operation {
                input_ty: operation_input_ty,
                name: operation_name,
                parse: operation_parse,
                ..
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
        capabilities:
            Capabilities {
                fixed_sequence_width,
                has_validation,
            },
        ..
    } = input;
    let TypeShape {
        impl_generics,
        self_type,
    } = shape;
    let view_type_impl = quote! {
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'view> = #association_error_type;
            type View<'view> = #view;
        }
    };

    if let Some(operation_ty) = operation_input_ty {
        let operation_name = operation_name.expect("operation input");
        let operation_parse = operation_parse.expect("operation input");
        ReadFragments {
            request_declarations: quote! {
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
            },
            request_impls: quote! {
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
            },
            inherent_methods: quote! {
                #vis fn view(input: #runtime::__private::Bytes) -> #view_input_request {
                    #view_input_request { input }
                }
                #vis fn cursor(input: #runtime::__private::Bytes) -> #cursor_input_request {
                    #cursor_input_request { input }
                }
            },
            view_type_impl,
        }
    } else {
        let validation_mode = has_validation;
        let view_request = quote!(#runtime::ValidatedViewRequest);
        let view_cursor = quote!(#runtime::ValidatedViewCursor);
        let fixed_sequence = fixed_sequence_width.map(|width| {
            if *has_validation {
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
        ReadFragments {
            request_declarations: quote!(),
            request_impls: quote!(),
            inherent_methods: quote! {
                #fixed_sequence
                #vis fn view(input: #runtime::__private::Bytes) -> #runtime::ValidatedViewRequest<'static, #view, #validation_mode> {
                    #view_request::new(input)
                }
                #vis fn cursor(input: #runtime::__private::Bytes) -> #view_cursor<'static, #view> {
                    #view_cursor::new(input)
                }
            },
            view_type_impl,
        }
    }
}
