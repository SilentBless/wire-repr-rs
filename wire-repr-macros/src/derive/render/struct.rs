//! Struct derive rendering.

use super::super::model::{Codec, FieldKind, FieldPosition, WireStruct};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::visit::Visit;

pub(super) fn render(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    if model.opcodes.is_none()
        && model.fields.iter().all(|field| {
            matches!(field.kind, FieldKind::Fixed(_))
                && !matches!(field.position, Some(FieldPosition::Source(_)))
        })
    {
        return render_fixed_view(model, runtime);
    }

    let WireStruct {
        vis,
        name,
        wire_lifetime,
        opcodes,
        fields,
    } = model;
    let plan = format_ident!("{name}Plan");
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let opcode_table = opcodes.as_ref();
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
    let controlled_by: Vec<_> = (0..fields.len())
        .map(|source_index| {
            fields.iter().position(|field| {
                matches!(
                    field.kind,
                    FieldKind::Bytes {
                        source_index: candidate,
                        ..
                    } if candidate == source_index
                )
            })
        })
        .collect();
    let has_bytes = controlled_by.iter().any(Option::is_some);
    let position_sources: Vec<_> = fields
        .iter()
        .map(|source| {
            fields.iter().any(|field| {
                matches!(field.position, Some(FieldPosition::Source(ref name)) if name == &source.name)
            })
        })
        .collect();
    let nested_wire_lifetime = wire_lifetime.as_ref().filter(|lifetime| {
        fields.iter().any(|field| {
            matches!(field.kind, FieldKind::Nested) && type_uses_lifetime(&field.ty, lifetime)
        })
    });
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
    let (plan_decl_generics, plan_decl_where, plan_type, plan_impl_generics, plan_impl_type) =
        if let Some(lifetime) = nested_wire_lifetime {
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
                quote!(),
                quote!(#plan<'_>),
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
        if let Some(lifetime) = nested_wire_lifetime {
            (
                quote!(<#lifetime>),
                quote!(#encode_error<#lifetime>),
                quote!(#encode_error<'_>),
            )
        } else {
            (quote!(), quote!(#encode_error), quote!(#encode_error))
        };
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ViewRequest<'__wire_repr_view, #view<'__wire_repr_view>>),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #runtime::ViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>>),
        )
    };

    let plan_fields = fields
        .iter()
        .zip(&plans)
        .map(|(field, plan)| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(#plan: <#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>)
            }
            FieldKind::Nested => {
                let ty = &field.ty;
                if field.uses_opcodes {
                    let opcode_table = opcode_table.expect("validated struct opcode input");
                    quote!(#plan: <#ty as #runtime::WireEncodeWithOpcodes<#opcode_table>>::Plan<'__wire_repr_value>)
                } else {
                    quote!(#plan: <#ty as #runtime::WireEncode>::Plan<'__wire_repr_value>)
                }
            }
            FieldKind::Prefix(codec) => {
                quote!(#plan: <#codec as #runtime::PrefixCodec>::Plan<'__wire_repr_value>)
            }
            FieldKind::Bytes { .. } | FieldKind::Rest => quote!(#plan: &'__wire_repr_value [u8]),
        });
    let gap_fields = gap_names.iter().map(|gap| quote!(#gap: usize));
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
    let decode_steps = fields
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
                    if field.uses_opcodes {
                        let opcode_table = opcode_table.expect("validated struct opcode input");
                        quote! {
                            let (#field_name, suffix) = <#child_view<'__wire_repr_wire> as #runtime::WireViewWithOpcodes<'__wire_repr_wire, #opcode_table>>::parse_view_with_opcodes(remaining, opcodes)
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
                            value: #source as u128,
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
        });
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let stored = format_ident!("field_{index}");
        let (return_type, value) = match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec_tokens = codec_tokens(codec, runtime);
                match codec {
                    Codec::OwnedBytes(length) => (quote!(&'__wire_repr_wire [u8; #length]), quote!(match <&'__wire_repr_wire [u8; #length]>::try_from(self.#stored) { Ok(bytes) => bytes, Err(_) => unreachable!("validated fixed byte array has its declared width"), })),
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
                    let error = if field.uses_opcodes {
                        let opcode_table = opcode_table.expect("validated struct opcode input");
                        quote!(<#child_view<'__wire_repr_wire> as #runtime::WireViewWithOpcodes<'__wire_repr_wire, #opcode_table>>::DecodeError)
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
    let encode_variants = fields.iter().zip(&variants).zip(&labels).filter_map(
        |((field, variant), label)| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                Some(quote!(#[doc = concat!("Preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::FixedCodec>::EncodeError),))
            }
            FieldKind::Nested => {
                let ty = &field.ty;
                if field.uses_opcodes {
                    let opcode_table = opcode_table.expect("validated struct opcode input");
                    Some(quote!(#[doc = concat!("Nested preparation error for field `", #label, "`.")] #variant(<#ty as #runtime::WireEncodeWithOpcodes<#opcode_table>>::EncodeError),))
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
    let prepare_steps = fields
        .iter()
        .zip(&plans)
        .zip(&variants)
        .zip(&controlled_by)
        .map(|(((field, plan), variant), controlled_by)| {
            let field_name = &field.name;
            match (&field.kind, controlled_by) {
                (FieldKind::Fixed(codec), Some(bytes_index)) => {
                    let codec = codec_tokens(codec, runtime);
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(self.#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: self.#bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::FixedCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Fixed(codec), None) => {
                    let codec = codec_tokens(codec, runtime);
                    quote!(let #plan = <#codec as #runtime::FixedCodec>::plan(self.#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Nested, _) => {
                    let ty = &field.ty;
                    if field.uses_opcodes {
                        let opcode_table = opcode_table.expect("validated struct opcode input");
                        quote!(let #plan = <#ty as #runtime::WireEncodeWithOpcodes<#opcode_table>>::prepare_with_opcodes(self.#field_name, opcodes).map_err(#encode_error::#variant)?;)
                    } else {
                        quote!(let #plan = <#ty as #runtime::WireEncode>::prepare(self.#field_name).map_err(#encode_error::#variant)?;)
                    }
                }
                (FieldKind::Prefix(codec), Some(bytes_index)) => {
                    let source_ty = &field.ty;
                    let source_label = field_name.to_string();
                    let bytes_field = &fields[*bytes_index];
                    let bytes_name = &bytes_field.name;
                    let bytes_label = bytes_name.to_string();
                    quote! {
                        let source_value = <#source_ty>::try_from(self.#bytes_name.len()).map_err(|_| {
                            #encode_error::LengthNotRepresentable {
                                field: #bytes_label,
                                source: #source_label,
                                length: self.#bytes_name.len(),
                            }
                        })?;
                        let #plan = <#codec as #runtime::PrefixCodec>::plan(source_value)
                            .map_err(#encode_error::#variant)?;
                    }
                }
                (FieldKind::Prefix(codec), None) => {
                    quote!(let #plan = <#codec as #runtime::PrefixCodec>::plan(self.#field_name).map_err(#encode_error::#variant)?;)
                }
                (FieldKind::Bytes { .. } | FieldKind::Rest, _) => {
                    quote!(let #plan = self.#field_name;)
                }
            }
        });
    let geometry_steps = fields
        .iter()
        .zip(&plans)
        .zip(&gaps)
        .map(|((field, plan), gap)| {
            let length = match field.kind {
                FieldKind::Fixed(_) | FieldKind::Prefix(_) | FieldKind::Bytes { .. } | FieldKind::Rest => {
                    quote!(#runtime::EncodePlan::encoded_len(&#plan))
                }
                FieldKind::Nested => {
                    quote!(#runtime::PreparedLayout::encoded_len(&#plan))
                }
            };
            if let (Some(gap), Some(position)) = (gap, &field.position) {
                let label = field.name.to_string();
                let field_start = match position {
                    FieldPosition::Static(position) => quote!(#position),
                    FieldPosition::Source(source) => quote! {
                        usize::try_from(self.#source).map_err(|_| {
                            #encode_error::PositionNotRepresentable {
                                field: #label,
                                value: self.#source as u128,
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
        });
    let commit_steps = fields
        .iter()
        .zip(&plans)
        .zip(&gaps)
        .map(|((field, plan), gap)| {
            let padding = gap.as_ref().map(|gap| {
                quote! {
                    let (padding, suffix) = remaining.split_at_mut(self.#gap);
                    padding.fill(0);
                    remaining = suffix;
                }
            });
            let write = match field.kind {
                FieldKind::Fixed(_)
                | FieldKind::Prefix(_)
                | FieldKind::Bytes { .. }
                | FieldKind::Rest => quote! {
                    let width = #runtime::EncodePlan::encoded_len(&self.#plan);
                    let (slot, suffix) = remaining.split_at_mut(width);
                    #runtime::EncodePlan::write_into(&self.#plan, slot);
                    remaining = suffix;
                },
                FieldKind::Nested => quote! {
                    let width = #runtime::PreparedLayout::encoded_len(&self.#plan);
                    let (slot, suffix) = remaining.split_at_mut(width);
                    let _ = #runtime::PreparedLayout::commit_into(self.#plan, slot)?;
                    remaining = suffix;
                },
            };
            quote! {
                #padding
                #write
            }
        });

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
                /// The decoded source value.
                value: u128,
            },
        }
    });
    let decode_length_arm = has_bytes.then(|| {
        quote! {
            Self::LengthNotRepresentable { field, value } => {
                write!(formatter, "byte length {value} for field `{field}` does not fit in usize")
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

    let view_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireViewWithOpcodes<'__wire_repr_wire, #opcode_table> for #view<'__wire_repr_wire> {
                type DecodeError = #view_error_type;
                fn parse_view_with_opcodes(input: &'__wire_repr_wire [u8], opcodes: &#opcode_table) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let mut remaining = input;
                    #(#decode_steps)*
                    let represented = &input[..input.len() - remaining.len()];
                    Ok((Self { bytes: represented, #(#view_initializers,)* }, remaining))
                }
                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError { #decode_error::TrailingBytes { expected: represented, actual: input } }
                fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes }
            }
        }
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

    let inherent_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature { #runtime::ViewRequest::new(input) }
                /// Supplies the opcode mapping used to prepare this value.
                #vis fn opcodes(self, opcodes: &#opcode_table) -> #runtime::OpcodeEncodeRequest<'_, Self, #opcode_table> { #runtime::OpcodeEncodeRequest::new(self, opcodes) }
            }
        }
    } else {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature { #runtime::ViewRequest::new(input) }
                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                    #runtime::ViewCursor::new(input)
                }
                /// Consumes this value and prepares an atomic encoding.
                #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan_type, #encode_error_type> where Self: '__wire_repr_value { <Self as #runtime::WireEncode>::prepare(self) }
                /// Consumes this value, prepares it, and commits it into `output`.
                #vis fn build_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error_type>> { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) }
            }
        }
    };

    let encode_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl #impl_generics #runtime::WireEncodeWithOpcodes<#opcode_table> for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;
                fn prepare_with_opcodes<'__wire_repr_value>(self, opcodes: &#opcode_table) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value { #(#prepare_steps)* let mut encoded_len = 0usize; #(#geometry_steps)* Ok(#plan { #(#plans,)* #(#gap_names,)* encoded_len }) }
            }
        }
    } else {
        quote! {
            impl #impl_generics #runtime::WireEncode for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;
                fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value { #(#prepare_steps)* let mut encoded_len = 0usize; #(#geometry_steps)* Ok(#plan { #(#plans,)* #(#gap_names,)* encoded_len }) }
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
            #encode_position_variants
            #encode_length_variant
            #(#encode_variants)*
            /// The encoded length overflowed `usize`.
            LengthOverflow,
        }
        impl ::core::fmt::Display for #encode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #encode_position_arms
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
            #(#getters)*
        }
        #view_impl

        /// A prepared encoding for this wire representation.
        #vis struct #plan #plan_decl_generics #plan_decl_where { #(#plan_fields,)* #(#gap_fields,)* encoded_len: usize }
        impl #plan_impl_generics #plan_impl_type {
            /// Returns the exact encoded byte count.
            #[must_use]
            #vis const fn encoded_len(&self) -> usize { self.encoded_len }
        }
        impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
            type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;
            fn encoded_len(&self) -> usize { self.encoded_len }
            fn commit_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::OutputTooShortError> {
                if output.len() < self.encoded_len { return Err(#runtime::OutputTooShortError { required: self.encoded_len, available: output.len() }); }
                { let (mut remaining, _) = output.split_at_mut(self.encoded_len); #(#commit_steps)* }
                let (bytes, suffix) = output.split_at_mut(self.encoded_len);
                Ok((#runtime::Written::new(bytes), suffix))
            }
        }

        #inherent_impl
        impl #impl_generics #runtime::WireViewType for #self_type {
            type DecodeError<'__wire_repr_view> = #association_error_type;
            type View<'__wire_repr_view> = #view<'__wire_repr_view>;
        }
        #encode_impl

    })
}

fn type_uses_lifetime(ty: &syn::Type, lifetime: &syn::Lifetime) -> bool {
    struct Finder<'a> {
        target: &'a syn::Lifetime,
        found: bool,
    }

    impl<'ast> Visit<'ast> for Finder<'_> {
        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            self.found |= lifetime.ident == self.target.ident;
        }
    }

    let mut finder = Finder {
        target: lifetime,
        found: false,
    };
    finder.visit_type(ty);
    finder.found
}

fn render_fixed_view(model: WireStruct, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireStruct {
        vis,
        name,
        wire_lifetime,
        opcodes: _,
        fields,
    } = model;
    let view = format_ident!("{name}View");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let plan = format_ident!("{name}Plan");
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
    let position_sources: Vec<_> = (0..fields.len())
        .map(|index| {
            fields.iter().any(|field| {
                matches!(field.position, Some(FieldPosition::Source(ref source)) if source == &fields[index].name)
            })
        })
        .collect();
    let (impl_generics, self_type, view_signature) = if let Some(lifetime) = &wire_lifetime {
        (
            quote!(<#lifetime>),
            quote!(#name<#lifetime>),
            quote!(#vis fn view<'__wire_repr_view>(input: &'__wire_repr_view [u8]) -> #runtime::ViewRequest<'__wire_repr_view, #view<'__wire_repr_view>>),
        )
    } else {
        (
            quote!(),
            quote!(#name),
            quote!(#vis fn view<'__wire_repr_wire>(input: &'__wire_repr_wire [u8]) -> #runtime::ViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>>),
        )
    };

    let plain_fixed_sequence = fields.iter().all(|field| {
        field.position.is_none() && field.padding_before == 0 && field.alignment_before.is_none()
    });
    let fixed_widths: Vec<_> = fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Fixed(codec) => {
                let codec = codec_tokens(codec, runtime);
                quote!(<#codec as #runtime::FixedCodec>::WIDTH)
            }
            _ => unreachable!(),
        })
        .collect();
    let fixed_view_constructor = plain_fixed_sequence.then(|| {
        quote! {
            fn from_sequence(bytes: &'__wire_repr_wire [u8]) -> Self {
                Self { bytes }
            }
        }
    });
    let fixed_sequence_method = plain_fixed_sequence.then(|| {
        quote! {
            /// Validates complete fixed-width sequence framing and returns an infallible iterator.
            #vis fn views<'__wire_repr_view>(
                input: &'__wire_repr_view [u8],
            ) -> Result<
                #runtime::FixedViewIterator<'__wire_repr_view, #view<'__wire_repr_view>>,
                #runtime::FixedViewSequenceError,
            > {
                let item_width = 0usize #(+ #fixed_widths)*;
                #runtime::FixedViewIterator::new(input, item_width, #view::from_sequence)
            }
        }
    });

    let decode_steps = fields.iter().enumerate().map(|(index, field)| {
        let label = &labels[index];
        let field_name = &field.name;
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        let geometry = decode_geometry(field, label, &decode_error);
        let decode_source = position_sources[index].then(|| quote!(
            let #field_name = <#codec as #runtime::FixedCodec>::decode(bytes);
        ));
        quote! {
            #geometry
            let width = <#codec as #runtime::FixedCodec>::WIDTH;
            let available = remaining.len();
            let Some((bytes, suffix)) = remaining.split_at_checked(width) else {
                return Err(#decode_error::InputTooShort { field: #label, required: width, available });
            };
            #decode_source
            remaining = suffix;
        }
    });
    let getters = fields.iter().enumerate().map(|(index, field)| {
        let field_name = &field.name;
        let label = &labels[index];
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        let prior = fields
            .iter()
            .take(index)
            .map(|prior| getter_cursor_step(prior, runtime));
        let geometry = getter_geometry(field, runtime);
        let return_type = match &field.kind {
            FieldKind::Fixed(Codec::OwnedBytes(length)) => quote!(&'__wire_repr_wire [u8; #length]),
            FieldKind::Fixed(_) => {
                quote!(<#codec as #runtime::FixedCodec>::Value<'__wire_repr_wire>)
            }
            _ => unreachable!(),
        };
        let value = match &field.kind {
            FieldKind::Fixed(Codec::OwnedBytes(length)) => quote! {
                match <&'__wire_repr_wire [u8; #length]>::try_from(bytes) {
                    Ok(bytes) => bytes,
                    Err(_) => unreachable!("validated fixed byte array has its declared width"),
                }
            },
            FieldKind::Fixed(_) => quote!(<#codec as #runtime::FixedCodec>::decode(bytes)),
            _ => unreachable!(),
        };
        quote! {
            #[doc = concat!("Returns the decoded `", #label, "` field.")]
            #[must_use]
            #vis fn #field_name(&self) -> #return_type {
                let mut cursor = 0usize;
                #(#prior)*
                #geometry
                let width = <#codec as #runtime::FixedCodec>::WIDTH;
                let bytes = &self.bytes[cursor..cursor + width];
                #value
            }
        }
    });
    let plan_fields = fields.iter().zip(&plans).map(|(field, plan)| {
        let codec = match &field.kind {
            FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
            _ => unreachable!(),
        };
        quote!(#plan: <#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>)
    });
    let prepare_steps = fields.iter().zip(&plans).zip(&variants).map(|((field, plan), variant)| {
        let field_name = &field.name;
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        quote!(let #plan = <#codec as #runtime::FixedCodec>::plan(self.#field_name).map_err(#encode_error::#variant)?;)
    });
    let geometry_steps = fields.iter().zip(&plans).zip(&gaps).map(|((field, plan), gap)| {
        let length = quote!(#runtime::EncodePlan::encoded_len(&#plan));
        if let (Some(gap), Some(position)) = (gap, &field.position) {
            let label = field.name.to_string();
            let field_start = match position {
                FieldPosition::Static(position) => quote!(#position),
                FieldPosition::Source(source) => quote! { usize::try_from(self.#source).map_err(|_| #encode_error::PositionNotRepresentable { field: #label, value: self.#source as u128 })? },
            };
            quote! { let field_start = #field_start; if field_start < encoded_len { return Err(#encode_error::PositionBeforeCursor { field: #label, position: field_start, cursor: encoded_len }); } let #gap = field_start - encoded_len; encoded_len = field_start.checked_add(#length).ok_or(#encode_error::LengthOverflow)?; }
        } else if let Some(gap) = gap {
            let padding = field.padding_before;
            let alignment = match field.alignment_before { Some(boundary) => quote!(Some(#boundary)), None => quote!(None::<usize>) };
            quote! { let before_gap = encoded_len; let padded = encoded_len.checked_add(#padding).ok_or(#encode_error::LengthOverflow)?; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; let field_start = padded.checked_add(alignment_padding).ok_or(#encode_error::LengthOverflow)?; let #gap = field_start - before_gap; encoded_len = field_start.checked_add(#length).ok_or(#encode_error::LengthOverflow)?; }
        } else { quote!(encoded_len = encoded_len.checked_add(#length).ok_or(#encode_error::LengthOverflow)?;) }
    });
    let commit_steps = fields.iter().zip(&plans).zip(&gaps).map(|((_, plan), gap)| {
        let padding = gap.as_ref().map(|gap| quote! { let (padding, suffix) = remaining.split_at_mut(self.#gap); padding.fill(0); remaining = suffix; });
        quote! { #padding let width = #runtime::EncodePlan::encoded_len(&self.#plan); let (slot, suffix) = remaining.split_at_mut(width); #runtime::EncodePlan::write_into(&self.#plan, slot); remaining = suffix; }
    });
    let encode_variants = fields.iter().zip(&variants).zip(&labels).map(|((field, variant), label)| {
        let codec = match &field.kind { FieldKind::Fixed(codec) => codec_tokens(codec, runtime), _ => unreachable!() };
        quote!(#[doc = concat!("Preparation error for field `", #label, "`.")] #variant(<#codec as #runtime::FixedCodec>::EncodeError),)
    });
    let encode_display_arms = fields.iter().zip(&variants).zip(&labels).map(|((_, variant), label)| quote!(Self::#variant(error) => write!(formatter, "wire preparation failed for field `{}`: {error:?}", #label),));
    let decode_position_variants = has_positions.then(|| quote! { PositionNotRepresentable { field: &'static str, value: u128 }, PositionBeforeCursor { field: &'static str, position: usize, cursor: usize }, });
    let decode_position_arms = has_positions.then(|| quote! { Self::PositionNotRepresentable { field, value } => write!(formatter, "position {value} for field `{field}` does not fit in usize"), Self::PositionBeforeCursor { field, position, cursor } => write!(formatter, "field `{field}` starts at byte {position}, before the current byte {cursor}"), });
    let encode_position_variants = decode_position_variants.clone();
    let encode_position_arms = decode_position_arms.clone();
    let decode_geometry_variant =
        has_geometry.then(|| quote!(GeometryOverflow { field: &'static str },));
    let decode_geometry_arm = has_geometry.then(|| quote!(Self::GeometryOverflow { field } => write!(formatter, "placement before field `{field}` does not fit in usize"),));

    Ok(quote! {
        /// Typed decoding failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #decode_error { InputTooShort { field: &'static str, required: usize, available: usize }, #decode_position_variants #decode_geometry_variant TrailingBytes { expected: usize, actual: usize }, }
        impl ::core::fmt::Display for #decode_error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self { Self::InputTooShort { field, required, available } => { let required_unit = if *required == 1 { "byte" } else { "bytes" }; let available_unit = if *available == 1 { "byte" } else { "bytes" }; let available_verb = if *available == 1 { "remains" } else { "remain" }; write!(formatter, "field `{field}` needs {required} {required_unit}, but only {available} {available_unit} {available_verb}") }, #decode_position_arms #decode_geometry_arm Self::TrailingBytes { expected, actual } => { let trailing = actual.saturating_sub(*expected); let unit = if trailing == 1 { "byte" } else { "bytes" }; write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation") } } } }
        impl ::core::error::Error for #decode_error {}
        /// Typed encoding-preparation failures for this wire representation.
        #[allow(missing_docs)]
        #[derive(Debug)]
        #vis enum #encode_error { #encode_position_variants #(#encode_variants)* LengthOverflow, }
        impl ::core::fmt::Display for #encode_error { fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result { match self { #encode_position_arms #(#encode_display_arms)* Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"), } } }
        impl ::core::error::Error for #encode_error {}
        /// A bytes-backed validated read view for this wire representation.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> { bytes: &'__wire_repr_wire [u8] }
        impl<'__wire_repr_wire> #view<'__wire_repr_wire> { #[doc = "Returns this view's exact represented bytes."] #[must_use] #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes } #fixed_view_constructor #(#getters)* }
        impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> { type DecodeError = #decode_error; fn parse_view(input: &'__wire_repr_wire [u8]) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> { let mut remaining = input; #(#decode_steps)* let represented = &input[..input.len() - remaining.len()]; Ok((Self { bytes: represented }, remaining)) } fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError { #decode_error::TrailingBytes { expected: represented, actual: input } } fn as_bytes(&self) -> &'__wire_repr_wire [u8] { self.bytes } }
        impl #impl_generics #runtime::WireViewType for #self_type { type DecodeError<'__wire_repr_view> = #decode_error; type View<'__wire_repr_view> = #view<'__wire_repr_view>; }
        /// A prepared encoding for this wire representation.
        #vis struct #plan<'__wire_repr_value> { #(#plan_fields,)* #(#gap_names: usize,)* encoded_len: usize }
        #[allow(missing_docs)]
        impl #plan<'_> { #[must_use] #vis const fn encoded_len(&self) -> usize { self.encoded_len } }
        impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> { type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>; fn encoded_len(&self) -> usize { self.encoded_len } fn commit_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::OutputTooShortError> { if output.len() < self.encoded_len { return Err(#runtime::OutputTooShortError { required: self.encoded_len, available: output.len() }); } { let (mut remaining, _) = output.split_at_mut(self.encoded_len); #(#commit_steps)* } let (bytes, suffix) = output.split_at_mut(self.encoded_len); Ok((#runtime::Written::new(bytes), suffix)) } }
        impl #impl_generics #self_type { #[doc = "Starts validating a bytes-backed read view from the supplied input."] #view_signature { #runtime::ViewRequest::new(input) } #fixed_sequence_method #[doc = "Consumes this value and prepares an atomic encoding."] #vis fn prepare<'__wire_repr_value>(self) -> Result<#plan<'__wire_repr_value>, #encode_error> where Self: '__wire_repr_value { <Self as #runtime::WireEncode>::prepare(self) } #[doc = "Consumes this value, prepares it, and commits it into `output`."] #vis fn build_into<'__wire_repr_output>(self, output: &'__wire_repr_output mut [u8]) -> Result<(#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]), #runtime::BuildIntoError<#encode_error>> { let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?; #runtime::PreparedLayout::commit_into(plan, output).map_err(#runtime::BuildIntoError::Output) } }
        impl #impl_generics #runtime::WireEncode for #self_type { type EncodeError = #encode_error; type Plan<'__wire_repr_value> = #plan<'__wire_repr_value> where Self: '__wire_repr_value; fn prepare<'__wire_repr_value>(self) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError> where Self: '__wire_repr_value { #(#prepare_steps)* let mut encoded_len = 0usize; #(#geometry_steps)* Ok(#plan { #(#plans,)* #(#gap_names,)* encoded_len }) } }
    })
}

fn decode_geometry(
    field: &super::super::model::Field,
    label: &str,
    decode_error: &proc_macro2::Ident,
) -> TokenStream {
    if let Some(position) = &field.position {
        let (position, conversion) = match position {
            FieldPosition::Static(position) => (quote!(#position), quote!()),
            FieldPosition::Source(source) => (
                quote!(position),
                quote!(let position = usize::try_from(#source).map_err(|_| #decode_error::PositionNotRepresentable { field: #label, value: #source as u128 })?;),
            ),
        };
        quote! { #conversion let represented = input.len() - remaining.len(); if #position < represented { return Err(#decode_error::PositionBeforeCursor { field: #label, position: #position, cursor: represented }); } let gap = #position - represented; let available = remaining.len(); let Some((_, suffix)) = remaining.split_at_checked(gap) else { return Err(#decode_error::InputTooShort { field: #label, required: gap, available }); }; remaining = suffix; }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! { let represented = input.len() - remaining.len(); let padded = represented.checked_add(#padding).ok_or(#decode_error::GeometryOverflow { field: #label })?; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; let gap = #padding.checked_add(alignment_padding).ok_or(#decode_error::GeometryOverflow { field: #label })?; let available = remaining.len(); let Some((_, suffix)) = remaining.split_at_checked(gap) else { return Err(#decode_error::InputTooShort { field: #label, required: gap, available }); }; remaining = suffix; }
    }
}

fn getter_cursor_step(field: &super::super::model::Field, runtime: &TokenStream) -> TokenStream {
    let codec = match &field.kind {
        FieldKind::Fixed(codec) => codec_tokens(codec, runtime),
        _ => unreachable!(),
    };
    let geometry = getter_geometry(field, runtime);
    quote! { #geometry cursor += <#codec as #runtime::FixedCodec>::WIDTH; }
}

fn getter_geometry(field: &super::super::model::Field, _runtime: &TokenStream) -> TokenStream {
    if let Some(position) = &field.position {
        match position {
            FieldPosition::Static(position) => quote!(cursor = #position;),
            FieldPosition::Source(source) => {
                quote!(cursor = usize::try_from(self.#source()).expect("validated position source fits usize");)
            }
        }
    } else if field.padding_before == 0 && field.alignment_before.is_none() {
        quote!()
    } else {
        let padding = field.padding_before;
        let alignment = match field.alignment_before {
            Some(boundary) => quote!(Some(#boundary)),
            None => quote!(None::<usize>),
        };
        quote! { let padded = cursor + #padding; let alignment_padding = match #alignment { Some(boundary) => { let remainder = padded % boundary; if remainder == 0 { 0 } else { boundary - remainder } }, None => 0 }; cursor = padded + alignment_padding; }
    }
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
