//! Enum derive rendering.

use super::super::model::{Variant, WireEnum};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireEnum, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireEnum {
        vis,
        name,
        wire_lifetime,
        tag,
        opcodes,
        variants,
    } = model;
    let plan = format_ident!("{name}Plan");
    let view = format_ident!("{name}View");
    let view_variant = format_ident!("__{name}ViewVariant");
    let decode_error = format_ident!("{name}DecodeError");
    let encode_error = format_ident!("{name}EncodeError");
    let tag_codec = format_ident!("{}", tag.codec);
    let tag_type = format_ident!("{}", tag.ty);
    let opcode_table = opcodes.as_ref().map(|input| &input.table);
    let opcode_error = opcodes.as_ref().map(|input| &input.error);
    let uses_opcodes = opcodes.is_some();
    let has_body = variants.iter().any(|variant| variant.body.is_some());
    let view_variant_decl_generics = has_body.then(|| quote!(<'__wire_repr_wire>));
    let view_variant_type = if has_body {
        quote!(#view_variant<'__wire_repr_wire>)
    } else {
        quote!(#view_variant)
    };
    let body_view_paths: Vec<_> = variants
        .iter()
        .map(|variant| {
            variant
                .body
                .as_ref()
                .map(super::generated_view_path)
                .transpose()
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
            quote!(
                #vis fn view<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #runtime::ViewRequest<'__wire_repr_view, #view<'__wire_repr_view>>
            ),
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
            quote!(),
            quote!(#plan<'_>),
            quote!(
                #vis fn view<'__wire_repr_wire>(
                    input: &'__wire_repr_wire [u8],
                ) -> #runtime::ViewRequest<'__wire_repr_wire, #view<'__wire_repr_wire>>
            ),
        )
    };
    let (
        decode_error_decl_generics,
        view_error_type,
        association_error_type,
        decode_error_impl_type,
    ) = if has_body {
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
    let encode_error_decl_generics = wire_lifetime
        .as_ref()
        .map_or_else(|| quote!(), |lifetime| quote!(<#lifetime>));

    let plan_variants = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        match &variant.body {
            Some(body) => quote! {
                #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
                #variant_name {
                    /// Prepared encoded tag.
                    tag: <#runtime::#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                    /// Prepared variant body.
                    body: <#body as #runtime::WireEncode>::Plan<'__wire_repr_value>,
                    /// Exact total encoded length.
                    encoded_len: usize,
                },
            },
            None => quote! {
                #[doc = concat!("Prepared `", stringify!(#variant_name), "` representation.")]
                #variant_name {
                    /// Prepared encoded tag.
                    tag: <#runtime::#tag_codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>,
                    /// Exact total encoded length.
                    encoded_len: usize,
                },
            },
        }
    });
    let plan_lengths = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        quote!(Self::#variant_name { encoded_len, .. } => *encoded_len,)
    });
    let view_variants = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        match &variant.body {
            Some(_) => {
                let body_view = body_view_paths[index]
                    .as_ref()
                    .expect("body variants have generated view paths");
                quote!(#variant_name(#body_view<'__wire_repr_wire>),)
            }
            None => quote!(#variant_name,),
        }
    });
    let view_getters = variants.iter().enumerate().map(|(index, variant)| {
        let variant_name = &variant.name;
        let method = snake_case(variant_name);
        match &variant.body {
            Some(_) => {
                let body_view = body_view_paths[index]
                    .as_ref()
                    .expect("body variants have generated view paths");
                quote! {
                #[doc = concat!("Returns the `", stringify!(#variant_name), "` body view when selected.")]
                #[must_use]
                #vis fn #method(&self) -> Option<#body_view<'__wire_repr_wire>> {
                    match self.variant {
                        #view_variant::#variant_name(body) => Some(body),
                        _ => None,
                    }
                }
            }
            },
            None => {
                let method = format_ident!("is_{}", method);
                quote! {
                    #[doc = concat!("Returns whether the `", stringify!(#variant_name), "` variant is selected.")]
                    #[must_use]
                    #vis fn #method(&self) -> bool {
                        matches!(self.variant, #view_variant::#variant_name)
                    }
                }
            }
        }
    });
    let decode_variants = variants.iter().enumerate().filter_map(|(index, variant)| {
        variant.body.as_ref().map(|_| {
            let variant_name = &variant.name;
            let body_view = body_view_paths[index]
                .as_ref()
                .expect("body variants have generated view paths");
            let error = quote!(<#body_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::DecodeError);
            quote! {
                #[doc = concat!("Nested decode error for variant `", stringify!(#variant_name), "`.")]
                #variant_name(#error),
            }
        })
    });
    let encode_variants = variants.iter().filter_map(|variant| {
        variant.body.as_ref().map(|body| {
            let variant_name = &variant.name;
            quote! {
                #[doc = concat!("Nested preparation error for variant `", stringify!(#variant_name), "`.")]
                #variant_name(<#body as #runtime::WireEncode>::EncodeError),
            }
        })
    });
    let decode_display_arms = variants.iter().filter_map(|variant| {
        variant.body.as_ref().map(|_| {
            let variant_name = &variant.name;
            quote!(Self::#variant_name(error) => write!(formatter, "wire decode failed in variant `{}`: {error}", stringify!(#variant_name)),)
        })
    });
    let encode_display_arms = variants.iter().filter_map(|variant| {
        variant.body.as_ref().map(|_| {
            let variant_name = &variant.name;
            quote!(Self::#variant_name(error) => write!(formatter, "wire preparation failed for variant `{}`: {error}", stringify!(#variant_name)),)
        })
    });
    let decode_arms = variants.iter().enumerate().map(|(index, variant)| {
        decode_arm(
            variant,
            body_view_paths[index].as_ref(),
            &view_variant,
            &decode_error,
            &tag_type,
            uses_opcodes,
            runtime,
        )
    });
    let prepare_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let prepare_tag = if uses_opcodes {
            let opcode = variant.opcode.as_ref().expect("dynamic opcode selector");
            quote! {
                let raw_tag = opcodes
                    .encode(#opcode)
                    .map_err(#encode_error::OpcodeMapping)?
                    .ok_or(#encode_error::OpcodeUnavailable {
                        opcode: stringify!(#opcode),
                    })?;
                let tag = match <#runtime::#tag_codec as #runtime::FixedCodec>::plan(raw_tag) {
                    Ok(plan) => plan,
                    Err(error) => match error {},
                };
            }
        } else {
            let tag_value = variant.tag.expect("static tag");
            quote! {
                let tag = match <#runtime::#tag_codec as #runtime::FixedCodec>::plan(
                    #tag_value as #tag_type,
                ) {
                    Ok(plan) => plan,
                    Err(error) => match error {},
                };
            }
        };
        match &variant.body {
            Some(body) => quote! {
                Self::#variant_name(body) => {
                    #prepare_tag
                    let body = <#body as #runtime::WireEncode>::prepare(body)
                        .map_err(#encode_error::#variant_name)?;
                    let encoded_len = <#runtime::#tag_codec as #runtime::FixedCodec>::WIDTH
                        .checked_add(#runtime::PreparedLayout::encoded_len(&body))
                        .ok_or(#encode_error::LengthOverflow)?;
                    Ok(#plan::#variant_name { tag, body, encoded_len })
                }
            },
            None => quote! {
                Self::#variant_name => {
                    #prepare_tag
                    Ok(#plan::#variant_name {
                        tag,
                        encoded_len: <#runtime::#tag_codec as #runtime::FixedCodec>::WIDTH,
                    })
                }
            },
        }
    });
    let opcode_decode_variant = opcode_error.map(|error| {
        quote! {
            /// The supplied opcode mapping failed while resolving a raw ID.
            OpcodeMapping(#error),
        }
    });
    let opcode_encode_variants = opcode_error.map(|error| {
        quote! {
            /// The supplied opcode mapping failed while resolving a variant ID.
            OpcodeMapping(#error),
            /// The supplied mapping has no canonical ID for this opcode.
            OpcodeUnavailable {
                /// The declared opcode selector.
                opcode: &'static str,
            },
        }
    });
    let opcode_decode_display_arm = uses_opcodes.then(|| quote! {
        Self::OpcodeMapping(error) => write!(formatter, "opcode mapping failed while decoding: {error}"),
    });
    let opcode_encode_display_arms = uses_opcodes.then(|| quote! {
        Self::OpcodeMapping(error) => write!(formatter, "opcode mapping failed while encoding: {error}"),
        Self::OpcodeUnavailable { opcode } => write!(formatter, "opcode mapping has no canonical ID for `{opcode}`"),
    });

    let commit_arms = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        match &variant.body {
            Some(_) => quote! {
                Self::#variant_name { tag, body, .. } => {
                    let (bytes, suffix) = output.split_at_mut(encoded_len);
                    let (tag_output, body_output) = bytes.split_at_mut(
                        <#runtime::#tag_codec as #runtime::FixedCodec>::WIDTH,
                    );
                    #runtime::EncodePlan::write_into(&tag, tag_output);
                    #runtime::PreparedLayout::commit_into(body, body_output)?;
                    Ok((#runtime::Written::new(bytes), suffix))
                }
            },
            None => quote! {
                Self::#variant_name { tag, .. } => {
                    let (bytes, suffix) = output.split_at_mut(encoded_len);
                    #runtime::EncodePlan::write_into(&tag, bytes);
                    Ok((#runtime::Written::new(bytes), suffix))
                }
            },
        }
    });

    let view_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireViewWithOpcodes<'__wire_repr_wire, #opcode_table> for #view<'__wire_repr_wire> {
                type DecodeError = #view_error_type;

                fn parse_view_with_opcodes(
                    input: &'__wire_repr_wire [u8],
                    opcodes: &#opcode_table,
                ) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let width = <#runtime::#tag_codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                        return Err(#decode_error::InputTooShort {
                            required: width,
                            available,
                        });
                    };
                    let tag = <#runtime::#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);
                    let opcode = opcodes
                        .decode(tag)
                        .map_err(#decode_error::OpcodeMapping)?;
                    let Some(opcode) = opcode else {
                        return Err(#decode_error::UnknownTag { tag });
                    };

                    match opcode {
                        #(#decode_arms)*
                        _ => Err(#decode_error::UnknownTag { tag }),
                    }
                }

                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes {
                        expected: represented,
                        actual: input,
                    }
                }

                fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }
            }
        }
    } else {
        quote! {
            impl<'__wire_repr_wire> #runtime::WireView<'__wire_repr_wire> for #view<'__wire_repr_wire> {
                type DecodeError = #view_error_type;

                fn parse_view(
                    input: &'__wire_repr_wire [u8],
                ) -> Result<(Self, &'__wire_repr_wire [u8]), Self::DecodeError> {
                    let width = <#runtime::#tag_codec as #runtime::FixedCodec>::WIDTH;
                    let available = input.len();
                    let Some((tag_bytes, remaining)) = input.split_at_checked(width) else {
                        return Err(#decode_error::InputTooShort {
                            required: width,
                            available,
                        });
                    };
                    let tag = <#runtime::#tag_codec as #runtime::FixedCodec>::decode(tag_bytes);

                    match tag {
                        #(#decode_arms)*
                        _ => Err(#decode_error::UnknownTag { tag }),
                    }
                }

                fn trailing_bytes_error(represented: usize, input: usize) -> Self::DecodeError {
                    #decode_error::TrailingBytes {
                        expected: represented,
                        actual: input,
                    }
                }

                fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                    self.bytes
                }
            }
        }
    };

    let inherent_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature {
                    #runtime::ViewRequest::new(input)
                }

                /// Supplies the opcode mapping used to prepare this value.
                #vis fn opcodes(
                    self,
                    opcodes: &#opcode_table,
                ) -> #runtime::OpcodeEncodeRequest<'_, Self, #opcode_table> {
                    #runtime::OpcodeEncodeRequest::new(self, opcodes)
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics #self_type {
                /// Starts decoding from the supplied input.
                #view_signature {
                    #runtime::ViewRequest::new(input)
                }

                /// Returns a fail-closed cursor over consecutive representations.
                #vis fn cursor<'__wire_repr_view>(
                    input: &'__wire_repr_view [u8],
                ) -> #runtime::ViewCursor<'__wire_repr_view, #view<'__wire_repr_view>> {
                    #runtime::ViewCursor::new(input)
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
                ) -> Result<
                    (#runtime::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                    #runtime::BuildIntoError<#encode_error_type>,
                > {
                    let plan = self.prepare().map_err(#runtime::BuildIntoError::Prepare)?;
                    #runtime::PreparedLayout::commit_into(plan, output)
                        .map_err(#runtime::BuildIntoError::Output)
                }
            }
        }
    };

    let encode_impl = if let Some(opcode_table) = opcode_table {
        quote! {
            impl #impl_generics #runtime::WireEncodeWithOpcodes<#opcode_table> for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;

                fn prepare_with_opcodes<'__wire_repr_value>(
                    self,
                    opcodes: &#opcode_table,
                ) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
                where
                    Self: '__wire_repr_value,
                {
                    match self {
                        #(#prepare_arms)*
                    }
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics #runtime::WireEncode for #self_type {
                type EncodeError = #encode_error_type;
                type Plan<'__wire_repr_value> = #plan_type where Self: '__wire_repr_value;

                fn prepare<'__wire_repr_value>(
                    self,
                ) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
                where
                    Self: '__wire_repr_value,
                {
                    match self {
                        #(#prepare_arms)*
                    }
                }
            }
        }
    };

    Ok(quote! {
        /// Typed decoding failures for this tagged wire enum.
        #[derive(Debug)]
        #vis enum #decode_error #decode_error_decl_generics {
            /// The tag did not fit in the remaining input.
            InputTooShort {
                /// The required tag width.
                required: usize,
                /// The available byte count.
                available: usize,
            },
            /// The encoded tag does not identify a declared variant.
            UnknownTag {
                /// The decoded raw tag.
                tag: #tag_type,
            },
            #opcode_decode_variant
            #(#decode_variants)*
            /// Input had bytes after the complete representation.
            TrailingBytes {
                /// The represented byte count.
                expected: usize,
                /// The supplied byte count.
                actual: usize,
            },
        }

        impl ::core::fmt::Display for #decode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InputTooShort { required, available } => {
                        let required_unit = if *required == 1 { "byte" } else { "bytes" };
                        let available_unit = if *available == 1 { "byte" } else { "bytes" };
                        let available_verb = if *available == 1 { "remains" } else { "remain" };
                        write!(formatter, "tag needs {required} {required_unit}, but only {available} {available_unit} {available_verb}")
                    }
                    Self::UnknownTag { tag } => write!(formatter, "unknown wire tag {tag}"),
                    #opcode_decode_display_arm
                    #(#decode_display_arms)*
                    Self::TrailingBytes { expected, actual } => {
                        let trailing = actual.saturating_sub(*expected);
                        let unit = if trailing == 1 { "byte" } else { "bytes" };
                        write!(formatter, "input has {trailing} trailing {unit} after the {expected}-byte representation")
                    }
                }
            }
        }

        impl ::core::error::Error for #decode_error_impl_type {}

        #[derive(Clone, Copy, Debug)]
        enum #view_variant #view_variant_decl_generics {
            #(#view_variants)*
        }

        /// A bytes-backed validated read view for this tagged wire enum.
        #[derive(Clone, Copy, Debug)]
        #vis struct #view<'__wire_repr_wire> {
            bytes: &'__wire_repr_wire [u8],
            variant: #view_variant_type,
        }

        impl<'__wire_repr_wire> #view<'__wire_repr_wire> {
            /// Returns this view's exact represented bytes.
            #[must_use]
            #vis const fn as_bytes(&self) -> &'__wire_repr_wire [u8] {
                self.bytes
            }

            #(#view_getters)*
        }

        #view_impl

        /// Typed encoding-preparation failures for this tagged wire enum.
        #[derive(Debug)]
        #vis enum #encode_error #encode_error_decl_generics {
            #opcode_encode_variants
            #(#encode_variants)*
            /// The encoded length overflowed `usize`.
            LengthOverflow,
        }

        impl ::core::fmt::Display for #encode_error_impl_type {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #opcode_encode_display_arms
                    #(#encode_display_arms)*
                    Self::LengthOverflow => formatter.write_str("encoded representation length does not fit in usize"),
                }
            }
        }

        impl ::core::error::Error for #encode_error_impl_type {}

        /// A prepared encoding for this tagged wire enum.
        #vis enum #plan #plan_decl_generics #plan_decl_where {
            #(#plan_variants)*
        }

        impl #plan_impl_generics #plan_impl_type {
            /// Returns the exact encoded byte count.
            #[must_use]
            #vis fn encoded_len(&self) -> usize {
                match self {
                    #(#plan_lengths)*
                }
            }
        }

        impl #plan_impl_generics #runtime::PreparedLayout for #plan_impl_type {
            type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;

            fn encoded_len(&self) -> usize {
                self.encoded_len()
            }

            fn commit_into<'__wire_repr_output>(
                self,
                output: &'__wire_repr_output mut [u8],
            ) -> Result<
                (Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                #runtime::OutputTooShortError,
            > {
                let encoded_len = self.encoded_len();
                if output.len() < encoded_len {
                    return Err(#runtime::OutputTooShortError {
                        required: encoded_len,
                        available: output.len(),
                    });
                }

                match self {
                    #(#commit_arms)*
                }
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

fn decode_arm(
    variant: &Variant,
    body_view: Option<&TokenStream>,
    view_variant: &proc_macro2::Ident,
    decode_error: &proc_macro2::Ident,
    tag_type: &proc_macro2::Ident,
    uses_opcodes: bool,
    runtime: &TokenStream,
) -> TokenStream {
    let variant_name = &variant.name;
    let selector = if uses_opcodes {
        let opcode = variant.opcode.as_ref().expect("dynamic opcode selector");
        quote!(value if value == #opcode)
    } else {
        let tag = variant.tag.expect("static tag");
        quote!(value if value == (#tag as #tag_type))
    };

    match &variant.body {
        Some(_) => {
            let body_view = body_view.expect("body variants have generated view paths");
            quote! {
                #selector => {
                    let (body, suffix) = <#body_view<'__wire_repr_wire> as #runtime::WireView<'__wire_repr_wire>>::parse_view(remaining)
                        .map_err(#decode_error::#variant_name)?;
                    let represented = &input[..input.len() - suffix.len()];
                    Ok((
                        Self {
                            bytes: represented,
                            variant: #view_variant::#variant_name(body),
                        },
                        suffix,
                    ))
                }
            }
        }
        None => quote! {
            #selector => Ok((
                Self {
                    bytes: &input[..input.len() - remaining.len()],
                    variant: #view_variant::#variant_name,
                },
                remaining,
            )),
        },
    }
}

fn snake_case(name: &proc_macro2::Ident) -> proc_macro2::Ident {
    let source = name.to_string();
    let mut value = String::new();
    for (index, character) in source.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                value.push('_');
            }
            value.extend(character.to_lowercase());
        } else {
            value.push(character);
        }
    }
    format_ident!("{value}")
}
