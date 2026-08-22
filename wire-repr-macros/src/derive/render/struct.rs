//! Struct derive rendering.

mod builder;
mod fixed;
mod plan;
mod selection;

use super::super::model::{
    Codec, ComputationArgument, ComputationByteSelection, Field, FieldKind, FieldPosition,
    WireStruct,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    if model.operation_input.is_none()
        && model
            .fields
            .iter()
            .all(|field| matches!(field.kind, FieldKind::Fixed(_)) && field.computation.is_none())
    {
        return fixed::render(model, runtime);
    }

    let WireStruct {
        vis,
        name,
        wire_lifetime,
        operation_input,
        validators: model_validators,
        validation_error,
        fields,
        preparation,
    } = model;
    let computation_order = preparation.computation_order;
    let controlled_by = preparation.controlled_by;
    let position_sources = preparation.position_sources;
    let plan = format_ident!("{name}Plan");
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let operation_input_ty = operation_input.as_ref().map(|input| &input.ty);
    let operation_name = operation_input.as_ref().map(|input| &input.name);
    let operation_parse = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_parse_with_{}", input.name));
    let operation_prepare = operation_input
        .as_ref()
        .map(|input| format_ident!("__wire_repr_prepare_with_{}", input.name));
    let operation_value = format_ident!("__wire_repr_operation");
    let view_input_request = format_ident!("{name}ViewInputRequest");
    let direct_view_request = format_ident!("{name}ViewRequest");
    let unchecked_view_request = format_ident!("{name}UncheckedViewRequest");
    let cursor_input_request = format_ident!("{name}CursorInputRequest");
    let direct_cursor = format_ident!("{name}Cursor");
    let unchecked_cursor = format_ident!("{name}UncheckedCursor");
    let encode_request = format_ident!("{name}EncodeRequest");
    let labels: Vec<_> = fields.iter().map(|field| field.name.to_string()).collect();
    let variants: Vec<_> = fields
        .iter()
        .map(|field| variant_name(&field.name.to_string()))
        .collect();
    let plans: Vec<_> = (0..fields.len())
        .map(|index| format_ident!("field_{index}"))
        .collect();
    let gaps: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            (field.position.is_some()
                || field.padding_before != 0
                || field.alignment_before.is_some())
            .then(|| format_ident!("gap_{index}"))
        })
        .collect();
    let gap_names: Vec<_> = gaps.iter().flatten().collect();
    let has_geometry = !gap_names.is_empty();
    let has_positions = fields.iter().any(|field| field.position.is_some());
    let has_computed = fields.iter().any(|field| field.computation.is_some());
    let has_builder = has_computed || controlled_by.iter().any(Option::is_some);
    let prepare_fields: Vec<_> = fields
        .iter()
        .enumerate()
        .filter(|(index, field)| field.computation.is_none() && controlled_by[*index].is_none())
        .map(|(_, field)| field)
        .collect();
    let prepare_field_names: Vec<_> = prepare_fields.iter().map(|field| &field.name).collect();
    let prepare_field_parameters: Vec<_> = prepare_fields
        .iter()
        .map(|field| {
            let field_name = &field.name;
            let ty = &field.ty;
            quote!(#field_name: #ty)
        })
        .collect();
    let prepare_destructure: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let field_name = &field.name;
            if field.computation.is_some() || controlled_by[index].is_some() {
                quote!(#field_name: _)
            } else {
                quote!(#field_name)
            }
        })
        .collect();
    let prepare_helper = format_ident!("__wire_repr_prepare_fields");

    let has_bytes = controlled_by.iter().any(Option::is_some) || has_computed;
    let has_nested = fields
        .iter()
        .any(|field| matches!(field.kind, FieldKind::Nested));
    let nested_view_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested)
                .then(|| super::generated_view_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_decode_error_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_decode_error_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_plan_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_plan_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_fields_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested)
                .then(|| super::generated_fields_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let nested_encode_error_paths: Vec<_> = fields
        .iter()
        .map(|field| {
            (matches!(field.kind, FieldKind::Nested) && field.operation_input.is_some())
                .then(|| super::generated_encode_error_path(&field.ty))
                .transpose()
        })
        .collect::<syn::Result<_>>()?;
    let (plan_decl_generics, plan_decl_where, plan_type, plan_impl_generics, plan_impl_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime, '__wire_repr_value>),
                quote!(where #lifetime: '__wire_repr_value),
                quote!(#plan<#lifetime, '__wire_repr_value>),
                quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
                quote!(#plan<#lifetime, '__wire_repr_value>),
            )
        } else {
            (
                quote!(<'__wire_repr_value>),
                quote!(),
                quote!(#plan<'__wire_repr_value>),
                quote!(<'__wire_repr_value>),
                quote!(#plan<'__wire_repr_value>),
            )
        };
    let (selection_impl_generics, selection_plan_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime: '__wire_repr_value, '__wire_repr_value>),
                quote!(#plan<#lifetime, '__wire_repr_value>),
            )
        } else {
            (
                quote!(<'__wire_repr_value>),
                quote!(#plan<'__wire_repr_value>),
            )
        };
    let (decode_error_decl_generics, error_impl_type, view_error_type, association_error_type) =
        if has_nested {
            (
                quote!(<'__wire_repr_wire>),
                quote!(#decode_error<'_>),
                quote!(#decode_error<'__wire_repr_wire>),
                quote!(#decode_error<'__wire_repr_view>),
            )
        } else {
            (
                quote!(),
                quote!(#decode_error),
                quote!(#decode_error),
                quote!(#decode_error),
            )
        };
    let (encode_error_decl_generics, encode_error_type, encode_error_impl_type) =
        if let Some(lifetime) = wire_lifetime.as_ref() {
            (
                quote!(<#lifetime>),
                quote!(#encode_error<#lifetime>),
                quote!(#encode_error<'_>),
            )
        } else {
            (quote!(), quote!(#encode_error), quote!(#encode_error))
        };
    let encode_lifetime_variant = wire_lifetime.as_ref().map(|lifetime| {
        quote! {
            #[doc(hidden)]
            __WireLifetime(
                ::core::convert::Infallible,
                ::core::marker::PhantomData<&#lifetime ()>,
            ),
        }
    });
    let encode_lifetime_arm = wire_lifetime
        .as_ref()
        .map(|_| quote!(Self::__WireLifetime(value, _) => match *value {},));
    let custom_validation_error = validation_error;
    let generated_validation_error = custom_validation_error.is_none() && has_nested;
    let validation_error = format_ident!("{name}ValidationError");
    let validation_error_type = if let Some(error) = custom_validation_error.as_ref() {
        quote!(#error)
    } else if generated_validation_error {
        quote!(#validation_error<'__wire_repr_wire>)
    } else {
        quote!(#view_error_type)
    };
    let validation_impl = {
        let field_validators = fields.iter().enumerate().flat_map(|(index, field)| {
            let name = &field.name;
            let own = field.validators.iter().map(move |validator| quote!(#validator(self.#name())?;));
            if matches!(field.kind, FieldKind::Nested) {
                let child_view = nested_view_paths[index].as_ref().expect("nested fields have generated view paths");
                let variant = &variants[index];
                let child = if generated_validation_error {
                    let nested_variant = format_ident!("Nested{variant}");
                    quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(#validation_error::#nested_variant)?;)
                } else {
                    quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::validate(&self.#name()).map_err(<Self::ValidationError as From<_>>::from)?;)
                };
                quote!(#child #(#own)*)
            } else { quote!(#(#own)*) }
        });
        quote! {
            impl<'__wire_repr_wire> #runtime::WireViewValidation<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type ValidationError = #validation_error_type;
                fn validate(&self) -> Result<(), Self::ValidationError> {
                    #(#field_validators)*
                    #(#model_validators(self)?;)*
                    Ok(())
                }
            }
        }
    };
    let generated_validation_error_decl = generated_validation_error.then(|| {
        let nested_variants = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let child_view = nested_view_paths[index].as_ref().expect("nested fields have generated view paths");
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(
                    #[doc = concat!("Nested semantic validation failed in field `", #label, "`.")]
                    #variant(<#child_view<'__wire_repr_wire> as #runtime::WireViewValidation<'__wire_repr_wire>>::ValidationError),
                )
            });
        let nested_arms = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| matches!(field.kind, FieldKind::Nested))
            .map(|(index, _)| {
                let variant = format_ident!("Nested{}", variants[index]);
                let label = &labels[index];
                quote!(Self::#variant(error) => write!(formatter, "nested validation failed in field `{}`: {error}", #label),)
            });
        quote! {
            /// Semantic validation failures for this wire representation.
            #[derive(Debug)]
            #vis enum #validation_error<'__wire_repr_wire> {
                /// Structural framing failed.
                Decode(#view_error_type),
                #(#nested_variants)*
            }
            impl ::core::fmt::Display for #validation_error<'_> {
                fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::Decode(error) => error.fmt(formatter),
                        #(#nested_arms)*
                    }
                }
            }
            impl ::core::error::Error for #validation_error<'_> {}
            impl<'__wire_repr_wire> From<#view_error_type> for #validation_error<'__wire_repr_wire> {
                fn from(error: #view_error_type) -> Self { Self::Decode(error) }
            }
        }
    });
    let view_request = quote!(#runtime::ValidatedViewRequest);
    let cursor_method = quote! {
        /// Returns a fail-closed cursor over consecutive representations.
        #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ValidatedViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
            #runtime::ValidatedViewCursor::new(input)
        }
    };
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = &wire_lifetime {
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

    let selection = selection::render(selection::Input {
        name: &name,
        vis: &vis,
        fields: &fields,
        plans: &plans,
        gaps: &gaps,
        nested_fields_paths: &nested_fields_paths,
        nested_view_paths: &nested_view_paths,
        nested_plan_paths: &nested_plan_paths,
        selection_impl_generics: &selection_impl_generics,
        selection_plan_type: &selection_plan_type,
        view: &view,
        runtime,
    });
    let selection::Output {
        field_proxy,
        declaration: selection_declaration,
    } = selection;
    let plan_output = plan::render(plan::Input {
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        fields: &fields,
        plans: &plans,
        gaps: &gaps,
        gap_names: &gap_names,
        nested_plan_paths: &nested_plan_paths,
        plan: &plan,
        plan_decl_generics: &plan_decl_generics,
        plan_decl_where: &plan_decl_where,
        plan_impl_generics: &plan_impl_generics,
        plan_impl_type: &plan_impl_type,
        field_proxy: &field_proxy,
        runtime,
    });
    let plan_declaration = plan_output.declaration;
    let plan_lifetime_init = plan_output.lifetime_init;
    let view_fields: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            match field.kind {
                FieldKind::Nested => {
                    let child_view = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    quote!(#stored: #child_view<'__wire_repr_wire>)
                }
                FieldKind::Fixed(_)
                | FieldKind::Prefix(_)
                | FieldKind::Bytes { .. }
                | FieldKind::Rest => {
                    quote!(#stored: &'__wire_repr_wire [u8])
                }
            }
        })
        .collect();
    let view_initializers: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let stored = format_ident!("field_{index}");
            let name = &field.name;
            match field.kind {
                FieldKind::Fixed(_) => {
                    let raw = format_ident!("raw_{index}");
                    quote!(#stored: #raw)
                }
                FieldKind::Prefix(_) => {
                    let raw = format_ident!("raw_{index}");
                    quote!(#stored: #raw)
                }
                FieldKind::Nested | FieldKind::Bytes { .. } | FieldKind::Rest => {
                    quote!(#stored: #name)
                }
            }
        })
        .collect();
    let decode_steps: Vec<_> = fields
        .iter()
        .enumerate()
        .zip(&labels)
        .zip(&variants)
        .map(|(((index, field), label), variant)| {
            let field_name = &field.name;
            let raw = format_ident!("raw_{index}");
            let geometry = if let Some(position) = &field.position {
                let (position, conversion) = match position {
                    FieldPosition::Static(position) => (quote!(#position), quote!()),
                    FieldPosition::Source(source) => (
                        quote!(position),
                        quote! {
                            let position = usize::try_from(#source).map_err(|_| {
                                #decode_error::PositionNotRepresentable {
                                    field: #label,
                                    value: #source as u128,
                                }
                            })?;
                        },
                    ),
                };
                quote! {
                    #conversion
                    let represented = input.len() - remaining.len();
                    if #position < represented {
                        return Err(#decode_error::PositionBeforeCursor {
                            field: #label,
                            position: #position,
                            cursor: represented,
                        });
                    }
                    let gap = #position - represented;
                    let available = remaining.len();
                    let Some((_, suffix)) = remaining.split_at_checked(gap) else {
                        return Err(#decode_error::InputTooShort {
                            field: #label,
                            required: gap,
                            available,
                        });
                    };
                    remaining = suffix;
                }
            } else if field.padding_before == 0 && field.alignment_before.is_none() {
                quote!()
            } else {
                let padding = field.padding_before;
                let alignment = match field.alignment_before {
                    Some(boundary) => quote!(Some(#boundary)),
                    None => quote!(None::<usize>),
                };
                quote! {
                    let represented = input.len() - remaining.len();
                    let padded = represented.checked_add(#padding).ok_or(
                        #decode_error::GeometryOverflow { field: #label },
                    )?;
                    let alignment_padding = match #alignment {
                        Some(boundary) => {
                            let remainder = padded % boundary;
                            if remainder == 0 { 0 } else { boundary - remainder }
                        }
                        None => 0,
                    };
                    let gap = #padding.checked_add(alignment_padding).ok_or(
                        #decode_error::GeometryOverflow { field: #label },
                    )?;
                    let available = remaining.len();
                    let Some((_, suffix)) = remaining.split_at_checked(gap) else {
                        return Err(#decode_error::InputTooShort {
                            field: #label,
                            required: gap,
                            available,
                        });
                    };
                    remaining = suffix;
                }
            };
            let decode_source = position_sources[index]
                .then(|| {
                    let codec = match &field.kind {
                        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
                        _ => unreachable!(),
                    };
                    quote!(let #field_name = <#codec as #runtime::FixedCodec>::decode(#raw);)
                })
                .or_else(|| {
                    controlled_by[index].and_then(|_| match &field.kind {
                        FieldKind::Fixed(codec) => {
                            let codec = codec_tokens(codec, runtime);
                            Some(quote!(let #field_name = <#codec as #runtime::FixedCodec>::decode(#raw);))
                        }
                        FieldKind::Prefix(_) => None,
                        _ => unreachable!(),
                    })
                });
            let decode = match &field.kind {
                FieldKind::Fixed(codec) => {
                    let codec = codec_tokens(codec, runtime);
                    quote! {
                        let width = <#codec as #runtime::FixedCodec>::WIDTH;
                        let available = remaining.len();
                        let Some((#raw, suffix)) = remaining.split_at_checked(width) else {
                            return Err(#decode_error::InputTooShort { field: #label, required: width, available });
                        };
                        #decode_source
                        remaining = suffix;
                    }
                }
                FieldKind::Nested => {
                    let child_view = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    if let Some(operation) = &field.operation_input {
                        let parse = format_ident!("__wire_repr_parse_with_{operation}");
                        quote! {
                            let (#field_name, suffix) = #child_view::#parse(remaining, operation)
                                .map_err(#decode_error::#variant)?;
                            remaining = suffix;
                        }
                    } else {
                        quote! {
                            let (#field_name, suffix) = <#child_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::parse_view(remaining)
                                .map_err(#decode_error::#variant)?;
                            remaining = suffix;
                        }
                    }
                }
                FieldKind::Prefix(codec) => {
                    let decode_source = controlled_by[index].is_some().then(|| quote! {
                        let #field_name = <#codec as #runtime::PrefixCodec>::decode(#raw);
                    });
                    quote! {
                        let extent = <#codec as #runtime::PrefixCodec>::validate_prefix(remaining)
                            .map_err(#decode_error::#variant)?;
                        let required = extent.encoded_len().get();
                        let available = remaining.len();
                        let Some((#raw, suffix)) = extent.split_input(remaining) else {
                            return Err(#decode_error::InputTooShort {
                                field: #label,
                                required,
                                available,
                            });
                        };
                        #decode_source
                        remaining = suffix;
                    }
                },
                FieldKind::Bytes { source, .. } => quote! {
                    let required = usize::try_from(#source).map_err(|_| {
                        #decode_error::LengthNotRepresentable {
                            field: #label,
                        }
                    })?;
                    let available = remaining.len();
                    let Some((#field_name, suffix)) = remaining.split_at_checked(required) else {
                        return Err(#decode_error::InputTooShort {
                            field: #label,
                            required,
                            available,
                        });
                    };
                    remaining = suffix;
                },
                FieldKind::Rest => quote! {
                    let #field_name = remaining;
                    remaining = &[];
                },
            };
            quote! { #geometry #decode }
        })
        .collect();
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let stored = format_ident!("field_{index}");
        let (return_type, value) = match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec_tokens = codec_tokens(codec, runtime);
                match codec {
                    Codec::OwnedBytes(length) => (quote!(&'__wire_repr_wire [u8; #length]), quote!(match <&'__wire_repr_wire [u8; #length]>::try_from(self.#stored) { Ok(bytes) => bytes, Err(_) => unreachable!("validated fixed byte array has its declared width"), })),
                    _ if field.computation.is_some() => {
                        let value_ty = &field.computation.as_ref().expect("checked").value_ty;
                        (quote!(#value_ty), quote!(<#codec_tokens as #runtime::FixedCodec>::decode(self.#stored)))
                    }
                    _ => (quote!(<#codec_tokens as #runtime::FixedCodec>::Value<'__wire_repr_wire>), quote!(<#codec_tokens as #runtime::FixedCodec>::decode(self.#stored))),
                }
            }
            FieldKind::Nested => {
                let child_view = nested_view_paths[index]
                    .as_ref()
                    .expect("nested fields have generated view paths");
                (quote!(#child_view<'__wire_repr_wire>), quote!(self.#stored))
            }
            FieldKind::Prefix(codec) => (quote!(<#codec as #runtime::PrefixCodec>::Value<'__wire_repr_wire>), quote!(<#codec as #runtime::PrefixCodec>::decode(self.#stored))),
            FieldKind::Bytes { .. } | FieldKind::Rest => (quote!(&'__wire_repr_wire [u8]), quote!(self.#stored)),
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            #vis fn #field_name(&self) -> #return_type { #value }
        }
    });
    let decode_variants =
        fields
            .iter()
            .zip(&variants)
            .zip(&labels)
            .enumerate()
            .filter_map(|(index, ((field, variant), label))| match &field.kind {
                FieldKind::Nested => {
                    let child_view = nested_view_paths[index]
                        .as_ref()
                        .expect("nested fields have generated view paths");
                    let error = if field.operation_input.is_some() {
                        let error = nested_decode_error_paths[index]
                            .as_ref()
                            .expect("operation-backed nested fields have generated error paths");
                        quote!(#error)
                    } else {
                        quote!(<#child_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::DecodeError)
                    };
                    Some(quote!(
                        #[doc = concat!("Nested decode error for field `", #label, "`.")]
                        #variant(#error),
                    ))
                }
                FieldKind::Prefix(codec) => Some(quote!(
                    #[doc = concat!("Prefix validation error for field `", #label, "`.")]
                    #variant(<#codec as #runtime::PrefixCodec>::DecodeError),
                )),
                FieldKind::Fixed(_) | FieldKind::Bytes { .. } | FieldKind::Rest => None,
            });
    let decode_display_arms = fields
        .iter()
        .zip(&variants)
        .zip(&labels)
        .filter_map(|((field, variant), label)| match field.kind {
            FieldKind::Nested => Some(quote!(Self::#variant(error) => write!(formatter, "wire decode failed in field `{}`: {error}", #label),)),
            FieldKind::Prefix(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire prefix validation failed in field `{}`: {error:?}", #label),)),
            FieldKind::Fixed(_) | FieldKind::Bytes { .. } | FieldKind::Rest => None,
        });
    let encode_variants = fields.iter().zip(&variants).zip(&labels).enumerate().filter_map(
        |(index, ((field, variant), label))| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                Some(quote!(#[doc = concat!("Preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::FixedCodec>::EncodeError),))
            }
            FieldKind::Nested => {
                let ty = &field.ty;
                if field.operation_input.is_some() {
                    let error = nested_encode_error_paths[index]
                        .as_ref()
                        .expect("operation-backed nested fields have generated error paths");
                    Some(quote!(#[doc = concat!("Nested preparation error for field `", #label, "`.")] #variant(#error),))
                } else {
                    Some(quote!(#[doc = concat!("Nested preparation error for field `", #label, "`.")] #variant(<#ty as #runtime::WireEncode>::EncodeError),))
                }
            }
            FieldKind::Prefix(codec) => Some(quote!(#[doc = concat!("Prefix preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::PrefixCodec>::EncodeError),)),
            FieldKind::Bytes { .. } | FieldKind::Rest => None,
        },
    );
    let encode_display_arms = fields
        .iter()
        .zip(&variants)
        .zip(&labels)
        .filter_map(|((field, variant), label)| match field.kind {
            FieldKind::Fixed(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error:?}", #label),)),
            FieldKind::Nested => Some(quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error}", #label),)),
            FieldKind::Prefix(_) => Some(quote!(Self::#variant(error) => write!(formatter, "wire prefix preparation failed for field `{}`: {error:?}", #label),)),
            FieldKind::Bytes { .. } | FieldKind::Rest => None,
        });
    let prepare_steps: Vec<_> = fields
        .iter()
        .zip(&plans)
        .zip(&variants)
        .zip(&controlled_by)
        .filter_map(|(((field, plan), variant), controlled_by)| {
            if field.computation.is_some() {
                return None;
            }
            let field_name = &field.name;
            Some(match (&field.kind, controlled_by) {
                (FieldKind::Fixed(codec), Some(bytes_index)) => {
                    let codec = codec_tokens(codec, runtime);
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: #bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::FixedCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Fixed(codec), None) => {
                    let codec = codec_tokens(codec, runtime);
                    quote!(let #plan = <#codec as #runtime::FixedCodec>::plan(#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Nested, _) => {
                    let ty = &field.ty;
                    if field.operation_input.is_some() {
                        quote!(let #plan = <#ty>::#operation_prepare(#field_name, #operation_value).map_err(#encode_error::#variant)?;)
                    } else {
                        quote!(let #plan = <#ty as #runtime::WireEncode>::prepare(#field_name).map_err(#encode_error::#variant)?;)
                    }
                }
                (FieldKind::Prefix(codec), Some(bytes_index)) => {
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: #bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::PrefixCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Prefix(codec), None) => {
                    quote!(let #plan = <#codec as #runtime::PrefixCodec>::plan(#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Bytes { .. } | FieldKind::Rest, _) => {
                    quote!(let #plan = #field_name;)
                }
            })
        })
        .collect();
    let computation_steps: Vec<_> = computation_order
        .iter()
        .map(|&index| {
            let field = &fields[index];
            let computation = field.computation.as_ref().expect("computed order");
            let FieldKind::Fixed(codec) = &field.kind else {
                unreachable!("computed fields are fixed codecs")
            };
            let codec = codec_tokens(codec, runtime);
            let plan = &plans[index];
            let variant = &variants[index];
            let source_ty = &computation.value_ty;
            let field_name = &field.name;
            let field_label = field.name.to_string();
            let callback = &computation.callback;
            let mut callback_preparation = Vec::new();
            let callback_arguments: Vec<_> = computation
                .arguments
                .iter()
                .enumerate()
                .map(|(argument_index, argument)| match argument {
                    ComputationArgument::Semantic { index, .. } => {
                        let name = &fields[*index].name;
                        quote!(&#name)
                    }
                    ComputationArgument::Bytes(selection) => {
                        let bytes_name = format_ident!(
                            "__wire_repr_computed_bytes_{index}_{argument_index}"
                        );
                        let source = computation_source(selection, index, &fields, &plans, &gaps, runtime);
                        callback_preparation.push(quote!(let #bytes_name = #source;));
                        quote!(&#bytes_name)
                    }
                })
                .collect();
            let requires_geometry = computation.requires_geometry;
            let step = quote! {
                #(#callback_preparation)*
                let #field_name = <#source_ty>::try_from(#callback(#(#callback_arguments),*)).map_err(|_| {
                    #encode_error::ComputedValueNotRepresentable {
                        field: #field_label,
                    }
                })?;
                let #plan = <#codec as #runtime::FixedCodec>::plan(#field_name)
                    .map_err(#encode_error::#variant)?;
            };
            (requires_geometry, step)
        })
        .collect();
    let early_computation_steps: Vec<_> = computation_steps
        .iter()
        .filter_map(|(needs_geometry, step)| (!needs_geometry).then_some(step))
        .collect();
    let computation_steps: Vec<_> = computation_steps
        .iter()
        .filter_map(|(needs_geometry, step)| needs_geometry.then_some(step))
        .collect();
    let early_computation_steps = &early_computation_steps;
    let computation_steps = &computation_steps;
    let geometry_steps: Vec<_> = fields
        .iter()
        .zip(&plans)
        .zip(&gaps)
        .map(|((field, plan), gap)| {
            let length = if field.computation.is_some() {
                let FieldKind::Fixed(codec) = &field.kind else {
                    unreachable!("computed fields are fixed codecs")
                };
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            } else {
                match field.kind {
                FieldKind::Fixed(_) | FieldKind::Prefix(_) | FieldKind::Bytes { .. } | FieldKind::Rest => {
                    quote!(#runtime::ByteSource::byte_len(&#plan))
                }
                FieldKind::Nested => {
                    quote!(#runtime::ByteSource::byte_len(&#plan))
                }
                }
            };
            if let (Some(gap), Some(position)) = (gap, &field.position) {
                let label = field.name.to_string();
                let field_start = match position {
                    FieldPosition::Static(position) => quote!(#position),
                    FieldPosition::Source(source) => quote! {
                        usize::try_from(#source).map_err(|_| {
                            #encode_error::PositionNotRepresentable {
                                field: #label,
                                value: #source as u128,
                            }
                        })?
                    },
                };
                quote! {
                    let field_start = #field_start;
                    if field_start < encoded_len {
                        return Err(#encode_error::PositionBeforeCursor {
                            field: #label,
                            position: field_start,
                            cursor: encoded_len,
                        });
                    }
                    let #gap = field_start - encoded_len;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else if let Some(gap) = gap {
                let padding = field.padding_before;
                let alignment = match field.alignment_before {
                    Some(boundary) => quote!(Some(#boundary)),
                    None => quote!(None::<usize>),
                };
                quote! {
                    let before_gap = encoded_len;
                    let padded = encoded_len.checked_add(#padding).ok_or(#encode_error::LengthOverflow)?;
                    let alignment_padding = match #alignment {
                        Some(boundary) => {
                            let remainder = padded % boundary;
                            if remainder == 0 { 0 } else { boundary - remainder }
                        }
                        None => 0,
                    };
                    let field_start = padded
                        .checked_add(alignment_padding)
                        .ok_or(#encode_error::LengthOverflow)?;
                    let #gap = field_start - before_gap;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else {
                quote! {
                    encoded_len = encoded_len
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            }
        })
        .collect();
    let decode_position_variants = has_positions.then(|| {
        quote! {
            /// A decoded position does not fit this platform's address space.
            PositionNotRepresentable {
                /// The positioned field.
                field: &'static str,
                /// The source value that does not fit.
                value: u128,
            },
            /// A field position points before the current cursor.
            PositionBeforeCursor {
                /// The positioned field.
                field: &'static str,
                /// The requested absolute position.
                position: usize,
                /// The current representation cursor.
                cursor: usize,
            },
        }
    });
    let decode_position_arms = has_positions.then(|| {
        quote! {
            Self::PositionNotRepresentable { field, value } => {
                write!(formatter, "position {value} for field `{field}` does not fit in usize")
            }
            Self::PositionBeforeCursor { field, position, cursor } => {
                write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}")
            }
        }
    });
    let encode_position_variants = has_positions.then(|| {
        quote! {
            /// A requested position does not fit this platform's address space.
            PositionNotRepresentable {
                /// The positioned field.
                field: &'static str,
                /// The source value that does not fit.
                value: u128,
            },
            /// A requested position points before the current cursor.
            PositionBeforeCursor {
                /// The positioned field.
                field: &'static str,
                /// The requested absolute position.
                position: usize,
                /// The current representation cursor.
                cursor: usize,
            },
        }
    });
    let encode_position_arms = has_positions.then(|| {
        quote! {
            Self::PositionNotRepresentable { field, value } => {
                write!(formatter, "position {value} for field `{field}` does not fit in usize")
            }
            Self::PositionBeforeCursor { field, position, cursor } => {
                write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}")
            }
        }
    });
    let decode_geometry_variant = has_geometry.then(|| {
        quote! {
            /// The requested padding or alignment overflowed `usize`.
            GeometryOverflow {
                /// The field whose placement overflowed.
                field: &'static str,
            },
        }
    });
    let decode_geometry_arm = has_geometry.then(|| {
        quote! {
            Self::GeometryOverflow { field } => {
                write!(formatter, "placement before field `{field}` does not fit in usize")
            }
        }
    });
    let decode_length_variant = has_bytes.then(|| {
        quote! {
            /// A decoded byte length does not fit this platform's address space.
            LengthNotRepresentable {
                /// The byte field controlled by the source value.
                field: &'static str,
            },
        }
    });
    let decode_length_arm = has_bytes.then(|| {
        quote! {
            Self::LengthNotRepresentable { field } => {
                write!(formatter, "byte length for field `{field}` does not fit in usize")
            }
        }
    });
    let encode_missing_variant = has_builder.then(|| {
        quote! {
            /// A caller-owned builder field was not supplied.
            MissingField {
                /// The omitted field.
                field: &'static str,
            },
        }
    });
    let encode_missing_arm = has_builder.then(|| {
        quote! {
            Self::MissingField { field } => write!(formatter, "missing required field `{field}`"),
        }
    });
    let encode_computed_value_variant = has_computed.then(|| {
        quote! {
            /// A computed value does not fit the destination field's semantic type.
            ComputedValueNotRepresentable {
                /// The computed destination field.
                field: &'static str,
            },
        }
    });
    let encode_computed_value_arm = has_computed.then(|| {
        quote! {
            Self::ComputedValueNotRepresentable { field } => {
                write!(formatter, "computed value does not fit field `{field}`")
            }
        }
    });
    let encode_length_variant = has_bytes.then(|| {
        quote! {
            /// A byte field's length is not representable by its source field.
            LengthNotRepresentable {
                /// The byte field whose length could not be represented.
                field: &'static str,
                /// The physical source field that stores the length.
                source: &'static str,
                /// The requested byte count.
                length: usize,
            },
        }
    });
    let encode_length_arm = has_bytes.then(|| quote! {
        Self::LengthNotRepresentable { field, source, length } => {
            write!(formatter, "field `{field}` has {length} bytes, which do not fit in length field `{source}`")
        }
    });
    let preparation_body = quote! {
        #(#prepare_steps)*
        #(#early_computation_steps)*
        let mut encoded_len = 0usize;
        #(#geometry_steps)*
        #(#computation_steps)*
        Ok(#plan {
            #(#plans,)*
            #(#gap_names,)*
            #plan_lifetime_init
            encoded_len,
        })
    };

    let operation_view_helper = if operation_input_ty.is_some() {
        quote! {
            #[doc(hidden)]
            #vis fn #operation_parse(
                input: &'__wire_repr_wire [u8],
                operation: &#operation_input_ty,
            ) -> Result<(Self, &'__wire_repr_wire [u8]), #view_error_type> {
                let mut remaining = input;
                #(#decode_steps)*
                let represented = &input[..input.len() - remaining.len()];
                Ok((Self { bytes: represented, #(#view_initializers,)* }, remaining))
            }
        }
    } else {
        quote!()
    };
    let view_impl = if operation_input_ty.is_some() {
        quote!()
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type DecodeError = #view_error_type;
                fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let mut remaining = input;
                    #(#decode_steps)*
                    let represented = &input[..input.len() - remaining.len()];
                    Ok((Self { bytes: represented, #(#view_initializers,)* }, remaining))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError { #decode_error::TrailingBytes { expected: represented, actual: input } }
                fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes }
            }
        }
    };

    let builder = builder::render(builder::Input {
        has_builder,
        name: &name,
        vis: &vis,
        wire_lifetime: wire_lifetime.as_ref(),
        operation_input_ty,
        operation_name,
        prepare_fields: &prepare_fields,
        prepare_field_names: &prepare_field_names,
        plan: &plan,
        encode_error: &encode_error,
        self_type: &self_type,
        prepare_helper: &prepare_helper,
        runtime,
    });
    let builder_declaration = builder.declaration;
    let builder_method = builder.method;

    let inherent_impl = if operation_input_ty.is_some() {
        let operation_name = operation_name.expect("operation input");
        let operation_encode_generics = if let Some(lifetime) = &wire_lifetime {
            quote!(<#lifetime, '__wire_repr_operation>)
        } else {
            quote!(<'__wire_repr_operation>)
        };
        let operation_encode_request_type = if let Some(lifetime) = &wire_lifetime {
            quote!(#encode_request<#lifetime, '_>)
        } else {
            quote!(#encode_request<'_>)
        };
        quote! {
            #[doc(hidden)] #vis struct #view_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)] #vis struct #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { input: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #cursor_input_request<'__wire_repr_wire> { input: &'__wire_repr_wire [u8] }
            #[doc(hidden)] #vis struct #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { remaining: &'__wire_repr_wire [u8], operation: &'__wire_repr_operation #operation_input_ty }
            #[doc(hidden)] #vis struct #encode_request #operation_encode_generics { value: #self_type, operation: &'__wire_repr_operation #operation_input_ty }

            #[allow(missing_docs)] impl<'__wire_repr_wire> #view_input_request<'__wire_repr_wire> {
                #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #direct_view_request<'__wire_repr_wire, '_> { #direct_view_request { input: self.input, operation } }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis fn unchecked(self) -> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_view_request { input: self.input, operation: self.operation } }
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #validation_error_type> { let (view, remainder) = #view::#operation_parse(self.input, self.operation)?; #runtime::WireViewValidation::validate(&view)?; Ok((view, remainder)) }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #validation_error_type> { let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?; if suffix.is_empty() { Ok(view) } else { Err(<#validation_error_type as From<#view_error_type>>::from(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len })) } }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_view_request<'__wire_repr_wire, '__wire_repr_operation> {
                #vis fn with_remainder(self) -> Result<(#view<'__wire_repr_wire>, &'__wire_repr_wire [u8]), #view_error_type> { #view::#operation_parse(self.input, self.operation) }
                #vis fn without_trailing(self) -> Result<#view<'__wire_repr_wire>, #view_error_type> { let input_len = self.input.len(); let (view, suffix) = self.with_remainder()?; if suffix.is_empty() { Ok(view) } else { Err(#decode_error::TrailingBytes { expected: view.as_bytes().len(), actual: input_len }) } }
            }
            #[allow(missing_docs)] impl<'__wire_repr_wire> #cursor_input_request<'__wire_repr_wire> { #[must_use] #vis fn #operation_name(self, operation: &#operation_input_ty) -> #direct_cursor<'__wire_repr_wire, '_> { #direct_cursor { remaining: self.input, operation } } }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #direct_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #[must_use] #vis fn unchecked(self) -> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> { #unchecked_cursor { remaining: self.remaining, operation: self.operation } }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#validation_error_type>> { if self.remaining.is_empty() { return Ok(None); } let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(|error| #runtime::ViewCursorError::Item(error.into()))?; if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); } #runtime::WireViewValidation::validate(&view).map_err(#runtime::ViewCursorError::Item)?; self.remaining = suffix; Ok(Some(view)) }
            }
            #[allow(missing_docs)]
            impl<'__wire_repr_wire, '__wire_repr_operation> #unchecked_cursor<'__wire_repr_wire, '__wire_repr_operation> {
                #[must_use] #vis const fn remaining(&self) -> &'__wire_repr_wire [u8] { self.remaining }
                #vis fn next(&mut self) -> Result<Option<#view<'__wire_repr_wire>>, #runtime::ViewCursorError<#view_error_type>> { if self.remaining.is_empty() { return Ok(None); } let (view, suffix) = #view::#operation_parse(self.remaining, self.operation).map_err(#runtime::ViewCursorError::Item)?; if suffix.len() == self.remaining.len() { return Err(#runtime::ViewCursorError::EmptyItem); } self.remaining = suffix; Ok(Some(view)) }
            }
            #[allow(missing_docs)] impl #operation_encode_generics #encode_request #operation_encode_generics { #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where #self_type: '__wire_repr_value { <#self_type>::#operation_prepare(self.value, self.operation) } #vis fn build_into<'__wire_repr_value, '__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> where #self_type: '__wire_repr_value { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) } }
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #builder_method
                #vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #view_input_request<'__wire_repr_view> { #view_input_request { input } }
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #cursor_input_request<'__wire_repr_view> { #cursor_input_request { input } }
                #vis fn #operation_name(self, operation: &#operation_input_ty) -> #operation_encode_request_type { #encode_request { value: self, operation } }
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
                #view_signature { #view_request::new(input) }
                #cursor_method
                #builder_method
                #[doc(hidden)]
                fn #prepare_helper<'__wire_repr_value>(#(#prepare_field_parameters),*) -> Result<#plan_type, #encode_error_type>
                where Self: '__wire_repr_value {
                    #preparation_body
                }
                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where Self: '__wire_repr_value { <Self as #runtime::WireEncode>::prepare(self) }
                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) }
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
                fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value {
                    let Self { #(#prepare_destructure,)* } = self;
                    Self::#prepare_helper(#(#prepare_field_names),*)
                }
            }
        }
    };

    Ok(quote! {
        /// Typed decoding failures for this wire representation.
        #[derive(Debug)]
        #vis enum #decode_error #decode_error_decl_generics {
            /// A fixed-width field did not fit in the remaining input.
            InputTooShort {
                /// The source field that did not fit.
                field: &'static str,
                /// The required byte count.
                required: usize,
                /// The available byte count.
                available: usize,
            },
            #decode_position_variants
            #decode_geometry_variant
            #decode_length_variant
            #(#decode_variants)*
            /// Input had bytes after the complete representation.
            TrailingBytes {
                /// The represented byte count.
                expected: usize,
                /// The supplied byte count.
                actual: usize,
            },
        }
        impl ::core::fmt::Display for #error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { field, required, available } => {
                        let required_unit = if *required == 1 { "byte" } else { "bytes" };
                        let available_unit = if *available == 1 { "byte" } else { "bytes" };
                        let available_verb = if *available == 1 { "remains" } else { "remain" };
                        write!(formatter, "field `{field}` needs {required} {required_unit}, but only {available} {available_unit} {available_verb}")
                    }
                    #decode_position_arms
                    #decode_geometry_arm
                    #decode_length_arm
                    #(#decode_display_arms)*
                    Self::TrailingBytes { expected, actual } => {
                        let trailing = actual.saturating_sub(*expected);
                        let unit = if trailing == 1 { "byte" } else { "bytes" };
                        write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation")
                    }
                }
            }
        }
        impl ::core::error::Error for #error_impl_type {}

        /// Typed encoding-preparation failures for this wire representation.
        #[derive(Debug)]
        #vis enum #encode_error #encode_error_decl_generics {
            #encode_lifetime_variant
            #encode_position_variants
            #encode_missing_variant
            #encode_computed_value_variant
            #encode_length_variant
            #(#encode_variants)*
            /// The encoded length overflowed `usize`.
            LengthOverflow,
        }
        impl ::core::fmt::Display for #encode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #encode_lifetime_arm
                    #encode_position_arms
                    #encode_missing_arm
                    #encode_computed_value_arm
                    #encode_length_arm
                    #(#encode_display_arms)*
                    Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"),
                }
            }
        }
        impl ::core::error::Error for #encode_error_impl_type {}

        /// A bytes-backed validated read view for this wire representation.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> { bytes: &'__wire_repr_wire [u8], #(#view_fields,)* }
        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            /// Returns this view's exact represented bytes.
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes }
            /// Returns a byte-selection root for this exact source representation.
            #[must_use]
            #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }
            #(#getters)*
            #operation_view_helper
        }
        #view_impl
        impl<'__wire_repr_wire> #runtime::ByteSource for #view<'__wire_repr_wire> {
            #[inline(always)]
            fn byte_len(&self) -> usize { self.as_bytes().len() }
            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) { sink.write(self.as_bytes()); }
        }
        impl<'__wire_repr_wire> #runtime::ByteSourceCursor for #view<'__wire_repr_wire> {
            type Segments<'__wire_repr_source> = ::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>> where Self: '__wire_repr_source;
            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                ::core::iter::once(#runtime::ByteSegment::Bytes(self.as_bytes()))
            }
        }
        #generated_validation_error_decl
        #validation_impl

        #selection_declaration
        #plan_declaration

        #builder_declaration
        #inherent_impl
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
        #encode_impl

    })
}

fn computation_source(
    selection: &ComputationByteSelection,
    own_index: usize,
    fields: &[Field],
    plans: &[proc_macro2::Ident],
    gaps: &[Option<proc_macro2::Ident>],
    runtime: &TokenStream,
) -> TokenStream {
    let mut components = Vec::new();
    match selection {
        ComputationByteSelection::Exclude(paths) => {
            for (index, plan) in plans.iter().enumerate() {
                if let Some(gap) = &gaps[index] {
                    components.push(quote!(#runtime::ByteSegment::Rest { byte: 0, len: #gap }));
                }
                if index == own_index {
                    continue;
                }
                let selected: Vec<_> = paths
                    .iter()
                    .filter(|path| path.top_level_index == index)
                    .collect();
                if selected.iter().any(|path| path.nested.is_empty()) {
                    continue;
                }
                if selected.is_empty() {
                    components.push(quote!(#runtime::__private::BorrowedSource::new(&#plan)));
                } else {
                    debug_assert!(matches!(fields[index].kind, FieldKind::Nested));
                    let selector = selected
                        .iter()
                        .map(|path| {
                            let nested = &path.nested;
                            quote!(fields #(.#nested)*)
                        })
                        .reduce(|left, right| quote!(#left | #right))
                        .expect("nonempty nested selection");
                    components.push(quote!(#plan.bytes().exclude(|fields| #selector)));
                }
            }
        }
        ComputationByteSelection::Include(paths) => {
            for (index, plan) in plans.iter().enumerate() {
                let selected: Vec<_> = paths
                    .iter()
                    .filter(|path| path.top_level_index == index)
                    .collect();
                if selected.is_empty() {
                    continue;
                }
                if selected.iter().any(|path| path.nested.is_empty()) {
                    components.push(quote!(#runtime::__private::BorrowedSource::new(&#plan)));
                    continue;
                }
                debug_assert!(matches!(fields[index].kind, FieldKind::Nested));
                let selector = selected
                    .iter()
                    .map(|path| {
                        let nested = &path.nested;
                        quote!(fields #(.#nested)*)
                    })
                    .reduce(|left, right| quote!(#left | #right))
                    .expect("nonempty nested selection");
                components.push(quote!(#plan.bytes().include(|fields| #selector)));
            }
        }
    }
    let _ = own_index;
    chain_sources(components, runtime)
}

fn chain_sources(mut sources: Vec<TokenStream>, runtime: &TokenStream) -> TokenStream {
    let Some(first) = sources.first().cloned() else {
        return quote!(#runtime::__private::EmptySource);
    };
    sources.drain(1..).fold(
        first,
        |left, right| quote!(#runtime::__private::ByteChain::new(#left, #right)),
    )
}

fn codec_tokens(codec: &Codec, runtime: &TokenStream) -> TokenStream {
    match codec {
        Codec::Builtin(name) => {
            let name = format_ident!("{name}");
            quote!(#runtime::#name)
        }
        Codec::OwnedBytes(length) => quote!(#runtime::__private::OwnedBytes<#length>),
        Codec::Custom(path) => quote!(#path),
    }
}
fn variant_name(name: &str) -> proc_macro2::Ident {
    let value: String = name
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    format_ident!("{value}")
}
