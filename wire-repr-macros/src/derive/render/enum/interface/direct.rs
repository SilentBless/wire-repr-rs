//! Direct enum request, cursor, and encoding surface rendering.

use quote::quote;

use super::{
    EncodeTypes, ImplementationTypes, Input, Mode, Names, Operation, Output, RequestNames,
    SignatureTypes, Types,
};

pub(super) fn render(input: Input<'_>) -> Output {
    let Names { request, .. } = input.names;
    let RequestNames { vis, view, .. } = request;
    let Types {
        implementation,
        encode: EncodeTypes { encode_error, plan },
        signatures: SignatureTypes { view_signature, .. },
        ..
    } = input.types;
    let ImplementationTypes {
        impl_generics,
        self_type,
    } = implementation;
    let Operation { prepare_arms, .. } = input.operation;
    let Mode {
        owned,
        static_view_request,
        static_cursor,
    } = input.mode;
    let runtime = input.runtime;

    let inherent = if owned {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature {
                    #static_view_request::new(input)
                }
                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor(
                    input: #runtime::__private::Bytes,
                ) -> #static_cursor<'static, #view> {
                    #static_cursor::new(input)
                }
                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan, #encode_error>
                where
                    Self: '__wire_repr_value,
                {
                    <Self as #runtime::WireEncode>::prepare(self)
                }
                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut #runtime::__private::BytesMut,
                ) -> Result<
                    #runtime::Written<'__wire_repr_output>,
                    #runtime::BuildIntoError<#encode_error>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature {
                    #static_view_request::new(input)
                }

                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #static_cursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                    #static_cursor::new(input)
                }

                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan, #encode_error>
                where
                    Self: '__wire_repr_value,
                {
                    <Self as #runtime::WireEncode>::prepare(self)
                }

                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(
                    self,
                    output: &'__wire_repr_output mut [u8],
                ) -> Result<
                    (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                    #runtime::BuildIntoError<#encode_error>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    };
    let encode_trait = quote! {
        impl #impl_generics #runtime::WireEncode for #self_type {
            type EncodeError = #encode_error;
            type Plan<'__wire_repr_value> = #plan
            where
                Self: '__wire_repr_value;

            fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
            where Self: '__wire_repr_value,
            {
                match self { #(#prepare_arms)* }
            }
        }
    };
    Output {
        inherent,
        encode_trait,
    }
}
