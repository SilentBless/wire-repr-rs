//! Borrowed dynamic struct view and cursor rendering.

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
                wire_lifetime,
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
        capabilities: Capabilities { has_validation, .. },
        ..
    } = input;
    let TypeShape {
        impl_generics,
        self_type,
    } = shape;
    let view_type_impl = quote! {
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
    };

    if operation_input_ty.is_some() {
        let operation_name = operation_name.expect("operation input");
        let operation_parse = operation_parse.expect("operation input");
        ReadFragments {
            request_declarations: quote! {
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
            },
            request_impls: quote! {
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
            },
            inherent_methods: quote! {
                #vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_input_request<'__wire_repr_view> {
                    #view_input_request { input }
                }
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor_input_request<'__wire_repr_view> {
                    #cursor_input_request { input }
                }
            },
            view_type_impl,
        }
    } else {
        let validation_mode = has_validation;
        let view_request = quote!(#runtime::ValidatedViewRequest);
        let view_signature = if wire_lifetime.is_some() {
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewRequest<'__wire_repr_view, #view<'__wire_repr_view>, #validation_mode>)
        } else {
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #runtime::ValidatedViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>, #validation_mode>)
        };
        ReadFragments {
            request_declarations: quote!(),
            request_impls: quote!(),
            inherent_methods: quote! {
                #view_signature {
                    #view_request::new(input)
                }
                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                    #runtime::ValidatedViewCursor::new(input)
                }
            },
            view_type_impl,
        }
    }
}
