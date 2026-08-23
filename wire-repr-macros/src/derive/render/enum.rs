//! Enum derive rendering.
mod error;
mod interface;
mod plan;
mod view;
use super::super::model::{EnumTag, UnknownPolicy, VariantSelector, WireEnum};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
pub(super) fn render(model: WireEnum, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireEnum {
        vis,
        name,
        wire_lifetime,
        tag,
        unknown,
        operation_input,
        variants,
    } = model;
    let operation = operation_input.as_ref().map(|input| &input.ty);
    let operation_error = operation_input
        .as_ref()
        .and_then(|input| input.error.as_ref());
    let plan = format_ident!("{name}Plan");
    let field_proxy = format_ident!("{name}Fields");
    let view = format_ident!("{name}View");
    let view_variant = format_ident!("__{name}ViewVariant");
    let view_input_request = format_ident!("{name}ViewInputRequest");
    let view_request = format_ident!("{name}ViewRequest");
    let unchecked_view_request = format_ident!("{name}UncheckedViewRequest");
    let cursor_input_request = format_ident!("{name}CursorInputRequest");
    let cursor = format_ident!("{name}Cursor");
    let unchecked_cursor = format_ident!("{name}UncheckedCursor");
    let encode_request = format_ident!("{name}EncodeRequest");
    let operation_parse = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_parse_with_{}", input.name));
    let operation_prepare = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_prepare_with_{}", input.name));
    let decode_error = format_ident!("{name}DecodeError");
    let validation_error = format_ident!("{name}ValidationError");
    let encode_error = format_ident!("{name}EncodeError");
    let (tag_codec, tag_type, byte_tag_width) = match &tag {
        EnumTag::Integer(tag) => {
            let codec = format_ident!("{}", tag.codec);
            let ty = format_ident!("{}", tag.ty);
            let codec = if tag.builtin {
                quote!(#runtime::#codec)
            } else {
                quote!(#codec)
            };
            (codec, quote!(#ty), None)
        }
        EnumTag::Bytes { width } => (
            quote!(#runtime::__private::OwnedBytes<#width>),
            quote!([u8; #width]),
            Some(*width),
        ),
    };
    let preserves_unknown = matches!(unknown, UnknownPolicy::Preserve);
    let unknown_variant = variants
        .iter()
        .find(|variant| matches!(variant.selector, VariantSelector::Unknown));
    let operation_input_ty = operation;
    let uses_operation_input = operation_input_ty.is_some();
    let has_body = variants.iter().any(|variant| {
        variant.body.is_some() && !matches!(variant.selector, VariantSelector::Unknown)
    });
    let static_view_request = if has_body {
        quote!(#runtime::ValidatedViewRequest)
    } else {
        quote!(#runtime::ViewRequest)
    };
    let owned = cfg!(feature = "bytes");
    let view_variant_has_lifetime =
        !owned && (has_body || (preserves_unknown && byte_tag_width.is_some()));

    let body_view_paths: Vec<_> = variants
        .iter()
        .map(|variant| {
            if matches!(variant.selector, VariantSelector::Unknown) {
                Ok(None)
            } else {
                variant
                    .body
                    .as_ref()
                    .map(super::generated_view_path)
                    .transpose()
            }
        })
        .collect::<syn::Result<_>>()?;
    let (
        impl_generics,
        self_type,
        encode_error_type,
        encode_error_impl_type,
        plan_decl_generics,
        plan_decl_where,
        plan_type,
        plan_impl_generics,
        plan_impl_type,
        view_signature,
    ) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#encode_error<#lifetime>),
            quote!(#encode_error<'_>),
            quote!(<#lifetime, '__wire_repr_value>),
            quote!(where #lifetime: '__wire_repr_value),
            quote!(#plan<#lifetime, '__wire_repr_value>),
            quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
            quote!(#plan<#lifetime, '__wire_repr_value>),
            if owned {
                quote!(
                    #vis fn view(
                        input: #runtime::__private::Bytes
                    ) -> #static_view_request<'static, #view>
                )
            } else {
                quote!(
                    #vis fn view<'__wire_repr_view>(
                        input: &'__wire_repr_view [u8]
                    ) -> #static_view_request<'__wire_repr_view, #view<'__wire_repr_view>>
                )
            },
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#encode_error),
            quote!(#encode_error),
            quote!(<'__wire_repr_value>),
            quote!(),
            quote!(#plan<'__wire_repr_value>),
            quote!(<'__wire_repr_value>),
            quote!(#plan<'__wire_repr_value>),
            if owned {
                quote!(
                    #vis fn view(
                        input: #runtime::__private::Bytes
                    ) -> #static_view_request<'static, #view>
                )
            } else {
                quote!(
                    #vis fn view<'__wire_repr_wire>(
                        input: &'__wire_repr_wire [u8]
                    ) -> #static_view_request<'__wire_repr_wire, #view<'__wire_repr_wire>>
                )
            },
        )
    };
    let (
        decode_error_decl_generics,
        view_error_type,
        association_error_type,
        decode_error_impl_type,
    ) = if owned {
        (
            quote!(),
            quote!(#decode_error),
            quote!(#decode_error),
            quote!(#decode_error),
        )
    } else if has_body || byte_tag_width.is_some() || operation_input.is_some() {
        (
            quote!(<'__wire_repr_wire>),
            quote!(#decode_error<'__wire_repr_wire>),
            quote!(#decode_error<'__wire_repr_view>),
            quote!(#decode_error<'_>),
        )
    } else {
        (
            quote!(),
            quote!(#decode_error),
            quote!(#decode_error),
            quote!(#decode_error),
        )
    };
    let validation_error_type = if owned && has_body {
        quote!(#validation_error)
    } else if has_body {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#view_error_type)
    };
    let validation_error_impl_type = if owned && has_body {
        quote!(#validation_error)
    } else if has_body {
        quote!(#validation_error<'_>)
    } else {
        quote!(#view_error_type)
    };
    let static_cursor = if has_body {
        quote!(#runtime::ValidatedViewCursor)
    } else {
        quote!(#runtime::ViewCursor)
    };
    let encode_error_decl_generics = wire_lifetime
        .as_ref()
        .map_or_else(|| quote!(), |lifetime| quote!(<#lifetime>));
    let operation_view_signature = if owned {
        quote!(
            #vis fn view(input: #runtime::__private::Bytes) -> #view_input_request
        )
    } else {
        quote!(
            #vis fn view<'__wire_repr_view>(
                input: &'__wire_repr_view [u8]
            ) -> #view_input_request<'__wire_repr_view>
        )
    };
    let (operation_encode_decl_generics, operation_encode_value_type) =
        if let Some(lifetime) = &wire_lifetime {
            (
                quote!(<#lifetime, '__wire_repr_operation>),
                quote!(#name<#lifetime>),
            )
        } else {
            (quote!(<'__wire_repr_operation>), quote!(#name))
        };
    let operation_encode_request_type = if let Some(lifetime) = &wire_lifetime {
        quote!(#encode_request<#lifetime, '_>)
    } else {
        quote!(#encode_request<'_>)
    };
    let plan_output = plan::render(plan::Input {
        schema: plan::Schema {
            vis: &vis,
            name: &name,
            variants: &variants,
            tag_codec: &tag_codec,
            tag_type: &tag_type,
        },
        plan: plan::Plan {
            uses_operation_input,
            plan: &plan,
            encode_error: &encode_error,
        },
        generics: plan::Generics {
            plan_decl_generics: &plan_decl_generics,
            plan_decl_where: &plan_decl_where,
            plan_impl_generics: &plan_impl_generics,
            plan_impl_type: &plan_impl_type,
        },
        wire_lifetime: wire_lifetime.as_ref(),
        runtime,
    });
    let prepare_arms = plan_output.prepare_arms;
    let plan_declaration = plan_output.declaration;
    let error::Output {
        before_view: error_before_view,
        after_field_proxy: error_after_field_proxy,
    } = error::render(error::Input {
        schema: error::Schema {
            vis: &vis,
            variants: &variants,
            body_view_paths: &body_view_paths,
            unknown,
            tag_type: &tag_type,
            byte_tag_width,
            has_body,
        },
        operation: error::Operation {
            error: operation_error,
            uses_input: uses_operation_input,
        },
        owned,
        errors: error::Errors {
            decode: error::DecodeErrors {
                decode_error: &decode_error,
                decode_error_decl_generics: &decode_error_decl_generics,
                decode_error_impl_type: &decode_error_impl_type,
                view_error_type: &view_error_type,
            },
            validation: error::ValidationErrors {
                validation_error: &validation_error,
                validation_error_impl_type: &validation_error_impl_type,
            },
            encode: error::EncodeErrors {
                encode_error: &encode_error,
                encode_error_decl_generics: &encode_error_decl_generics,
                encode_error_impl_type: &encode_error_impl_type,
            },
        },
        runtime,
    });
    let interface::Output {
        inherent: inherent_impl,
        encode_trait: encode_impl,
    } = interface::render(interface::Input {
        names: interface::Names {
            request: interface::RequestNames {
                vis: &vis,
                view: &view,
                view_input_request: &view_input_request,
                view_request: &view_request,
                unchecked_view_request: &unchecked_view_request,
            },
            cursor: interface::CursorNames {
                cursor_input_request: &cursor_input_request,
                cursor: &cursor,
                unchecked_cursor: &unchecked_cursor,
            },
            encode_request: &encode_request,
        },
        types: interface::Types {
            implementation: interface::ImplementationTypes {
                impl_generics: &impl_generics,
                self_type: &self_type,
            },
            view: interface::ViewTypes {
                validation_error: &validation_error_type,
                view_error: &view_error_type,
                decode_error: &decode_error,
            },
            encode: interface::EncodeTypes {
                encode_error: &encode_error_type,
                plan: &plan_type,
            },
            signatures: interface::SignatureTypes {
                view_signature: &view_signature,
                operation_view_signature: &operation_view_signature,
                operation_encode_decl_generics: &operation_encode_decl_generics,
                operation_encode_value_type: &operation_encode_value_type,
                operation_encode_request_type: &operation_encode_request_type,
            },
        },
        operation: interface::Operation {
            input_ty: operation_input_ty,
            name: operation_input.as_ref().map(|input| &input.name),
            parse: operation_parse.as_ref(),
            prepare: operation_prepare.as_ref(),
            prepare_arms: &prepare_arms,
        },
        mode: interface::Mode {
            owned,
            static_view_request: &static_view_request,
            static_cursor: &static_cursor,
        },
        runtime,
    });
    let view_declaration = view::render(view::Input {
        names: view::Names {
            vis: &vis,
            view: &view,
            view_variant: &view_variant,
            decode_error: &decode_error,
            validation_error: &validation_error,
            operation_parse: operation_parse.as_ref(),
        },
        types: view::Types {
            tag_codec: &tag_codec,
            tag_type: &tag_type,
            view_error: &view_error_type,
            validation_error: &validation_error_type,
        },
        geometry: view::Geometry {
            schema: view::Schema {
                variants: &variants,
                body_view_paths: &body_view_paths,
                unknown_variant,
            },
            framing: view::Framing {
                byte_tag_width,
                view_variant_has_lifetime,
            },
            operation: view::Operation {
                uses_input: uses_operation_input,
                operation_input: operation_input_ty,
            },
            owned,
        },
        runtime,
    });
    let view_association = if owned {
        quote!(#view)
    } else {
        quote!(#view<'__wire_repr_view>)
    };
    Ok(quote! {
        #error_before_view

        #view_declaration

        /// Generated field-selection proxy for this tagged wire enum.
        #[doc(hidden)]
        #vis struct #field_proxy<S: #runtime::MarkerScope = #runtime::RootScope>(
            ::core::marker::PhantomData<fn() -> S>,
        );

        impl<S: #runtime::MarkerScope> Copy for #field_proxy<S> {}

        impl<S: #runtime::MarkerScope> Clone for #field_proxy<S> {
            fn clone(&self) -> Self {
                *self
            }
        }

        #[allow(missing_docs)]
        impl<S: #runtime::MarkerScope> #field_proxy<S> {
            #[doc(hidden)]
            #vis fn __wire_repr_new() -> Self {
                Self(::core::marker::PhantomData)
            }
        }

        #error_after_field_proxy

        #plan_declaration

        #inherent_impl

        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view_association;
        }

        #encode_impl
    })
}
