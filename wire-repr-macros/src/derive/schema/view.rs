use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, Generics, LifetimeParam, TypeParam, parse_quote};

use super::model::{FieldKind, Scalar, ScalarType, Schema, ValueType};
use super::{
    fresh_field_ident, fresh_schema_lifetime, from_bytes_method, pascal, private_ident,
    scalar_type_tokens, value_type_tokens, view_offset, view_optional_size,
};

pub(super) fn render(schema: &Schema, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let vis = &schema.vis;
    let name = &schema.name;
    let view_trait = format_ident!("{}View", name.unraw());
    let descriptor = format_ident!("__WireRepr{}ViewDescriptor", name.unraw());
    let borrowed_view = format_ident!("__WireRepr{}BorrowedView", name.unraw());
    let owned_view = format_ident!("__WireRepr{}ViewState", name.unraw());
    let error = format_ident!("{}ViewError", name.unraw());

    let bounded = bounded_generics(schema, runtime);
    let (impl_generics, type_generics, where_clause) = bounded.split_for_impl();
    let self_type = quote!(#name #type_generics);
    let backing = super::fresh_type_ident(&bounded, "Backing");
    let view_lifetime = fresh_schema_lifetime(schema, &bounded, "view");
    let input_field = fresh_field_ident(schema, "input");
    let represented_length_field = private_ident(schema, "represented_length");
    let frame_input = private_ident(schema, "frame_input");
    let frame_offset = private_ident(schema, "frame_offset");
    let current_input = private_ident(schema, "current_input");
    let view_input = private_ident(schema, "view_input");
    let input_length = private_ident(schema, "input_length");
    let frame_result = private_ident(schema, "frame_result");
    let framed_descriptor = private_ident(schema, "framed_descriptor");
    let framed_consumed = private_ident(schema, "framed_consumed");
    let owned_value = private_ident(schema, "owned_value");

    let nested_fields = schema.nested_fields().collect::<Vec<_>>();
    let descriptor_parameters = (0..nested_fields.len())
        .map(|index| format_ident!("__WireReprState{index}"))
        .collect::<Vec<_>>();
    let error_parameters = (0..nested_fields.len())
        .map(|index| format_ident!("__WireReprError{index}"))
        .collect::<Vec<_>>();
    let nested_error_types = nested_fields.iter().map(|field| {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        quote!(<#ty as #runtime::WireView>::Error)
    });
    let error_type = if nested_fields.is_empty() {
        quote!(#error)
    } else {
        quote!(#error<#(#nested_error_types),*>)
    };

    let error_names = error_names(schema);
    let error_declaration = render_error(schema, &error, &error_names, &error_parameters, runtime);
    let validator_calls =
        schema.validators.iter().zip(&error_names.validators).map(
            |(validator, variant)| quote!(#validator(&#owned_value).map_err(#error::#variant)?;),
        );

    let retained_char_fields = schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Scalar(scalar) = &field.kind else {
                return None;
            };
            (matches!(scalar.value_type, ValueType::Char) && scalar.constant.is_none()).then(|| {
                (
                    field.name.clone(),
                    private_ident(schema, &format!("scalar_{index}_value")),
                )
            })
        })
        .collect::<Vec<_>>();
    let descriptor_char_fields = retained_char_fields
        .iter()
        .map(|(field, _)| quote!(#field: char,));
    let descriptor_nested_fields =
        nested_fields
            .iter()
            .zip(&descriptor_parameters)
            .map(|(nested, state)| {
                let field = &nested.name;
                quote!(#field: #state,)
            });
    let descriptor_declaration = if descriptor_parameters.is_empty() {
        quote! {
            #[doc(hidden)]
            #vis struct #descriptor {
                #(#descriptor_char_fields)*
            }
        }
    } else {
        quote! {
            #[doc(hidden)]
            #vis struct #descriptor<#(#descriptor_parameters),*> {
                #(#descriptor_char_fields)*
                #(#descriptor_nested_fields)*
            }
        }
    };
    let descriptor_states = nested_fields.iter().map(|field| {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        quote!(<#ty as #runtime::WireView>::State)
    });
    let descriptor_type = if nested_fields.is_empty() {
        quote!(#descriptor)
    } else {
        quote!(#descriptor<#(#descriptor_states),*>)
    };

    let borrowed_char_fields = retained_char_fields
        .iter()
        .map(|(field, _)| quote!(#field: char,));
    let borrowed_nested_fields = nested_fields.iter().map(|field| {
        let name = &field.name;
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        quote!(#name: &#view_lifetime <#ty as #runtime::WireView>::State,)
    });
    let borrowed_char_values = retained_char_fields
        .iter()
        .map(|(field, _)| quote!(#field: state.#field,));
    let borrowed_nested_values = nested_fields.iter().map(|field| {
        let name = &field.name;
        quote!(#name: &state.#name,)
    });
    let descriptor_char_values = retained_char_fields
        .iter()
        .map(|(field, value)| quote!(#field: #value,));
    let descriptor_nested_values = nested_fields.iter().map(|field| {
        let name = &field.name;
        let state = private_ident(schema, &format!("{}_state", name.unraw()));
        quote!(#name: #state,)
    });
    let descriptor_value = quote! {
        #descriptor {
            #(#descriptor_char_values)*
            #(#descriptor_nested_values)*
        }
    };

    let fixed_size = view_optional_size(schema, runtime);
    let frame_steps = render_frame_steps(
        schema,
        &error,
        &error_names,
        &frame_input,
        &frame_offset,
        runtime,
    );
    let consumed = if let Some(last) = schema.fields.last() {
        match &last.kind {
            FieldKind::Nested(_) => {
                let total = private_ident(schema, &format!("{}_total", last.name.unraw()));
                quote!(#total)
            }
            FieldKind::Scalar(_) | FieldKind::Bytes(_) => {
                let total = view_offset_for_end(schema, last, runtime);
                quote!(#total.expect("framed fixed schema width"))
            }
        }
    } else {
        quote!(0usize)
    };

    let trait_methods = render_trait_methods(schema, runtime);
    let owned_methods = render_view_methods(schema, false, runtime);
    let borrowed_methods = render_view_methods(schema, true, runtime);

    let mut owned_generics = bounded.clone();
    let backing_position = owned_generics
        .params
        .iter()
        .take_while(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
        .count();
    owned_generics.params.insert(
        backing_position,
        GenericParam::Type(TypeParam::from(backing.clone())),
    );
    owned_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let (owned_impl, owned_types, owned_where) = owned_generics.split_for_impl();

    let mut borrowed_generics = bounded.clone();
    borrowed_generics.params.insert(
        0,
        GenericParam::Lifetime(LifetimeParam::new(view_lifetime.clone())),
    );
    let (borrowed_impl, borrowed_types, borrowed_where) = borrowed_generics.split_for_impl();
    let trait_path = quote!(#view_trait #type_generics);

    Ok(quote! {
        #error_declaration
        #descriptor_declaration

        #[doc(hidden)]
        #vis struct #owned_view #owned_impl #owned_where {
            #input_field: #backing,
            #represented_length_field: usize,
            descriptor: #descriptor_type,
        }

        #[doc(hidden)]
        #vis struct #borrowed_view #borrowed_impl #borrowed_where {
            #input_field: &#view_lifetime [u8],
            #(#borrowed_char_fields)*
            #(#borrowed_nested_fields)*
        }

        #[doc = "Exact-source view API generated for this schema."]
        #vis trait #view_trait #impl_generics #where_clause {
            /// Returns the exact represented bytes.
            fn as_bytes(&self) -> &[u8];

            #(#trait_methods)*
        }

        impl #owned_impl AsRef<[u8]> for #owned_view #owned_types #owned_where {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.#input_field.as_ref()[..self.#represented_length_field]
            }
        }

        impl #borrowed_impl AsRef<[u8]> for #borrowed_view #borrowed_types #borrowed_where {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                self.#input_field
            }
        }

        impl #owned_impl #trait_path for #owned_view #owned_types #owned_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }

            #(#owned_methods)*
        }

        impl #borrowed_impl #trait_path for #borrowed_view #borrowed_types #borrowed_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                self.#input_field
            }

            #(#borrowed_methods)*
        }

        // SAFETY: generated framing bounds-checks every retained scalar value and child extent;
        // the descriptor owns no input references and reconstruction slices the same exact span.
        #[allow(unsafe_code)]
        unsafe impl #impl_generics #runtime::WireView for #self_type #where_clause {
            type Error = #error_type;
            type State = #descriptor_type;
            type View<#view_lifetime> = #borrowed_view #borrowed_types;

            const FIXED_SIZE: Option<usize> = #fixed_size;

            #[inline]
            fn frame(
                #frame_input: &[u8],
                #frame_offset: usize,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                #(#frame_steps)*
                Ok(#runtime::Frame::new(#descriptor_value, #consumed))
            }

            #[inline(always)]
            unsafe fn from_validated_parts<#view_lifetime>(
                input: &#view_lifetime [u8],
                state: &#view_lifetime Self::State,
            ) -> Self::View<#view_lifetime> {
                // SAFETY: the WireView caller guarantees `input` has the exact framed length and
                // `state` came from that frame; every retained child state is paired with its
                // checked generated range below.
                #borrowed_view {
                    #input_field: input,
                    #(#borrowed_char_values)*
                    #(#borrowed_nested_values)*
                }
            }
        }

        impl #impl_generics #self_type #where_clause {
            /// Validates one exact representation, including schema validators.
            #[inline]
            #vis fn view<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<impl #trait_path, #error_type> {
                let #current_input = #view_input.as_ref();
                let #input_length = #current_input.len();
                let #frame_result = <Self as #runtime::WireView>::frame(#current_input, 0)?;
                let (#framed_descriptor, #framed_consumed) = #frame_result.into_parts();
                if #framed_consumed > #input_length {
                    return Err(#error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed: #framed_consumed,
                        available: #input_length,
                    }));
                }
                let #owned_value = #owned_view {
                    #input_field: #view_input,
                    #represented_length_field: #framed_consumed,
                    descriptor: #framed_descriptor,
                };
                #(#validator_calls)*
                if #framed_consumed < #input_length {
                    return Err(#error::Trailing(#runtime::TrailingBytes {
                        offset: #framed_consumed,
                        trailing: #input_length - #framed_consumed,
                    }));
                }
                Ok(#owned_value)
            }

            /// Validates exact structural framing while skipping schema validators.
            #[inline]
            #vis fn view_unchecked<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<impl #trait_path, #error_type> {
                let #current_input = #view_input.as_ref();
                let #input_length = #current_input.len();
                let #frame_result = <Self as #runtime::WireView>::frame(#current_input, 0)?;
                let (#framed_descriptor, #framed_consumed) = #frame_result.into_parts();
                if #framed_consumed > #input_length {
                    return Err(#error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed: #framed_consumed,
                        available: #input_length,
                    }));
                }
                if #framed_consumed < #input_length {
                    return Err(#error::Trailing(#runtime::TrailingBytes {
                        offset: #framed_consumed,
                        trailing: #input_length - #framed_consumed,
                    }));
                }
                Ok(#owned_view {
                    #input_field: #view_input,
                    #represented_length_field: #framed_consumed,
                    descriptor: #framed_descriptor,
                })
            }
        }
    })
}

fn bounded_generics(schema: &Schema, runtime: &TokenStream) -> Generics {
    let mut generics = schema.generics.clone();
    for field in schema.nested_fields() {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: #runtime::WireView));
    }
    generics
}

struct ErrorNames {
    fields: Vec<FieldErrorNames>,
    validators: Vec<syn::Ident>,
}

struct FieldErrorNames {
    shortage: syn::Ident,
    mismatch: Option<syn::Ident>,
    conversion: Option<syn::Ident>,
    nested: Option<syn::Ident>,
    extent: Option<syn::Ident>,
}

fn error_names(schema: &Schema) -> ErrorNames {
    let mut used = BTreeSet::from([
        "InvalidFrame".to_owned(),
        "LayoutUnavailable".to_owned(),
        "Trailing".to_owned(),
    ]);
    let fields = schema
        .fields
        .iter()
        .map(|field| {
            let base = pascal(&field.name).to_string();
            let mismatch = field
                .kind
                .constant()
                .map(|_| unique_variant(&mut used, &format!("{base}Constant")));
            let conversion = match &field.kind {
                FieldKind::Scalar(scalar) if scalar.value_type.is_converted() => {
                    Some(unique_variant(&mut used, &format!("{base}Value")))
                }
                FieldKind::Scalar(_) | FieldKind::Bytes(_) | FieldKind::Nested(_) => None,
            };
            let nested = matches!(field.kind, FieldKind::Nested(_))
                .then(|| unique_variant(&mut used, &base));
            let extent = matches!(field.kind, FieldKind::Nested(_))
                .then(|| unique_variant(&mut used, &format!("{base}Extent")));
            let decorated_shortage = mismatch.is_some() || conversion.is_some() || nested.is_some();
            let shortage_base = if decorated_shortage {
                format!("{base}NeedMore")
            } else {
                base.clone()
            };
            let shortage = unique_variant(&mut used, &shortage_base);
            FieldErrorNames {
                shortage,
                mismatch,
                conversion,
                nested,
                extent,
            }
        })
        .collect();
    let validators = schema
        .validators
        .iter()
        .map(|path| {
            let name = path.segments.last().expect("validator path has a segment");
            unique_variant(&mut used, &pascal(&name.ident).to_string())
        })
        .collect();
    ErrorNames { fields, validators }
}

fn unique_variant(used: &mut BTreeSet<String>, base: &str) -> syn::Ident {
    if used.insert(base.to_owned()) {
        return format_ident!("{base}");
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}{suffix}");
        if used.insert(candidate.clone()) {
            return format_ident!("{candidate}");
        }
    }
    unreachable!("usize suffix space cannot be exhausted by generated variants")
}

fn render_error(
    schema: &Schema,
    error: &syn::Ident,
    names: &ErrorNames,
    error_parameters: &[syn::Ident],
    runtime: &TokenStream,
) -> TokenStream {
    let mut variants = Vec::new();
    let mut nested_index = 0usize;
    for (field, names) in schema.fields.iter().zip(&names.fields) {
        let field_name = field.name.to_string();
        let shortage = &names.shortage;
        let shortage_message = format!("failed to frame field `{field_name}`: {{0}}");
        variants.push(quote! {
            #[doc = "The field ended before its complete representation."]
            #[error(#shortage_message)]
            #shortage(#[source] #runtime::NeedMore),
        });
        if let Some(mismatch) = &names.mismatch {
            let mismatch_message = format!("invalid constant field `{field_name}`: {{0}}");
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let ty = value_type_tokens(scalar.value_type);
                    variants.push(quote! {
                        #[doc = "The stored constant did not match the schema value."]
                        #[error(#mismatch_message)]
                        #mismatch(#[source] #runtime::ConstantMismatch<#ty>),
                    });
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    variants.push(quote! {
                        #[doc = "The stored constant did not match the schema value."]
                        #[error(#mismatch_message)]
                        #mismatch(#[source] #runtime::ConstantMismatch<#ty>),
                    });
                }
                FieldKind::Nested(_) => unreachable!(),
            }
        }
        if let Some(conversion) = &names.conversion {
            let message = format!("invalid physical value for field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "The physical scalar cannot be represented by the Rust field type."]
                #[error(#message)]
                #conversion(#[source] #runtime::ScalarConversionError),
            });
        }
        if let Some(nested) = &names.nested {
            let parameter = &error_parameters[nested_index];
            nested_index += 1;
            let message = format!("failed to frame nested field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "The nested schema failed structural framing."]
                #[error(#message)]
                #nested(#[source] #parameter),
            });
        }
        if let Some(extent) = &names.extent {
            let message = format!("invalid extent for nested field `{field_name}`: {{0}}");
            variants.push(quote! {
                #[doc = "The nested schema reported an extent outside its assigned span."]
                #[error(#message)]
                #extent(#[source] #runtime::InvalidFrameExtent),
            });
        }
    }
    for (validator, variant) in schema.validators.iter().zip(&names.validators) {
        let validator_name = quote!(#validator).to_string();
        let validator_error =
            crate::validator::error_type(validator).expect("validated validator path");
        let message = format!("schema validator `{validator_name}` failed: {{0}}");
        variants.push(quote! {
            #[doc = "A schema-level validator rejected the framed view."]
            #[error(#message)]
            #variant(#[source] #validator_error),
        });
    }
    variants.push(quote! {
        #[doc = "A field offset or fixed width could not be established."]
        #[error("fixed layout is unavailable or overflows before field `{field}`")]
        LayoutUnavailable { field: &'static str },
    });
    variants.push(quote! {
        #[doc = "A child reported an extent outside its supplied input."]
        #[error("invalid child frame: {0}")]
        InvalidFrame(#[source] #runtime::InvalidFrameExtent),
    });
    variants.push(quote! {
        #[doc = "Exact framing found trailing input."]
        #[error("exact framing failed: {0}")]
        Trailing(#[source] #runtime::TrailingBytes),
    });

    let vis = &schema.vis;
    if error_parameters.is_empty() {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error {
                #(#variants)*
            }
        }
    } else {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<#(#error_parameters: ::core::error::Error + 'static),*> {
                #(#variants)*
            }
        }
    }
}

fn render_frame_steps(
    schema: &Schema,
    error: &syn::Ident,
    names: &ErrorNames,
    frame_input: &syn::Ident,
    frame_offset: &syn::Ident,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    let mut steps = Vec::new();
    for (index, (field, names)) in schema.fields.iter().zip(&names.fields).enumerate() {
        let field_name = field.name.unraw().to_string();
        let offset = private_ident(schema, &format!("field_{index}_offset"));
        let offset_value = view_offset(&field.offset, runtime);
        let absolute = private_ident(schema, &format!("field_{index}_absolute"));
        let input_end = private_ident(schema, &format!("field_{index}_input_end"));
        steps.push(quote! {
            let #offset = #offset_value.ok_or(#error::LayoutUnavailable { field: #field_name })?;
            let #absolute = #frame_offset
                .checked_add(#offset)
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
            let #input_end = #frame_offset
                .checked_add(#frame_input.len())
                .ok_or(#error::LayoutUnavailable { field: #field_name })?;
        });
        match &field.kind {
            FieldKind::Scalar(scalar) => {
                let bytes = private_ident(schema, &format!("scalar_{index}_bytes"));
                let raw = private_ident(schema, &format!("scalar_{index}_raw"));
                let decoded = private_ident(schema, &format!("scalar_{index}_value"));
                let expected_value = private_ident(schema, &format!("scalar_{index}_expected"));
                let width = scalar.width();
                let end = private_ident(schema, &format!("field_{index}_end"));
                let shortage = &names.shortage;
                steps.push(quote! {
                    let #end = #offset.checked_add(#width)
                        .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                    let #bytes: [u8; #width] = #frame_input
                        .get(#offset..#end)
                        .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                            offset: #input_end,
                            additional_at_least: #end.saturating_sub(#frame_input.len()),
                        }))?
                        .try_into()
                        .expect("scalar range has its declared fixed width");
                });
                if scalar.constant.is_some() || scalar.value_type.is_converted() {
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let value_ty = value_type_tokens(scalar.value_type);
                    let decode = from_bytes_method(scalar.endian);
                    let conversion = convert_from_wire(scalar, &raw);
                    let converted = if let Some(variant) = &names.conversion {
                        quote! {
                            let #decoded: #value_ty = #conversion.ok_or_else(|| {
                                #error::#variant(#runtime::ScalarConversionError {
                                    offset: #absolute,
                                    from: stringify!(#wire_ty),
                                    to: stringify!(#value_ty),
                                })
                            })?;
                        }
                    } else {
                        quote!(let #decoded: #value_ty = #raw;)
                    };
                    let constant_check = scalar.constant.as_ref().map(|expected| {
                        let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
                        let differs = constants_differ(scalar, &decoded, &expected_value);
                        quote! {
                            let #expected_value: #value_ty = #expected;
                            if #differs {
                                return Err(#error::#mismatch(#runtime::ConstantMismatch {
                                    offset: #absolute,
                                    expected: #expected_value,
                                    actual: #decoded,
                                }));
                            }
                        }
                    });
                    steps.push(quote! {
                        let #raw = #wire_ty::#decode(#bytes);
                        #converted
                        #constant_check
                    });
                }
            }
            FieldKind::Bytes(bytes_field) => {
                let bytes = private_ident(schema, &format!("bytes_{index}_value"));
                let end = private_ident(schema, &format!("field_{index}_end"));
                let len = &bytes_field.len;
                let ty = &field.ty;
                let shortage = &names.shortage;
                let constant_check = bytes_field.constant.as_ref().map(|constant| {
                    let expected = private_ident(schema, &format!("bytes_{index}_expected"));
                    let actual = private_ident(schema, &format!("bytes_{index}_actual"));
                    let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
                    quote! {
                        let #expected: #ty = #constant;
                        if #bytes != #expected.as_slice() {
                            let #actual: #ty = #bytes
                                .try_into()
                                .expect("mismatched byte-array span has its declared width");
                            return Err(#error::#mismatch(#runtime::ConstantMismatch {
                                offset: #absolute,
                                expected: #expected,
                                actual: #actual,
                            }));
                        }
                    }
                });
                steps.push(quote! {
                    let #end = #offset.checked_add(#len)
                        .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                    let #bytes = #frame_input
                        .get(#offset..#end)
                        .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                            offset: #input_end,
                            additional_at_least: #end.saturating_sub(#frame_input.len()),
                        }))?;
                    #constant_check
                });
            }
            FieldKind::Nested(nested) => {
                let ty = &nested.ty;
                let available = private_ident(schema, &format!("{}_input", field.name.unraw()));
                let child_frame = private_ident(schema, &format!("{}_frame", field.name.unraw()));
                let child_state = private_ident(schema, &format!("{}_state", field.name.unraw()));
                let child_consumed =
                    private_ident(schema, &format!("{}_consumed", field.name.unraw()));
                let child_total = private_ident(schema, &format!("{}_total", field.name.unraw()));
                let shortage = &names.shortage;
                let variant = names.nested.as_ref().expect("nested error variant");
                let extent = names.extent.as_ref().expect("nested extent variant");
                let fixed_check = if nested.terminal {
                    TokenStream::new()
                } else {
                    let expected =
                        private_ident(schema, &format!("{}_expected", field.name.unraw()));
                    quote! {
                        let #expected = <#ty as #runtime::WireView>::FIXED_SIZE
                            .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                        if #child_consumed != #expected {
                            return Err(#error::#extent(#runtime::InvalidFrameExtent {
                                offset: #absolute,
                                consumed: #child_consumed,
                                available: #expected,
                            }));
                        }
                    }
                };
                steps.push(quote! {
                    let #available = #frame_input.get(#offset..).ok_or_else(|| {
                        #error::#shortage(#runtime::NeedMore {
                            offset: #input_end,
                            additional_at_least: #offset.saturating_sub(#frame_input.len()),
                        })
                    })?;
                    let #child_frame = <#ty as #runtime::WireView>::frame(
                        #available,
                        #absolute,
                    )
                    .map_err(#error::#variant)?;
                    let (#child_state, #child_consumed) = #child_frame.into_parts();
                    if #child_consumed > #available.len() {
                        return Err(#error::#extent(#runtime::InvalidFrameExtent {
                            offset: #absolute,
                            consumed: #child_consumed,
                            available: #available.len(),
                        }));
                    }
                    #fixed_check
                    let #child_total = #offset.checked_add(#child_consumed)
                        .ok_or(#error::LayoutUnavailable { field: #field_name })?;
                });
            }
        }
    }
    steps
}

fn render_trait_methods(schema: &Schema, runtime: &TokenStream) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .map(|field| {
            let name = &field.name;
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let ty = value_type_tokens(scalar.value_type);
                    quote! {
                        #[doc = concat!("Returns decoded field `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    quote! {
                        #[doc = concat!("Returns fixed byte-array field `", stringify!(#name), "`.")]
                        fn #name(&self) -> #ty;
                    }
                }
                FieldKind::Nested(nested) => {
                    let ty = &nested.ty;
                    let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
                    quote! {
                        #[doc = concat!("Returns nested field `", stringify!(#name), "`.")]
                        fn #name<#field_lifetime>(&#field_lifetime self)
                            -> <#ty as #runtime::WireView>::View<#field_lifetime>;
                    }
                }
            }
        })
        .collect()
}

fn render_view_methods(schema: &Schema, borrowed: bool, runtime: &TokenStream) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .map(|field| {
            let name = &field.name;
            let offset_value = view_offset(&field.offset, runtime);
            match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let width = scalar.width();
                    let value_ty = value_type_tokens(scalar.value_type);
                    let body = if matches!(scalar.value_type, ValueType::Char)
                        && scalar.constant.is_none()
                    {
                        if borrowed {
                            quote!(self.#name)
                        } else {
                            quote!(self.descriptor.#name)
                        }
                    } else if let Some(constant) = &scalar.constant {
                        quote! {
                            let value: #value_ty = #constant;
                            value
                        }
                    } else {
                        let wire_ty = scalar_type_tokens(scalar.wire_type);
                        let decode = from_bytes_method(scalar.endian);
                        let raw = private_ident(schema, "scalar_raw");
                        let decoded = if scalar.value_type.is_converted() {
                            let conversion = convert_from_validated_wire(scalar, &raw);
                            quote!(#conversion)
                        } else {
                            quote!(#raw)
                        };
                        quote! {
                            let offset = #offset_value.expect("validated scalar offset");
                            let #raw = #wire_ty::#decode(
                                self.as_bytes()[offset..offset + #width]
                                    .try_into()
                                    .expect("validated scalar field span"),
                            );
                            #decoded
                        }
                    };
                    quote! {
                        #[inline(always)]
                        fn #name(&self) -> #value_ty {
                            #body
                        }
                    }
                }
                FieldKind::Bytes(bytes) => {
                    let ty = &field.ty;
                    let len = &bytes.len;
                    let body = if let Some(constant) = &bytes.constant {
                        quote! {
                            let value: #ty = #constant;
                            value
                        }
                    } else {
                        quote! {
                            let offset = #offset_value.expect("validated byte-array offset");
                            self.as_bytes()[offset..offset + #len]
                                .try_into()
                                .expect("validated byte-array field span")
                        }
                    };
                    quote! {
                        #[inline(always)]
                        fn #name(&self) -> #ty {
                            #body
                        }
                    }
                }
                FieldKind::Nested(nested) => {
                    let ty = &nested.ty;
                    let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
                    let state = if borrowed {
                        quote!(self.#name)
                    } else {
                        quote!(&self.descriptor.#name)
                    };
                    let end = if nested.terminal {
                        quote!(self.as_bytes().len())
                    } else {
                        quote!(
                            offset
                                + <#ty as #runtime::WireView>::FIXED_SIZE
                                    .expect("framed nonterminal child has fixed width")
                        )
                    };
                    quote! {
                        #[allow(unsafe_code)]
                        #[inline(always)]
                        fn #name<#field_lifetime>(&#field_lifetime self)
                            -> <#ty as #runtime::WireView>::View<#field_lifetime>
                        {
                            let offset = #offset_value.expect("validated nested offset");
                            let end = #end;
                            // SAFETY: framing produced this state for a child span of this exact length.
                            unsafe {
                                <#ty as #runtime::WireView>::from_validated_parts(
                                    &self.as_bytes()[offset..end],
                                    #state,
                                )
                            }
                        }
                    }
                }
            }
        })
        .collect()
}

fn view_offset_for_end(
    _schema: &Schema,
    field: &super::model::Field,
    runtime: &TokenStream,
) -> TokenStream {
    let offset = view_offset(&field.offset, runtime);
    let width = match &field.kind {
        FieldKind::Scalar(scalar) => {
            let width = scalar.width();
            quote!(#width)
        }
        FieldKind::Bytes(bytes) => {
            let len = &bytes.len;
            quote!(#len)
        }
        FieldKind::Nested(_) => unreachable!(),
    };
    quote!(#offset.and_then(|offset| offset.checked_add(#width)))
}

fn constants_differ(scalar: &Scalar, actual: &syn::Ident, expected: &syn::Ident) -> TokenStream {
    if matches!(
        scalar.value_type,
        ValueType::Scalar(ScalarType::F32 | ScalarType::F64)
    ) {
        quote!(#actual.to_bits() != #expected.to_bits())
    } else {
        quote!(#actual != #expected)
    }
}

fn convert_from_validated_wire(scalar: &Scalar, raw: &syn::Ident) -> TokenStream {
    match scalar.value_type {
        ValueType::Scalar(_) => quote!(#raw),
        ValueType::Usize => quote!(#raw as usize),
        ValueType::Isize => quote!(#raw as isize),
        ValueType::Bool => quote!(#raw != 0),
        ValueType::Char => unreachable!("char getters use retained validated descriptor state"),
    }
}

fn convert_from_wire(scalar: &Scalar, raw: &syn::Ident) -> TokenStream {
    match scalar.value_type {
        ValueType::Scalar(_) => quote!(Some(#raw)),
        ValueType::Usize => quote!(usize::try_from(#raw).ok()),
        ValueType::Isize => quote!(isize::try_from(#raw).ok()),
        ValueType::Bool => quote!(match #raw {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }),
        ValueType::Char => quote!(u32::try_from(#raw).ok().and_then(char::from_u32)),
    }
}
