use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, Generics, LifetimeParam, TypeParam, parse_quote};

use super::model::{FieldKind, Scalar, ScalarType, Schema, ValueType};
use super::{
    fresh_field_ident, fresh_schema_lifetime, fresh_type_ident, from_bytes_method, pascal,
    private_ident, scalar_type_tokens, value_type_tokens,
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
    let backing = fresh_type_ident(&bounded, "Backing");
    let view_lifetime = fresh_schema_lifetime(schema, &bounded, "view");
    let input_field = fresh_field_ident(schema, "input");
    let frame_input = private_ident(schema, "frame_input");
    let frame_offset = private_ident(schema, "frame_offset");
    let child_frame = private_ident(schema, "child_frame");
    let child_state = private_ident(schema, "child_state");
    let child_consumed = private_ident(schema, "child_consumed");
    let child_available = private_ident(schema, "child_available");
    let child_total = private_ident(schema, "child_total");
    let represented_length_field = private_ident(schema, "represented_length");
    let current_input = private_ident(schema, "current_input");
    let view_input = private_ident(schema, "view_input");
    let input_length = private_ident(schema, "input_length");
    let frame_result = private_ident(schema, "frame_result");
    let framed_descriptor = private_ident(schema, "framed_descriptor");
    let framed_consumed = private_ident(schema, "framed_consumed");
    let owned_value = private_ident(schema, "owned_value");

    let error_names = error_names(schema);
    let error_declaration = render_error(schema, &error, &error_names, runtime);
    let validator_calls =
        schema.validators.iter().zip(&error_names.validators).map(
            |(validator, variant)| quote!(#validator(&#owned_value).map_err(#error::#variant)?;),
        );
    let error_type = if let Some(nested) = &schema.nested {
        let ty = &nested.ty;
        quote!(#error<<#ty as #runtime::WireView>::Error>)
    } else {
        quote!(#error)
    };

    let retained_char_fields = schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Scalar(scalar) = &field.kind;
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
        .map(|(field, _)| quote!(#field: char,))
        .collect::<Vec<_>>();
    let descriptor_state = fresh_type_ident(&bounded, "DescriptorState");
    let (descriptor_declaration, descriptor_type) = if let Some(nested) = &schema.nested {
        let field = &nested.name;
        (
            quote! {
                #[doc(hidden)]
                #vis struct #descriptor<#descriptor_state> {
                    #(#descriptor_char_fields)*
                    #field: #descriptor_state,
                }
            },
            {
                let ty = &nested.ty;
                quote!(#descriptor<<#ty as #runtime::WireView>::State>)
            },
        )
    } else {
        (
            quote! {
                #[doc(hidden)]
                #vis struct #descriptor {
                    #(#descriptor_char_fields)*
                }
            },
            quote!(#descriptor),
        )
    };

    let borrowed_char_fields = retained_char_fields
        .iter()
        .map(|(field, _)| quote!(#field: char,))
        .collect::<Vec<_>>();
    let borrowed_nested_fields = schema.nested.iter().map(|nested| {
        let field = &nested.name;
        let ty = &nested.ty;
        quote!(#field: &#view_lifetime <#ty as #runtime::WireView>::State,)
    });
    let borrowed_state_fields = borrowed_char_fields
        .into_iter()
        .chain(borrowed_nested_fields);
    let borrowed_char_values = retained_char_fields
        .iter()
        .map(|(field, _)| quote!(#field: state.#field,));
    let borrowed_nested_values = schema.nested.iter().map(|nested| {
        let field = &nested.name;
        quote!(#field: &state.#field,)
    });
    let borrowed_state_values = borrowed_char_values.chain(borrowed_nested_values);
    let descriptor_char_values = retained_char_fields
        .iter()
        .map(|(field, value)| quote!(#field: #value,));

    let descriptor_value = if let Some(nested) = &schema.nested {
        let field = &nested.name;
        quote!(#descriptor {
            #(#descriptor_char_values)*
            #field: #child_state,
        })
    } else {
        quote!(#descriptor {
            #(#descriptor_char_values)*
        })
    };

    let fixed_size = if let Some(nested) = &schema.nested {
        let ty = &nested.ty;
        let prefix = schema.prefix_width;
        quote! {
            match <#ty as #runtime::WireView>::FIXED_SIZE {
                Some(child) => match #prefix.checked_add(child) {
                    Some(total) => Some(total),
                    None => None,
                },
                None => None,
            }
        }
    } else {
        let width = schema.prefix_width;
        quote!(Some(#width))
    };

    let frame_steps = render_frame_steps(
        schema,
        &error,
        &error_names,
        &frame_input,
        &frame_offset,
        &child_frame,
        runtime,
    );
    let consumed = if schema.nested.is_some() {
        quote!(#child_total)
    } else {
        let width = schema.prefix_width;
        quote!(#width)
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
    let nested_state = schema.nested.as_ref().map(|nested| {
        let offset = nested.offset;
        quote! {
            let (#child_state, #child_consumed) = #child_frame.into_parts();
            let #child_available = #frame_input.len() - #offset;
            if #child_consumed > #child_available {
                return Err(#error::InvalidFrame(#runtime::InvalidFrameExtent {
                    offset: #frame_offset + #offset,
                    consumed: #child_consumed,
                    available: #child_available,
                }));
            }
            let #child_total = #offset + #child_consumed;
        }
    });

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
            #(#borrowed_state_fields)*
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
                #nested_state
                Ok(#runtime::Frame::new(#descriptor_value, #consumed))
            }

            #[inline(always)]
            unsafe fn from_validated_parts<#view_lifetime>(
                input: &#view_lifetime [u8],
                state: &#view_lifetime Self::State,
            ) -> Self::View<#view_lifetime> {
                #borrowed_view {
                    #input_field: input,
                    #(#borrowed_state_values)*
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
                let #frame_result =
                    <Self as #runtime::WireView>::frame(#current_input, 0)?;
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
                let #frame_result =
                    <Self as #runtime::WireView>::frame(#current_input, 0)?;
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
    if let Some(nested) = &schema.nested {
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
    nested: Option<syn::Ident>,
    validators: Vec<syn::Ident>,
}

struct FieldErrorNames {
    shortage: syn::Ident,
    mismatch: Option<syn::Ident>,
    conversion: Option<syn::Ident>,
}

fn error_names(schema: &Schema) -> ErrorNames {
    let mut used = BTreeSet::from(["InvalidFrame".to_owned(), "Trailing".to_owned()]);
    let fields = schema
        .fields
        .iter()
        .map(|field| {
            let FieldKind::Scalar(scalar) = &field.kind;
            let base = pascal(&field.name).to_string();
            let shortage = if scalar.constant.is_some() || scalar.value_type.is_converted() {
                unique_variant(&mut used, &format!("{base}NeedMore"))
            } else {
                unique_variant(&mut used, &base)
            };
            FieldErrorNames {
                shortage,
                mismatch: scalar
                    .constant
                    .as_ref()
                    .map(|_| unique_variant(&mut used, &format!("{base}Constant"))),
                conversion: scalar
                    .value_type
                    .is_converted()
                    .then(|| unique_variant(&mut used, &format!("{base}Value"))),
            }
        })
        .collect();
    let nested = schema
        .nested
        .as_ref()
        .map(|field| unique_variant(&mut used, &pascal(&field.name).to_string()));
    let validators = schema
        .validators
        .iter()
        .map(|path| {
            let name = path.segments.last().expect("validator path has a segment");
            unique_variant(&mut used, &pascal(&name.ident).to_string())
        })
        .collect();
    ErrorNames {
        fields,
        nested,
        validators,
    }
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
    runtime: &TokenStream,
) -> TokenStream {
    let mut variants = Vec::new();
    for (field, names) in schema.fields.iter().zip(&names.fields) {
        let field_name = field.name.to_string();
        let FieldKind::Scalar(scalar) = &field.kind;
        let shortage = &names.shortage;
        let shortage_message = format!("failed to frame field `{field_name}`: {{0}}");
        variants.push(quote!(
            #[doc = "The scalar field ended before its complete representation."]
            #[error(#shortage_message)]
            #shortage(#[source] #runtime::NeedMore),
        ));
        if scalar.constant.is_some() {
            let mismatch = names.mismatch.as_ref().expect("constant mismatch variant");
            let value_ty = value_type_tokens(scalar.value_type);
            let mismatch_message = format!("invalid constant field `{field_name}`: {{0}}");
            variants.push(quote!(
                #[doc = "The stored constant did not match the schema value."]
                #[error(#mismatch_message)]
                #mismatch(#[source] #runtime::ConstantMismatch<#value_ty>),
            ));
        }
        if let Some(conversion) = &names.conversion {
            let message = format!("invalid physical value for field `{field_name}`: {{0}}");
            variants.push(quote!(
                #[doc = "The physical scalar cannot be represented by the Rust field type."]
                #[error(#message)]
                #conversion(#[source] #runtime::ScalarConversionError),
            ));
        }
    }
    if let (Some(nested), Some(variant)) = (&schema.nested, &names.nested) {
        let field_name = nested.name.to_string();
        let message = format!("failed to frame nested field `{field_name}`: {{0}}");
        variants.push(quote!(
            #[doc = "The nested schema failed structural framing."]
            #[error(#message)]
            #variant(#[source] E),
        ));
    }
    for (validator, variant) in schema.validators.iter().zip(&names.validators) {
        let validator_name = quote!(#validator).to_string();
        let validator_error =
            crate::validator::error_type(validator).expect("validated validator path");
        let message = format!("schema validator `{validator_name}` failed: {{0}}");
        variants.push(quote!(
            #[doc = "A schema-level validator rejected the framed view."]
            #[error(#message)]
            #variant(#[source] #validator_error),
        ));
    }
    variants.push(quote!(
        #[doc = "A child reported an extent outside its supplied input."]
        #[error("invalid child frame: {0}")]
        InvalidFrame(#[source] #runtime::InvalidFrameExtent),
    ));
    variants.push(quote!(
        #[doc = "Exact framing found trailing input."]
        #[error("exact framing failed: {0}")]
        Trailing(#[source] #runtime::TrailingBytes),
    ));

    let vis = &schema.vis;
    if schema.nested.is_some() {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<E: ::core::error::Error + 'static> {
                #(#variants)*
            }
        }
    } else {
        quote! {
            #[doc = "Typed structural failure generated for this schema."]
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error {
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
    child_frame: &syn::Ident,
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    let mut steps = Vec::new();
    for (index, (field, names)) in schema.fields.iter().zip(&names.fields).enumerate() {
        let bytes = private_ident(schema, &format!("scalar_{index}_bytes"));
        let raw = private_ident(schema, &format!("scalar_{index}_raw"));
        let decoded = private_ident(schema, &format!("scalar_{index}_value"));
        let expected_value = private_ident(schema, &format!("scalar_{index}_expected"));
        let offset = field.offset;
        let FieldKind::Scalar(scalar) = &field.kind;
        let width = scalar.width();
        let end = offset + width;
        let shortage = &names.shortage;
        steps.push(quote! {
            let #bytes: [u8; #width] = #frame_input
                .get(#offset..#end)
                .ok_or_else(|| #error::#shortage(#runtime::NeedMore {
                    offset: #frame_offset + #frame_input.len(),
                    additional_at_least: #end - #frame_input.len(),
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
                            offset: #frame_offset + #offset,
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
                            offset: #frame_offset + #offset,
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
    if let (Some(nested), Some(variant)) = (&schema.nested, &names.nested) {
        let ty = &nested.ty;
        let offset = nested.offset;
        steps.push(quote! {
            let #child_frame = <#ty as #runtime::WireView>::frame(
                &#frame_input[#offset..],
                #frame_offset + #offset,
            )
            .map_err(#error::#variant)?;
        });
    }
    steps
}

fn render_trait_methods(schema: &Schema, runtime: &TokenStream) -> Vec<TokenStream> {
    let mut methods = Vec::new();
    for field in &schema.fields {
        let name = &field.name;
        let FieldKind::Scalar(scalar) = &field.kind;
        let ty = value_type_tokens(scalar.value_type);
        methods.push(quote! {
            #[doc = concat!("Returns decoded field `", stringify!(#name), "`.")]
            fn #name(&self) -> #ty;
        });
    }
    if let Some(nested) = &schema.nested {
        let name = &nested.name;
        let ty = &nested.ty;
        let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
        methods.push(quote! {
            #[doc = concat!("Returns nested field `", stringify!(#name), "`.")]
            fn #name<#field_lifetime>(&#field_lifetime self)
                -> <#ty as #runtime::WireView>::View<#field_lifetime>;
        });
    }
    methods
}

fn render_view_methods(schema: &Schema, borrowed: bool, runtime: &TokenStream) -> Vec<TokenStream> {
    let mut methods = Vec::new();
    for field in &schema.fields {
        let name = &field.name;
        let offset = field.offset;
        let FieldKind::Scalar(scalar) = &field.kind;
        let width = scalar.width();
        let end = offset + width;
        let value_ty = value_type_tokens(scalar.value_type);
        let body = if matches!(scalar.value_type, ValueType::Char) && scalar.constant.is_none() {
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
            if scalar.value_type.is_converted() {
                let raw = private_ident(schema, "scalar_raw");
                let conversion = convert_from_validated_wire(scalar, &raw);
                quote! {
                    let #raw = #wire_ty::#decode(
                        self.as_bytes()[#offset..#end]
                            .try_into()
                            .expect("validated scalar field span"),
                    );
                    #conversion
                }
            } else {
                quote! {
                    #wire_ty::#decode(
                        self.as_bytes()[#offset..#end]
                            .try_into()
                            .expect("validated scalar field span"),
                    )
                }
            }
        };
        methods.push(quote! {
            #[inline(always)]
            fn #name(&self) -> #value_ty {
                #body
            }
        });
    }
    if let Some(nested) = &schema.nested {
        let name = &nested.name;
        let ty = &nested.ty;
        let offset = nested.offset;
        let field_lifetime = fresh_schema_lifetime(schema, &schema.generics, "field");
        let state = if borrowed {
            quote!(self.#name)
        } else {
            quote!(&self.descriptor.#name)
        };
        methods.push(quote! {
            #[allow(unsafe_code)]
            #[inline(always)]
            fn #name<#field_lifetime>(&#field_lifetime self)
                -> <#ty as #runtime::WireView>::View<#field_lifetime>
            {
                // SAFETY: framing produced this state for a child span of this exact length.
                unsafe {
                    <#ty as #runtime::WireView>::from_validated_parts(
                        &self.as_bytes()[#offset..],
                        #state,
                    )
                }
            }
        });
    }
    methods
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
        ValueType::Char => quote!(
            u32::try_from(#raw).ok().and_then(char::from_u32)
        ),
    }
}
