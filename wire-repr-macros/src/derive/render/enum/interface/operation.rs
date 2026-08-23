//! Operation-bound enum request, cursor, and encoding surface rendering.

use quote::quote;

use super::{
    CursorNames, EncodeTypes, ImplementationTypes, Input, Mode, Names, Operation, Output,
    RequestNames, SignatureTypes, Types, ViewTypes,
};

pub(super) fn render(input: Input<'_>) -> Output {
    let Names {
        request,
        cursor: cursor_names,
        encode_request,
    } = input.names;
    let RequestNames {
        vis,
        view,
        view_input_request,
        view_request,
        unchecked_view_request,
    } = request;
    let CursorNames {
        cursor_input_request,
        cursor,
        unchecked_cursor,
    } = cursor_names;

    let Types {
        implementation,
        view: view_types,
        encode: encode_types,
        signatures,
    } = input.types;
    let ImplementationTypes {
        impl_generics,
        self_type,
    } = implementation;
    let ViewTypes {
        validation_error,
        view_error,
        decode_error,
    } = view_types;
    let EncodeTypes { encode_error, plan } = encode_types;
    let SignatureTypes {
        view_signature: _,
        operation_view_signature,
        operation_encode_decl_generics,
        operation_encode_value_type,
        operation_encode_request_type,
    } = signatures;
    let Operation {
        input_ty,
        name: operation_name,
        parse: operation_parse,
        prepare: operation_prepare,
        prepare_arms,
    } = input.operation;
    let Mode {
        owned,
        static_view_request: _,
        static_cursor: _,
    } = input.mode;
    let runtime = input.runtime;

    let operation_input_ty = input_ty.expect("operation input");
    let operation_name = operation_name.expect("operation name");
    let operation_parse = operation_parse.expect("operation parser");
    let operation_prepare = operation_prepare.expect("operation preparer");
    let inherent = if owned {
        quote! {
            #[doc(hidden)]
            #vis struct #view_input_request {
                input: #runtime::__private::Bytes,
            }
            #[doc(hidden)]
            #vis struct #view_request<'operation> {
                input: #runtime::__private::Bytes,
                operation: &'operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_view_request<'operation> {
                input: #runtime::__private::Bytes,
                operation: &'operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #cursor_input_request {
                input: #runtime::__private::Bytes,
            }
            #[doc(hidden)]
            #vis struct #cursor<'operation> {
                remaining: #runtime::__private::Bytes,
                operation: &'operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_cursor<'operation> {
                remaining: #runtime::__private::Bytes,
                operation: &'operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #encode_request #operation_encode_decl_generics {
                value: #operation_encode_value_type,
                operation: &'__wire_repr_operation #operation_input_ty,
            }

            #[allow(missing_docs)]
            impl #view_input_request {
                #[must_use]
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #view_request<'_> {
                    #view_request {
                        input: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'operation> #view_request<'operation> {
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_view_request<'operation> {
                    #unchecked_view_request {
                        input: self.input,
                        operation: self.operation,
                    }
                }
                #vis fn with_remainder(self) -> Result<(#view, #runtime::__private::Bytes), #validation_error> {
                    let (view, remainder) = #view::#operation_parse(self.input, self.operation)
                        .map_err(<#validation_error as From<#view_error>>::from)?;
                    #runtime::WireViewValidation::validate(&view)?;
                    Ok((view, remainder))
                }
                #vis fn without_trailing(self) -> Result<#view, #validation_error> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(<#validation_error as From<#view_error>>::from(#decode_error::TrailingBytes {
                            expected: view.as_bytes().len(), actual: input_len,
                        }))
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'operation> #unchecked_view_request<'operation> {
                #vis fn with_remainder(self) -> Result<(#view, #runtime::__private::Bytes), #view_error> {
                    #view::#operation_parse(self.input, self.operation)
                }
                #vis fn without_trailing(self) -> Result<#view, #view_error> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len })
                    }
                }
            }
            #[allow(missing_docs)]
            impl #cursor_input_request {
                #[must_use]
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #cursor<'_> {
                    #cursor {
                        remaining: self.input,
                        operation,
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'operation> #cursor<'operation> {
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
                #vis fn next(&mut self) -> Result<Option<#view>, #runtime::ViewCursorError<#validation_error>> {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining.clone(), self.operation)
                        .map_err(|error| #runtime::ViewCursorError::Item(error.into()))?;
                    if suffix.len() == self.remaining.len() {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    #runtime::WireViewValidation::validate(&view).map_err(#runtime::ViewCursorError::Item)?;
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
                #vis fn next(&mut self) -> Result<Option<#view>, #runtime::ViewCursorError<#view_error>> {
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
            impl #operation_encode_decl_generics #encode_request #operation_encode_decl_generics {
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan, #encode_error>
                where #operation_encode_value_type: '__wire_repr_value,
                {
                    <#operation_encode_value_type>::#operation_prepare(self.value, self.operation)
                }
                #vis fn build_into<'__wire_repr_value, 'output>(
                    self,
                    output: &'output mut #runtime::__private::BytesMut,
                ) -> Result<
                    #runtime::Written<'output>,
                    #runtime::BuildIntoError<#encode_error>,
                >
                where #operation_encode_value_type: '__wire_repr_value,
                {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #operation_view_signature {
                    #view_input_request { input }
                }
                #vis fn cursor(
                    input: #runtime::__private::Bytes,
                ) -> #cursor_input_request {
                    #cursor_input_request { input }
                }
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #operation_encode_request_type {
                    #encode_request {
                        value: self,
                        operation,
                    }
                }
                #[doc(hidden)]
                #vis fn #operation_prepare<'__wire_repr_value>(
                    self,
                    operation: &#operation_input_ty,
                ) -> Result<#plan, #encode_error>
                where
                    Self: '__wire_repr_value,
                {
                    match self { #(#prepare_arms)* }
                }
            }
        }
    } else {
        quote! {
            #[doc(hidden)]
            #vis struct #view_input_request<'__wire_repr_wire> {
                input: &'__wire_repr_wire [u8],
            }
            #[doc(hidden)]
            #vis struct #view_request<'__wire_repr_wire, '__wire_repr_operation> {
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
            #vis struct #cursor<'__wire_repr_wire, '__wire_repr_operation> {
                remaining: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                remaining: &'__wire_repr_wire [u8],
                operation: &'__wire_repr_operation #operation_input_ty,
            }
            #[doc(hidden)]
            #vis struct #encode_request #operation_encode_decl_generics {
                value: #operation_encode_value_type,
                operation: &'__wire_repr_operation #operation_input_ty,
            }

            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #view_input_request<'__wire_repr_wire> {
                #[must_use]
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #view_request<'__wire_repr_wire, '_> {
                    #view_request {
                        input: self.input,
                        operation,
                    }
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation> #view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use]
                #vis fn unchecked(self) -> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                    #unchecked_view_request {
                        input: self.input,
                        operation: self.operation,
                    }
                }
                #vis fn with_remainder(
                    self,
                ) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #validation_error> {
                    let (view, remainder) = #view::#operation_parse(self.input, self.operation)
                        .map_err(<#validation_error as From<#view_error>>::from)?;
                    #runtime::WireViewValidation::validate(&view)?;
                    Ok((view, remainder))
                }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #validation_error> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(<#validation_error as From<#view_error>>::from(#decode_error::TrailingBytes {
                            expected: view.as_bytes().len(), actual: input_len,
                        }))
                    }
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation>
                #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation>
            {
                #vis fn with_remainder(
                    self,
                ) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #view_error> {
                    #view::#operation_parse(self.input, self.operation)
                }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #view_error> {
                    let input_len = self.input.len();
                    let (view, suffix) = self.with_remainder()?;
                    if suffix.is_empty() {
                        Ok(view)
                    } else {
                        Err(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len })
                    }
                }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire> #cursor_input_request<'__wire_repr_wire> {
                #[must_use]
                #vis fn #operation_name(self, operation: &#operation_input_ty) -> #cursor<'__wire_repr_wire, '_> {
                    #cursor {
                        remaining: self.input,
                        operation,
                    }
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation> #cursor<'__wire_repr_wire, '__wire_repr_operation> {
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
                #vis fn next(
                    &mut self,
                ) -> Result<
                    Option<#view<'__wire_repr_wire>>,
                    #runtime::ViewCursorError<#validation_error>,
                > {
                    if self.remaining.is_empty() {
                        return Ok(None);
                    }
                    let (view, suffix) = #view::#operation_parse(self.remaining, self.operation)
                        .map_err(|error| #runtime::ViewCursorError::Item(error.into()))?;
                    if suffix.len() == self.remaining.len() {
                        return Err(#runtime::ViewCursorError::EmptyItem);
                    }
                    #runtime::WireViewValidation::validate(&view).map_err(#runtime::ViewCursorError::Item)?;
                    self.remaining = suffix;
                    Ok(Some(view))
                }
            }
            impl<'__wire_repr_wire, '__wire_repr_operation>
                #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation>
            {
                #[must_use]
                #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] {
                    self.remaining
                }
                #vis fn next(
                    &mut self,
                ) -> Result<
                    Option<#view<'__wire_repr_wire>>,
                    #runtime::ViewCursorError<#view_error>,
                > {
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
            impl #operation_encode_decl_generics #encode_request #operation_encode_decl_generics {
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan, #encode_error>
                where #operation_encode_value_type: '__wire_repr_value,
                {
                    <#operation_encode_value_type>::#operation_prepare(self.value, self.operation)
                }
                #vis fn build_into<'__wire_repr_value, '__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<
                    (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                    #runtime::BuildIntoError<#encode_error>,
                >
                where #operation_encode_value_type: '__wire_repr_value,
                {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #operation_view_signature {
                    #view_input_request { input }
                }
                #vis fn cursor<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #cursor_input_request<'__wire_repr_view> {
                    #cursor_input_request { input }
                }
                #vis fn #operation_name(
                    self,
                    operation: &#operation_input_ty,
                ) -> #operation_encode_request_type {
                    #encode_request {
                        value: self,
                        operation,
                    }
                }
                #[doc(hidden)]
                #vis fn #operation_prepare<'__wire_repr_value>(
                    self,
                    operation: &#operation_input_ty,
                ) -> Result<#plan, #encode_error>
                where
                    Self: '__wire_repr_value,
                {
                    match self { #(#prepare_arms)* }
                }
            }
        }
    };

    Output {
        inherent,
        encode_trait: quote!(),
    }
}
