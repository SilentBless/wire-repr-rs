use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;
use syn::{
    Data, DeriveInput, Expr, Fields, GenericParam, Generics, Ident, Meta, Type, Visibility,
    parse_quote,
};

use super::model::{Endian, ScalarType};
use super::{
    fresh_lifetime, fresh_type_ident, from_bytes_method, scalar_type_tokens, to_bytes_method,
};

pub(super) fn is_bitfield(input: &DeriveInput) -> bool {
    matches!(input.data, Data::Struct(_))
        && has_wire_key(&input.attrs, "as")
        && matches!(&input.data, Data::Struct(data) if data.fields.iter().any(has_bit_attribute))
}

pub(super) fn render_view(input: DeriveInput, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let schema = BitfieldSchema::parse(input, "WireView")?;
    let vis = &schema.vis;
    let name = &schema.name;
    let view_trait = format_ident!("{}View", name.unraw());
    let view_error = format_ident!("{}ViewError", name.unraw());
    let view_impl = format_ident!("__WireRepr{}ViewImpl", name.unraw());
    let wire_ty = scalar_type_tokens(schema.representation);
    let width = schema.representation.width();
    let decode = from_bytes_method(schema.endian);
    let view_lifetime = fresh_lifetime(&schema.generics, "bitfield_view");
    let backing = fresh_type_ident(&schema.generics, "Backing");
    let holder = fresh_type_ident(&schema.generics, "Holder");
    let marker_type = fresh_type_ident(&schema.generics, "Marker");
    let (impl_generics, type_generics, where_clause) = schema.generics.split_for_impl();
    let self_type = quote!(#name #type_generics);
    let marker = quote!(fn() -> #self_type);
    let schema_arguments = generic_arguments(&schema.generics);
    let retained_type = quote!(#view_impl<#(#schema_arguments,)* #backing, #wire_ty, #marker>);
    let projected_type = quote!(
        #view_impl<#(#schema_arguments,)* &#view_lifetime [u8], &#view_lifetime #wire_ty, #marker>
    );

    let mut retained_generics = schema.generics.clone();
    retained_generics.params.push(parse_quote!(#backing));
    retained_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let (retained_impl, _, retained_where) = retained_generics.split_for_impl();
    let mut projected_generics = schema.generics.clone();
    projected_generics
        .params
        .insert(0, parse_quote!(#view_lifetime));
    let (projected_impl, _, projected_where) = projected_generics.split_for_impl();
    let mut common_generics = schema.generics.clone();
    common_generics.params.push(parse_quote!(#backing));
    common_generics.params.push(parse_quote!(#holder));
    common_generics.params.push(parse_quote!(#marker_type));
    common_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let (common_impl, common_types, common_where) = common_generics.split_for_impl();
    let trait_path = quote!(#view_trait #type_generics);

    let trait_methods = schema.fields.iter().map(|field| {
        let name = &field.name;
        let ty = &field.ty;
        quote!(fn #name(&self) -> #ty;)
    });
    let render_methods = |raw: TokenStream| {
        schema
            .fields
            .iter()
            .map(|field| {
                let name = &field.name;
                let ty = &field.ty;
                let start = field.start;
                let mask = field.value_mask();
                let value = if field.is_bool {
                    quote!(((#raw >> #start) & 1) != 0)
                } else {
                    quote!(((#raw >> #start) & (#mask as #wire_ty)) as #ty)
                };
                quote! {
                    #[inline(always)]
                    fn #name(&self) -> #ty {
                        #value
                    }
                }
            })
            .collect::<Vec<_>>()
    };
    let retained_methods = render_methods(quote!(self.raw));
    let projected_methods = render_methods(quote!(*self.raw));

    Ok(quote! {
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #view_error {
            #[error(transparent)]
            NeedMore(#[from] #runtime::NeedMore),
            #[error(transparent)]
            Trailing(#[from] #runtime::TrailingBytes),
        }

        #[doc(hidden)]
        #vis struct #view_impl #common_impl #common_where {
            input: #backing,
            represented_length: usize,
            raw: #holder,
            marker: ::core::marker::PhantomData<#marker_type>,
        }

        #[doc = "Exact-source view API generated for this nominal bitfield schema."]
        #vis trait #view_trait #impl_generics #where_clause {
            fn as_bytes(&self) -> &[u8];
            #(#trait_methods)*
        }

        impl #common_impl AsRef<[u8]> for #view_impl #common_types #common_where {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.input.as_ref()[..self.represented_length]
            }
        }

        impl #retained_impl #trait_path for #retained_type #retained_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
            #(#retained_methods)*
        }

        impl #projected_impl #trait_path for #projected_type #projected_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
            #(#projected_methods)*
        }

        impl #common_impl #runtime::ExactWire<#self_type>
            for #view_impl #common_types #common_where
        {
            #[inline(always)]
            fn as_wire_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
        }

        // SAFETY: framing checks the complete fixed-width integer span and retains only its decoded
        // value. Reconstruction pairs that value with an immutable span of the same exact width.
        #[allow(unsafe_code)]
        unsafe impl #impl_generics #runtime::WireView for #self_type #where_clause {
            type Error = #view_error;
            type State = #wire_ty;
            type View<#view_lifetime> = #projected_type;

            const FIXED_SIZE: Option<usize> = Some(#width);

            #[inline]
            fn frame(input: &[u8], offset: usize) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                if input.len() < #width {
                    return Err(#view_error::NeedMore(#runtime::NeedMore {
                        offset: offset.saturating_add(input.len()),
                        additional_at_least: #width - input.len(),
                    }));
                }
                let raw = #wire_ty::#decode(
                    input[..#width].try_into().expect("bitfield width checked"),
                );
                Ok(#runtime::Frame::new(raw, #width))
            }

            #[inline(always)]
            unsafe fn from_validated_parts<#view_lifetime>(
                input: &#view_lifetime [u8],
                raw: &#view_lifetime Self::State,
            ) -> Self::View<#view_lifetime> {
                #view_impl {
                    input,
                    represented_length: input.len(),
                    raw,
                    marker: ::core::marker::PhantomData,
                }
            }
        }

        impl #impl_generics #self_type #where_clause {
            #[inline]
            #vis fn view<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #view_error> {
                let bytes = input.as_ref();
                let frame = <Self as #runtime::WireView>::frame(bytes, 0)?;
                let (raw, consumed) = frame.into_parts();
                if consumed < bytes.len() {
                    return Err(#view_error::Trailing(#runtime::TrailingBytes {
                        offset: consumed,
                        trailing: bytes.len() - consumed,
                    }));
                }
                Ok(#view_impl {
                    input,
                    represented_length: consumed,
                    raw,
                    marker: ::core::marker::PhantomData,
                })
            }

            #[inline]
            #vis fn view_unchecked<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #view_error> {
                Self::view(input)
            }
        }
    })
}

pub(super) fn render_builder(
    input: DeriveInput,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let schema = BitfieldSchema::parse(input, "WireBuilder")?;
    let vis = &schema.vis;
    let name = &schema.name;
    let build_error = format_ident!("{}BuildError", name.unraw());
    let builder = format_ident!("{}Builder", name.unraw());
    let writer = format_ident!("{}Writer", name.unraw());
    let wire_ty = scalar_type_tokens(schema.representation);
    let encode = to_bytes_method(schema.endian);
    let output = fresh_type_ident(&schema.generics, "Output");
    let state_parameters = (0..schema.fields.len())
        .map(|index| format_ident!("__WireReprState{index}"))
        .collect::<Vec<_>>();
    let (impl_generics, type_generics, where_clause) = schema.generics.split_for_impl();
    let self_type = quote!(#name #type_generics);
    let marker = quote!(fn() -> #self_type);
    let schema_arguments = generic_arguments(&schema.generics);
    let unset_states = schema
        .fields
        .iter()
        .map(|_| quote!(#runtime::__private::Unset))
        .collect::<Vec<_>>();
    let set_states = schema
        .fields
        .iter()
        .map(|_| quote!(#runtime::__private::Set<()>))
        .collect::<Vec<_>>();
    let initial_builder = quote!(#builder<#(#schema_arguments,)* #(#unset_states),*>);
    let complete_builder = quote!(#builder<#(#schema_arguments,)* #(#set_states),*>);

    let mut builder_generics = schema.generics.clone();
    for state in &state_parameters {
        builder_generics.params.push(parse_quote!(#state));
    }
    let (builder_impl, _, builder_where) = builder_generics.split_for_impl();
    let mut writer_generics = builder_generics.clone();
    writer_generics.params.push(parse_quote!(#output));
    writer_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#output: #runtime::Output));
    let (writer_impl, _, writer_where) = writer_generics.split_for_impl();

    let builder_setters = schema.fields.iter().enumerate().map(|(target, field)| {
        render_setter(
            &schema,
            field,
            target,
            &state_parameters,
            None,
            runtime,
            &builder,
        )
    });
    let writer_setters = schema.fields.iter().enumerate().map(|(target, field)| {
        render_setter(
            &schema,
            field,
            target,
            &state_parameters,
            Some(&output),
            runtime,
            &writer,
        )
    });

    let complete_generics = schema.generics.clone();
    let (complete_impl, _, complete_where) = complete_generics.split_for_impl();
    let mut finish_generics = schema.generics.clone();
    finish_generics.params.push(parse_quote!(#output));
    finish_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#output: #runtime::Output));
    let (finish_impl, _, finish_where) = finish_generics.split_for_impl();
    let complete_writer = quote!(#writer<#(#schema_arguments,)* #(#set_states,)* #output>);

    Ok(quote! {
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #build_error {
            #[error("value for bitfield `{field}` does not fit its declared range")]
            OutOfRange { field: &'static str },
        }

        #vis struct #builder #builder_impl #builder_where {
            raw: #wire_ty,
            invalid: Option<&'static str>,
            marker: ::core::marker::PhantomData<(#marker, #(#state_parameters),*)>,
        }

        #vis struct #writer #writer_impl #writer_where {
            output: #runtime::Writer<#output>,
            raw: #wire_ty,
            marker: ::core::marker::PhantomData<(#marker, #(#state_parameters),*)>,
        }

        #(#builder_setters)*
        #(#writer_setters)*

        impl #complete_impl #runtime::WireWrite<#complete_builder> for #self_type #complete_where {
            type Error = #build_error;

            #[inline(always)]
            fn write<__WireReprOutput: #runtime::Output>(
                value: #complete_builder,
                output: &mut #runtime::ChildWriter<'_, __WireReprOutput>,
            ) -> Result<(), #runtime::WriteError<Self::Error, __WireReprOutput::GrowError>> {
                if let Some(field) = value.invalid {
                    return Err(#runtime::WriteError::Schema(
                        #build_error::OutOfRange { field },
                    ));
                }
                output.write(&value.raw.#encode())?;
                Ok(())
            }
        }

        impl #finish_impl #complete_writer #finish_where {
            #[inline]
            #vis fn finish(mut self) -> Result<
                #runtime::Written<#output>,
                #runtime::OutputError<<#output as #runtime::Output>::GrowError>,
            > {
                self.output.write(&self.raw.#encode())?;
                Ok(self.output.finish())
            }
        }

        impl #impl_generics #runtime::WireBuilder for #self_type #where_clause {
            const FIXED_SIZE: Option<usize> = Some(::core::mem::size_of::<#wire_ty>());
            type Builder = #initial_builder;

            #[inline(always)]
            fn builder() -> Self::Builder {
                #builder {
                    raw: 0,
                    invalid: None,
                    marker: ::core::marker::PhantomData,
                }
            }
        }

        impl #impl_generics #self_type #where_clause {
            #[inline(always)]
            #vis fn builder<#output: #runtime::Output>(output: #output) -> #writer<
                #(#schema_arguments,)*
                #(#unset_states,)*
                #output
            > {
                #writer {
                    output: #runtime::Writer::new(output),
                    raw: 0,
                    marker: ::core::marker::PhantomData,
                }
            }
        }
    })
}

fn render_setter(
    schema: &BitfieldSchema,
    field: &BitField,
    target: usize,
    state_parameters: &[Ident],
    output: Option<&Ident>,
    runtime: &TokenStream,
    owner: &Ident,
) -> TokenStream {
    let build_error = format_ident!("{}BuildError", schema.name.unraw());
    let name = &field.name;
    let ty = &field.ty;
    let wire_ty = scalar_type_tokens(schema.representation);
    let start = field.start;
    let value_mask = field.value_mask();
    let shifted_mask = field.shifted_mask();
    let schema_arguments = generic_arguments(&schema.generics);
    let input_states = state_parameters
        .iter()
        .enumerate()
        .map(|(index, state)| {
            if index == target {
                quote!(#runtime::__private::Unset)
            } else {
                quote!(#state)
            }
        })
        .collect::<Vec<_>>();
    let output_states = state_parameters
        .iter()
        .enumerate()
        .map(|(index, state)| {
            if index == target {
                quote!(#runtime::__private::Set<()>)
            } else {
                quote!(#state)
            }
        })
        .collect::<Vec<_>>();
    let mut setter_generics = schema.generics.clone();
    for (index, state) in state_parameters.iter().enumerate() {
        if index != target {
            setter_generics.params.push(parse_quote!(#state));
        }
    }
    if let Some(output) = output {
        setter_generics.params.push(parse_quote!(#output));
        setter_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#output: #runtime::Output));
    }
    let (setter_impl, _, setter_where) = setter_generics.split_for_impl();
    let owner_input = if let Some(output) = output {
        quote!(#owner<#(#schema_arguments,)* #(#input_states,)* #output>)
    } else {
        quote!(#owner<#(#schema_arguments,)* #(#input_states),*>)
    };
    let owner_output = if let Some(output) = output {
        quote!(#owner<#(#schema_arguments,)* #(#output_states,)* #output>)
    } else {
        quote!(#owner<#(#schema_arguments,)* #(#output_states),*>)
    };
    let raw_value = if field.is_bool {
        quote!(if value { 1 as #wire_ty } else { 0 as #wire_ty })
    } else {
        quote!(value as #wire_ty)
    };
    let range_check = (!field.is_bool).then(|| {
        quote! {
            if (value as u128) > #value_mask {
                return Err(#build_error::OutOfRange { field: stringify!(#name) });
            }
        }
    });
    if output.is_some() {
        quote! {
            impl #setter_impl #owner_input #setter_where {
                #[inline(always)]
                pub fn #name(self, value: #ty) -> Result<#owner_output, #build_error> {
                    #range_check
                    let raw_value = #raw_value;
                    let raw = (self.raw & !(#shifted_mask as #wire_ty))
                        | ((raw_value << #start) & (#shifted_mask as #wire_ty));
                    Ok(#owner {
                        output: self.output,
                        raw,
                        marker: ::core::marker::PhantomData,
                    })
                }
            }
        }
    } else {
        let invalid = if field.is_bool {
            quote!(self.invalid)
        } else {
            quote! {
                if (value as u128) > #value_mask {
                    Some(stringify!(#name))
                } else {
                    self.invalid
                }
            }
        };
        quote! {
            impl #setter_impl #owner_input #setter_where {
                #[inline(always)]
                pub fn #name(self, value: #ty) -> #owner_output {
                    let raw_value = #raw_value;
                    let raw = (self.raw & !(#shifted_mask as #wire_ty))
                        | ((raw_value << #start) & (#shifted_mask as #wire_ty));
                    #owner {
                        raw,
                        invalid: #invalid,
                        marker: ::core::marker::PhantomData,
                    }
                }
            }
        }
    }
}

struct BitfieldSchema {
    vis: Visibility,
    name: Ident,
    generics: Generics,
    representation: ScalarType,
    endian: Endian,
    fields: Vec<BitField>,
}

struct BitField {
    name: Ident,
    ty: Type,
    start: u32,
    end: u32,
    is_bool: bool,
}

impl BitField {
    fn width(&self) -> u32 {
        self.end - self.start + 1
    }
    fn value_mask(&self) -> u128 {
        if self.width() == 128 {
            u128::MAX
        } else {
            (1u128 << self.width()) - 1
        }
    }
    fn shifted_mask(&self) -> u128 {
        self.value_mask() << self.start
    }
}

impl BitfieldSchema {
    fn parse(input: DeriveInput, owner: &str) -> syn::Result<Self> {
        let (representation_type, declared_endian) = parse_item_attributes(&input.attrs, owner)?;
        let representation = scalar_from_type(&representation_type)?;
        if !representation.is_unsigned_integer() {
            return Err(syn::Error::new_spanned(
                &representation_type,
                "bitfield representation must be an unsigned fixed-width integer",
            ));
        }
        let endian =
            super::model::scalar_endian(representation, declared_endian, &representation_type)?;
        let Data::Struct(data) = input.data else {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "nominal bitfield requires a struct",
            ));
        };
        let Fields::Named(fields) = data.fields else {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "nominal bitfield requires named logical fields",
            ));
        };
        let mut used = 0u128;
        let mut parsed = Vec::with_capacity(fields.named.len());
        let representation_bits = (representation.width() * 8) as u32;
        for field in fields.named {
            let name = field.ident.expect("named field");
            let (start, end) = parse_field_attributes(&field.attrs)?;
            if end >= representation_bits {
                return Err(syn::Error::new_spanned(
                    &name,
                    "bit range exceeds the physical representation",
                ));
            }
            let width = end - start + 1;
            let logical = scalar_from_type(&field.ty).ok();
            let is_bool = is_bool(&field.ty);
            if width == 1 && !is_bool && logical.is_none() {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "single-bit fields require bool or an unsigned integer",
                ));
            }
            if width > 1 && (is_bool || logical.is_none_or(|ty| !ty.is_unsigned_integer())) {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "multi-bit ranges require an unsigned integer field",
                ));
            }
            if let Some(logical) = logical
                && width > (logical.width() * 8) as u32
            {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "logical field is narrower than its declared bit range",
                ));
            }
            let mask = if width == 128 {
                u128::MAX
            } else {
                ((1u128 << width) - 1) << start
            };
            if used & mask != 0 {
                return Err(syn::Error::new_spanned(
                    &name,
                    "bitfield ranges cannot overlap",
                ));
            }
            used |= mask;
            parsed.push(BitField {
                name,
                ty: field.ty,
                start,
                end,
                is_bool,
            });
        }
        if parsed.is_empty() {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "nominal bitfield requires at least one logical field",
            ));
        }
        Ok(Self {
            vis: input.vis,
            name: input.ident,
            generics: input.generics,
            representation,
            endian,
            fields: parsed,
        })
    }
}

fn has_bit_attribute(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attribute| {
        attribute.path().is_ident("wire") && {
            let tokens = attribute.meta.to_token_stream().to_string();
            tokens.contains("bit")
        }
    })
}

fn has_wire_key(attributes: &[syn::Attribute], key: &str) -> bool {
    attributes.iter().any(|attribute| {
        let Meta::List(list) = &attribute.meta else {
            return false;
        };
        attribute.path().is_ident("wire")
            && list
                .tokens
                .clone()
                .into_iter()
                .any(|token| matches!(token, proc_macro2::TokenTree::Ident(ident) if ident == key))
    })
}
fn parse_item_attributes(
    attributes: &[syn::Attribute],

    owner: &str,
) -> syn::Result<(Type, Option<Endian>)> {
    let mut representation = None;
    let mut endian = None;
    for attribute in attributes {
        if !attribute.path().is_ident("wire") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("as") {
                if representation.is_some() {
                    return Err(meta.error("duplicate `as` bitfield representation"));
                }
                representation = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("le") || meta.path.is_ident("be") {
                if endian.is_some() {
                    return Err(meta.error("duplicate or conflicting endian attribute"));
                }
                endian = Some(if meta.path.is_ident("le") {
                    Endian::Little
                } else {
                    Endian::Big
                });
                return Ok(());
            }
            Err(meta.error(format!("unsupported {owner} bitfield attribute")))
        })?;
    }
    let representation = representation.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "nominal bitfield requires #[wire(as = unsigned_type)]",
        )
    })?;
    Ok((representation, endian))
}

fn parse_field_attributes(attributes: &[syn::Attribute]) -> syn::Result<(u32, u32)> {
    let mut range = None;
    for attribute in attributes {
        if !attribute.path().is_ident("wire") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("bit") {
                if range.is_some() {
                    return Err(meta.error("duplicate or conflicting bit range"));
                }
                let value: syn::LitInt = meta.value()?.parse()?;
                let bit = value.base10_parse()?;
                range = Some((bit, bit));
                return Ok(());
            }
            if meta.path.is_ident("bits") {
                if range.is_some() {
                    return Err(meta.error("duplicate or conflicting bit range"));
                }
                let value: Expr = meta.value()?.parse()?;
                let Expr::Range(range_expr) = value else {
                    return Err(meta.error("bits requires an inclusive integer range"));
                };
                let Some(start) = range_expr.start else {
                    return Err(meta.error("bits range requires a start"));
                };
                let Some(end) = range_expr.end else {
                    return Err(meta.error("bits range requires an end"));
                };
                if !matches!(range_expr.limits, syn::RangeLimits::Closed(_)) {
                    return Err(meta.error("bits range must be inclusive"));
                }
                let Expr::Lit(start) = *start else {
                    return Err(meta.error("bits range bounds must be integer literals"));
                };
                let Expr::Lit(end) = *end else {
                    return Err(meta.error("bits range bounds must be integer literals"));
                };
                let syn::Lit::Int(start) = start.lit else {
                    return Err(meta.error("bits range bounds must be integer literals"));
                };
                let syn::Lit::Int(end) = end.lit else {
                    return Err(meta.error("bits range bounds must be integer literals"));
                };
                range = Some((start.base10_parse()?, end.base10_parse()?));
                return Ok(());
            }
            Err(meta.error("unsupported nominal bitfield field attribute"))
        })?;
    }
    let (start, end) = range.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "bitfield field requires #[wire(bit = N)] or #[wire(bits = A..=B)]",
        )
    })?;
    if start > end {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "bit range start must not exceed end",
        ));
    }
    Ok((start, end))
}

fn scalar_from_type(ty: &Type) -> syn::Result<ScalarType> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "expected a primitive integer type",
        ));
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            ty,
            "expected a primitive integer type",
        ));
    }
    let name = path.path.segments[0].ident.unraw().to_string();
    ScalarType::from_name(&name)
        .ok_or_else(|| syn::Error::new_spanned(ty, "expected a primitive integer type"))
}

fn is_bool(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident("bool"))
}

fn generic_arguments(generics: &Generics) -> Vec<TokenStream> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => {
                let lifetime = &parameter.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
            GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
        })
        .collect()
}
