use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, TypeParam, parse_quote};

use super::builder::{
    Slot, SlotKind, choice_start_ident, choice_trait_ident, convert_to_wire, slots,
    unique_build_variant,
};
use super::model::{DynamicExtent, FieldKind, LayoutOffset, Position, Schema, SizeTerm};
use super::{
    builder_offset, builder_optional_size, fresh_field_ident, fresh_type_ident, pascal,
    private_ident, scalar_type_tokens, to_bytes_method, value_type_tokens,
};

pub(super) fn render(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    let vis = &schema.vis;
    let name = &schema.name;
    let writer_name = format_ident!("{}Writer", name.unraw());
    let slots = slots(schema);
    let marker = fresh_field_ident(schema, "writer_schema");
    let output = fresh_type_ident(&schema.generics, "Output");
    let original_arguments = generic_arguments(&schema.generics);
    let state_markers = slots.iter().map(|slot| &slot.state).collect::<Vec<_>>();

    let mut writer_generics = schema.generics.clone();
    for parameter in &mut writer_generics.params {
        match parameter {
            GenericParam::Type(parameter) => parameter.default = None,
            GenericParam::Const(parameter) => parameter.default = None,
            GenericParam::Lifetime(_) => {}
        }
    }
    let mut output_parameter = TypeParam::from(output.clone());
    output_parameter.bounds.push(parse_quote!(#runtime::Output));
    writer_generics
        .params
        .push(GenericParam::Type(output_parameter));
    for slot in &slots {
        let mut parameter = TypeParam::from(slot.state.clone());
        parameter.default = Some(parse_quote!(#runtime::__private::Unset));
        writer_generics.params.push(GenericParam::Type(parameter));
    }
    let writer_declaration_generics = writer_generics.clone();
    let initial_states = slots
        .iter()
        .map(|_| quote!(#runtime::__private::Unset))
        .collect::<Vec<_>>();
    let initial_writer = writer_type(&writer_name, &original_arguments, &output, &initial_states);

    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let setters = render_setters(
        schema,
        &writer_name,
        &marker,
        &output,
        &original_arguments,
        &slots,
        runtime,
    );
    let finish = render_finish(
        schema,
        &writer_name,
        &output,
        &original_arguments,
        &slots,
        runtime,
    );

    quote! {
        #[doc(hidden)]
        #vis struct #writer_name #writer_declaration_generics {
            writer: #runtime::Writer<#output>,
            #marker: ::core::marker::PhantomData<
                fn() -> (#self_type, #(#state_markers,)*)
            >,
        }

        impl #schema_impl #self_type #schema_where {
            /// Creates a progressive typestate writer over `output`.
            #[inline(always)]
            #vis fn builder<#output: #runtime::Output>(output: #output) -> #initial_writer {
                #writer_name {
                    writer: #runtime::Writer::new(output),
                    #marker: ::core::marker::PhantomData,
                }
            }
        }

        #(#setters)*
        #finish
    }
}

fn render_setters(
    schema: &Schema,
    writer_name: &syn::Ident,
    marker: &syn::Ident,
    output: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> Vec<TokenStream> {
    let vis = &schema.vis;
    let error_name = format_ident!("{}WriteError", schema.name.unraw());
    let (conversion_names, nested_names) = writer_error_names(schema);
    let has_conversions = conversion_names.iter().any(Option::is_some);
    let mut rendered = Vec::new();

    for (target_index, target) in slots.iter().enumerate() {
        let physical_index = schema
            .fields
            .iter()
            .position(|field| field.name == target.field)
            .expect("writer slot has one physical field");
        let physical = &schema.fields[physical_index];
        let mut impl_generics = schema.generics.clone();
        let mut output_parameter = TypeParam::from(output.clone());
        output_parameter.bounds.push(parse_quote!(#runtime::Output));
        impl_generics
            .params
            .push(GenericParam::Type(output_parameter));
        add_offset_bounds(&mut impl_generics, &physical.offset, runtime);

        let build_fn = fresh_type_ident(&impl_generics, "BuildFn");
        let child_builder = fresh_type_ident(&impl_generics, "ChildBuilder");
        let raw_bytes = fresh_type_ident(&impl_generics, "RawBytes");
        let item_error = fresh_type_ident(&impl_generics, "ItemError");
        let array_lifetime = super::fresh_lifetime(&impl_generics, "array");
        let mut current_states = Vec::with_capacity(slots.len());
        let mut returned_states = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            if index == target_index {
                current_states.push(quote!(#runtime::__private::Unset));
                returned_states.push(match &target.kind {
                    SlotKind::Value(ty) => quote!(#runtime::__private::Set<#ty>),
                    SlotKind::RawBytes => quote!(#runtime::__private::Set<#raw_bytes>),
                    SlotKind::Array(_) => quote!(#runtime::__private::Set<()>),
                    SlotKind::Choice(flag) => {
                        let final_value = super::builder::choice_final_ident(schema, flag);
                        quote!(#runtime::__private::Set<#final_value>)
                    }
                    SlotKind::Nested(_) => quote!(#runtime::__private::Set<#child_builder>),
                });
            } else {
                let state = &slot.state;
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(state.clone())));
                current_states.push(quote!(#state));
                returned_states.push(quote!(#state));
            }
        }
        add_geometry_state_bounds(
            schema,
            physical,
            slots,
            target_index,
            &mut impl_generics,
            runtime,
        );
        let current_writer = writer_type(writer_name, original_arguments, output, &current_states);
        let returned_writer =
            writer_type(writer_name, original_arguments, output, &returned_states);
        let field = &target.field;
        let offset = builder_offset(&physical.offset, runtime);
        let offset_local = private_ident(schema, &format!("{}_offset", field.unraw()));
        let value = private_ident(schema, &format!("{}_value", field.unraw()));
        let field_name = field.unraw().to_string();
        let layout_failure = quote! {
            #runtime::WriteError::Schema(
                #error_name::Layout(#runtime::LayoutError { field: #field_name }),
            )
        };
        let offset_binding = if schema.has_explicit_geometry() {
            render_writer_start(schema, physical, &offset_local, &error_name, runtime)
        } else if schema.layout_can_fail() {
            quote!(let #offset_local = #offset.ok_or_else(|| #layout_failure)?;)
        } else {
            quote!(
                let #offset_local = #offset.expect("validated fixed layout offset");
            )
        };
        let pending_constants = if schema.has_explicit_geometry() {
            render_pending_constants(
                schema,
                physical_index,
                slots,
                &conversion_names,
                &error_name,
                runtime,
            )
        } else {
            TokenStream::new()
        };
        let assignments = quote! {
            #writer_name {
                writer: self.writer,
                #marker: ::core::marker::PhantomData,
            }
        };

        match (&target.kind, &physical.kind) {
            (SlotKind::Value(ty), FieldKind::Scalar(scalar)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let wire_ty = scalar_type_tokens(scalar.wire_type);
                let encode = to_bytes_method(scalar.endian);
                let encoded = private_ident(schema, &format!("{}_encoded", field.unraw()));
                let write = if scalar.value_type.is_converted() {
                    let conversion = convert_to_wire(scalar, &value, &wire_ty);
                    let variant = conversion_names[physical_index]
                        .as_ref()
                        .expect("converted field has one error variant");
                    quote! {
                        let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                            #runtime::WriteError::Schema(
                                #error_name::#variant(#runtime::ScalarBuildConversionError {
                                    from: stringify!(#ty),
                                    to: stringify!(#wire_ty),
                                }),
                            )
                        })?;
                        self.writer.write_at(#offset_local, &#encoded.#encode())?;
                    }
                } else {
                    quote! {
                        let #encoded: #wire_ty = #value;
                        self.writer.write_at(#offset_local, &#encoded.#encode())?;
                    }
                };
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field(
                            mut self,
                            #value: #ty,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        > {
                            #pending_constants
                            #offset_binding
                            #write
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Value(ty), FieldKind::Bytes(_)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes fixed byte-array field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field(
                            mut self,
                            #value: #ty,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        > {
                            #pending_constants
                            #offset_binding
                            self.writer.write_at(#offset_local, &#value)?;
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::RawBytes, FieldKind::RawBytes(raw)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let patch = match &raw.extent {
                    DynamicExtent::Bounded(controller) => render_length_patch(
                        schema,
                        controller,
                        field,
                        quote!(bytes.len()),
                        &error_name,
                        runtime,
                    ),
                    DynamicExtent::Rest => TokenStream::new(),
                };
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes raw byte field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field<#raw_bytes>(
                            mut self,
                            #value: #raw_bytes,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        >
                        where
                            #raw_bytes: AsRef<[u8]>,
                        {
                            #pending_constants
                            #offset_binding
                            let bytes = #value.as_ref();
                            #patch
                            self.writer.write_at(#offset_local, bytes)?;
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Array(item), FieldKind::Array(array)) => {
                let item_error_type = quote!(#item_error);
                let schema_error = writer_error_type(
                    schema,
                    &error_name,
                    has_conversions,
                    Some((field, item_error_type)),
                    runtime,
                );
                let variant = nested_names[physical_index]
                    .as_ref()
                    .expect("array field has one error variant");
                let patch = render_count_patch(
                    schema,
                    &array.controller,
                    quote!(count),
                    &error_name,
                    runtime,
                );
                let array_writer =
                    private_ident(schema, &format!("{}_array_writer", field.unraw()));
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Streams counted array field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field<#build_fn, #item_error>(
                            mut self,
                            build: #build_fn,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        >
                        where
                            #item_error: ::core::error::Error + 'static,
                            #build_fn: for<#array_lifetime> FnOnce(
                                #runtime::ArrayWriter<#array_lifetime, #output, #item>,
                            ) -> Result<
                                #runtime::ArrayWriter<
                                    #array_lifetime,
                                    #output,
                                    #item,
                                >,
                                #runtime::WriteError<
                                    #item_error,
                                    <#output as #runtime::Output>::GrowError,
                                >,
                            >,
                        {
                            #pending_constants
                            #offset_binding
                            let count = {
                                let #array_writer =
                                    #runtime::ArrayWriter::<#output, #item>::new(
                                        &mut self.writer,
                                    );
                                let #array_writer = build(#array_writer).map_err(|error| {
                                    match error {
                                        #runtime::WriteError::Schema(error) => {
                                            #runtime::WriteError::Schema(
                                                #error_name::#variant(error),
                                            )
                                        }
                                        #runtime::WriteError::Output(error) => {
                                            #runtime::WriteError::Output(error)
                                        }
                                    }
                                })?;
                                #array_writer.count()
                            };
                            #patch
                            let value = ();
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Choice(flag), FieldKind::Flag(_)) => {
                let schema_error =
                    writer_error_type(schema, &error_name, has_conversions, None, runtime);
                let start = choice_start_ident(schema, flag);
                let final_value = super::builder::choice_final_ident(schema, flag);
                let choice_trait = choice_trait_ident(schema, flag);
                let patch =
                    render_presence_patch(schema, flag, quote!(present), &error_name, runtime);
                let choice_writer =
                    private_ident(schema, &format!("{}_choice_writer", flag.unraw()));
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Chooses conditional group `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field<#build_fn>(
                            mut self,
                            choose: #build_fn,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        >
                        where
                            #build_fn: FnOnce(#start) -> #final_value,
                        {
                            #pending_constants
                            #offset_binding
                            let choice = choose(#start);
                            let present =
                                <#final_value as #choice_trait>::is_present(&choice);
                            #patch
                            {
                                let mut #choice_writer =
                                    self.writer.child_at(#offset_local)?;
                                <#final_value as #choice_trait>::write(
                                    choice,
                                    &mut #choice_writer,
                                )?;
                                #choice_writer.finish()?;
                            }
                            Ok(#assignments)
                        }
                    }
                });
            }
            (SlotKind::Nested(ty), FieldKind::Nested(nested)) => {
                let child_error = quote!(<#ty as #runtime::WireWrite<#child_builder>>::Error);
                let schema_error = writer_error_type(
                    schema,
                    &error_name,
                    has_conversions,
                    Some((field, child_error)),
                    runtime,
                );
                let variant = nested_names[physical_index]
                    .as_ref()
                    .expect("nested field has one error variant");
                let child_writer = private_ident(schema, &format!("{}_writer", field.unraw()));
                let create_child = if nested.extent.is_some() {
                    quote!(self.writer.child_at(#offset_local)?)
                } else if nested.terminal {
                    quote! {
                        match <#ty as #runtime::WireBuilder>::FIXED_SIZE {
                            Some(size) => self.writer.fixed_child_at(#offset_local, size)?,
                            None => self.writer.child_at(#offset_local)?,
                        }
                    }
                } else {
                    quote! {{
                        let size = <#ty as #runtime::WireBuilder>::FIXED_SIZE
                            .ok_or_else(|| #layout_failure)?;
                        self.writer.fixed_child_at(#offset_local, size)?
                    }}
                };
                let patch = nested
                    .extent
                    .as_ref()
                    .map(|controller| {
                        render_length_patch(
                            schema,
                            controller,
                            field,
                            quote!(child_length),
                            &error_name,
                            runtime,
                        )
                    })
                    .unwrap_or_default();
                let (impl_params, _, impl_where) = impl_generics.split_for_impl();
                rendered.push(quote! {
                    impl #impl_params #current_writer #impl_where {
                        #[doc = concat!("Writes nested field `", stringify!(#field), "`.")]
                        #[inline]
                        #vis fn #field<#build_fn, #child_builder>(
                            mut self,
                            build: #build_fn,
                        ) -> Result<
                            #returned_writer,
                            #runtime::WriteError<
                                #schema_error,
                                <#output as #runtime::Output>::GrowError,
                            >,
                        >
                        where
                            #ty: #runtime::WireBuilder + #runtime::WireWrite<#child_builder>,
                            #build_fn: FnOnce(
                                <#ty as #runtime::WireBuilder>::Builder,
                            ) -> #child_builder,
                        {
                            #pending_constants
                            #offset_binding
                            let child_length = {
                                let mut #child_writer = #create_child;
                                let value = build(<#ty as #runtime::WireBuilder>::builder());
                                <#ty as #runtime::WireWrite<#child_builder>>::write(
                                    value,
                                    &mut #child_writer,
                                )
                                .map_err(|error| match error {
                                    #runtime::WriteError::Schema(error) => {
                                        #runtime::WriteError::Schema(
                                            #error_name::#variant(error),
                                        )
                                    }
                                    #runtime::WriteError::Output(error) => {
                                        #runtime::WriteError::Output(error)
                                    }
                                })?;
                                let child_end = #child_writer.position();
                                #child_writer.finish()?;
                                child_end.checked_sub(#offset_local)
                                    .ok_or_else(|| #layout_failure)?
                            };
                            #patch
                            Ok(#assignments)
                        }
                    }
                });
            }
            _ => unreachable!("slot kind must match its physical field"),
        }
    }
    rendered
}

fn render_finish(
    schema: &Schema,
    writer_name: &syn::Ident,
    output: &syn::Ident,
    original_arguments: &[TokenStream],
    slots: &[Slot],
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let error_name = format_ident!("{}WriteError", schema.name.unraw());
    let (conversion_names, _) = writer_error_names(schema);
    let has_conversions = conversion_names.iter().any(Option::is_some);
    let mut impl_generics = schema.generics.clone();
    let mut output_parameter = TypeParam::from(output.clone());
    output_parameter.bounds.push(parse_quote!(#runtime::Output));
    impl_generics
        .params
        .push(GenericParam::Type(output_parameter));

    let mut complete_states = Vec::with_capacity(slots.len());
    let mut child_errors = Vec::new();
    for slot in slots {
        match &slot.kind {
            SlotKind::Value(ty) => {
                complete_states.push(quote!(#runtime::__private::Set<#ty>));
            }
            SlotKind::RawBytes => {
                let bytes =
                    fresh_type_ident(&impl_generics, &format!("{}Bytes", pascal(&slot.field)));
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(bytes.clone())));
                impl_generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#bytes: AsRef<[u8]>));
                complete_states.push(quote!(#runtime::__private::Set<#bytes>));
            }
            SlotKind::Array(_) => {
                complete_states.push(quote!(#runtime::__private::Set<()>));
            }
            SlotKind::Choice(flag) => {
                let final_value = super::builder::choice_final_ident(schema, flag);
                complete_states.push(quote!(#runtime::__private::Set<#final_value>));
            }
            SlotKind::Nested(ty) => {
                let child =
                    fresh_type_ident(&impl_generics, &format!("{}Builder", pascal(&slot.field)));
                impl_generics
                    .params
                    .push(GenericParam::Type(TypeParam::from(child.clone())));
                impl_generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: #runtime::WireWrite<#child>));
                complete_states.push(quote!(#runtime::__private::Set<#child>));
                child_errors.push((
                    slot.field.clone(),
                    quote!(<#ty as #runtime::WireWrite<#child>>::Error),
                ));
            }
        }
    }
    let last_slot_physical = slots.last().and_then(|slot| {
        schema
            .fields
            .iter()
            .position(|field| field.name == slot.field)
    });
    for field in &schema.fields {
        add_offset_bounds(&mut impl_generics, &field.offset, runtime);
    }
    let complete_writer = writer_type(writer_name, original_arguments, output, &complete_states);
    let schema_error =
        writer_error_type_from_all(schema, &error_name, has_conversions, &child_errors, runtime);

    let constant_writes = schema
        .fields
        .iter()
        .zip(&conversion_names)
        .enumerate()
        .filter_map(|(index, (field, conversion_variant))| {
            if schema.has_explicit_geometry()
                && last_slot_physical.is_some_and(|last| index <= last)
            {
                return None;
            }
            let constant = field.kind.constant()?;
            let offset = builder_offset(&field.offset, runtime);
            let offset_local = private_ident(schema, &format!("constant_{index}_offset"));
            let field_name = field.name.unraw().to_string();
            let layout_failure = quote! {
                #runtime::WriteError::Schema(
                    #error_name::Layout(#runtime::LayoutError { field: #field_name }),
                )
            };
            let offset_binding = if schema.has_explicit_geometry() {
                render_writer_start(schema, field, &offset_local, &error_name, runtime)
            } else if schema.layout_can_fail() {
                quote!(let #offset_local = #offset.ok_or_else(|| #layout_failure)?;)
            } else {
                quote!(
                    let #offset_local = #offset.expect("validated fixed layout offset");
                )
            };
            Some(match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let value_ty = value_type_tokens(scalar.value_type);
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    if let Some(variant) = conversion_variant {
                        let semantic = private_ident(schema, &format!("constant_{index}_semantic"));
                        let encoded = private_ident(schema, &format!("constant_{index}_encoded"));
                        let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                        quote! {
                            #offset_binding
                            let #semantic: #value_ty = #constant;
                            let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error_name::#variant(
                                        #runtime::ScalarBuildConversionError {
                                            from: stringify!(#value_ty),
                                            to: stringify!(#wire_ty),
                                        },
                                    ),
                                )
                            })?;
                            self.writer.write_at(#offset_local, &#encoded.#encode())?;
                        }
                    } else {
                        quote! {
                            #offset_binding
                            let encoded: #wire_ty = #constant;
                            self.writer.write_at(#offset_local, &encoded.#encode())?;
                        }
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    let value = private_ident(schema, &format!("constant_{index}_bytes"));
                    quote! {
                        #offset_binding
                        let #value: #ty = #constant;
                        self.writer.write_at(#offset_local, &#value)?;
                    }
                }
                FieldKind::RawBytes(_)
                | FieldKind::Array(_)
                | FieldKind::Flag(_)
                | FieldKind::Nested(_) => unreachable!(),
            })
        });
    let ensure_total = if schema.has_explicit_geometry() {
        TokenStream::new()
    } else if schema
        .fields
        .last()
        .is_some_and(|field| !matches!(field.kind, FieldKind::Nested(_)))
    {
        let total = builder_optional_size(schema, runtime);
        if schema.layout_can_fail() {
            quote! {
                let total = #total.ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error_name::Layout(#runtime::LayoutError { field: "<finish>" }),
                    )
                })?;
                self.writer.ensure(total, total)?;
            }
        } else {
            quote! {
                let total = #total.expect("validated fixed layout width");
                self.writer.ensure(total, total)?;
            }
        }
    } else {
        TokenStream::new()
    };
    let (impl_params, _, impl_where) = impl_generics.split_for_impl();

    quote! {
        impl #impl_params #complete_writer #impl_where {
            /// Finishes the representation and returns its exact output range.
            #[inline]
            #vis fn finish(
                mut self,
            ) -> Result<
                #runtime::Written<#output>,
                #runtime::WriteError<
                    #schema_error,
                    <#output as #runtime::Output>::GrowError,
                >,
            > {
                #ensure_total
                #(#constant_writes)*
                Ok(self.writer.finish())
            }
        }
    }
}

fn writer_error_names(schema: &Schema) -> (Vec<Option<syn::Ident>>, Vec<Option<syn::Ident>>) {
    let mut used = BTreeSet::from(["Layout".to_owned(), "LengthConflict".to_owned()]);
    let conversions = schema
        .fields
        .iter()
        .map(|field| match &field.kind {
            FieldKind::Scalar(scalar) if scalar.value_type.is_converted() => Some(
                unique_build_variant(&mut used, &format!("{}Value", pascal(&field.name))),
            ),
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Flag(_)
            | FieldKind::Nested(_) => None,
        })
        .collect::<Vec<_>>();
    let nested = schema
        .fields
        .iter()
        .map(|field| {
            matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_))
                .then(|| unique_build_variant(&mut used, &pascal(&field.name).to_string()))
        })
        .collect();
    (conversions, nested)
}

fn writer_error_type(
    schema: &Schema,
    error: &syn::Ident,
    has_conversions: bool,
    target: Option<(&syn::Ident, TokenStream)>,
    _runtime: &TokenStream,
) -> TokenStream {
    let child_errors = schema
        .fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_)))
        .map(|field| {
            if target
                .as_ref()
                .is_some_and(|(name, _)| **name == field.name)
            {
                target.as_ref().expect("checked target").1.clone()
            } else {
                quote!(::core::convert::Infallible)
            }
        })
        .collect::<Vec<_>>();
    if child_errors.is_empty() {
        if has_conversions || schema.layout_can_fail() {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#child_errors),*>)
    }
}

fn writer_error_type_from_all(
    schema: &Schema,
    error: &syn::Ident,
    has_conversions: bool,
    child_errors: &[(syn::Ident, TokenStream)],
    _runtime: &TokenStream,
) -> TokenStream {
    let errors = schema
        .fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Nested(_) | FieldKind::Array(_)))
        .map(|field| {
            child_errors
                .iter()
                .find(|(name, _)| *name == field.name)
                .map(|(_, error)| error.clone())
                .unwrap_or_else(|| quote!(::core::convert::Infallible))
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        if has_conversions || schema.layout_can_fail() {
            quote!(#error)
        } else {
            quote!(::core::convert::Infallible)
        }
    } else {
        quote!(#error<#(#errors),*>)
    }
}
fn render_pending_constants(
    schema: &Schema,
    target_physical_index: usize,
    slots: &[Slot],
    conversion_names: &[Option<syn::Ident>],
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let target_slot_index = slots
        .iter()
        .position(|slot| slot.field == schema.fields[target_physical_index].name)
        .expect("target field has one writer slot");
    let previous_physical_index = target_slot_index
        .checked_sub(1)
        .and_then(|index| slots.get(index))
        .and_then(|slot| {
            schema
                .fields
                .iter()
                .position(|field| field.name == slot.field)
        });
    let start = previous_physical_index.map_or(0, |index| index + 1);
    let writes = schema.fields[start..target_physical_index]
        .iter()
        .enumerate()
        .filter_map(|(relative_index, field)| {
            let constant = field.kind.constant()?;
            let index = start + relative_index;
            let offset = private_ident(schema, &format!("pending_constant_{index}_offset"));
            let geometry = render_writer_start(schema, field, &offset, error, runtime);
            Some(match &field.kind {
                FieldKind::Scalar(scalar) => {
                    let value_ty = value_type_tokens(scalar.value_type);
                    let wire_ty = scalar_type_tokens(scalar.wire_type);
                    let encode = to_bytes_method(scalar.endian);
                    if let Some(variant) = &conversion_names[index] {
                        let semantic =
                            private_ident(schema, &format!("pending_constant_{index}_semantic"));
                        let encoded =
                            private_ident(schema, &format!("pending_constant_{index}_encoded"));
                        let conversion = convert_to_wire(scalar, &semantic, &wire_ty);
                        quote! {
                            #geometry
                            let #semantic: #value_ty = #constant;
                            let #encoded: #wire_ty = #conversion.ok_or_else(|| {
                                #runtime::WriteError::Schema(
                                    #error::#variant(
                                        #runtime::ScalarBuildConversionError {
                                            from: stringify!(#value_ty),
                                            to: stringify!(#wire_ty),
                                        },
                                    ),
                                )
                            })?;
                            self.writer.write_at(#offset, &#encoded.#encode())?;
                        }
                    } else {
                        quote! {
                            #geometry
                            let value: #wire_ty = #constant;
                            self.writer.write_at(#offset, &value.#encode())?;
                        }
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    quote! {
                        #geometry
                        let value: #ty = #constant;
                        self.writer.write_at(#offset, &value)?;
                    }
                }
                FieldKind::RawBytes(_)
                | FieldKind::Array(_)
                | FieldKind::Flag(_)
                | FieldKind::Nested(_) => unreachable!(),
            })
        });
    quote!(#(#writes)*)
}

fn render_writer_start(
    schema: &Schema,
    field: &super::model::Field,
    offset: &syn::Ident,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let field_name = field.name.unraw().to_string();
    let static_offset = builder_offset(&field.offset, runtime);
    let has_dynamic_prefix = field
        .offset
        .terms
        .iter()
        .any(|term| matches!(term, SizeTerm::Dynamic));
    let (base_setup, requires_forward) = match &field.layout.position {
        Some(Position::Static(position)) => (quote!(let mut #offset: usize = #position;), true),
        Some(Position::Field(controller)) => {
            let controller_field = schema
                .fields
                .iter()
                .find(|candidate| candidate.name == *controller)
                .expect("validated position controller");
            let FieldKind::Scalar(scalar) = &controller_field.kind else {
                unreachable!("validated position controller is scalar")
            };
            let controller_offset = builder_offset(&controller_field.offset, runtime);
            let width = scalar.width();
            let wire_ty = scalar_type_tokens(scalar.wire_type);
            let decode = super::from_bytes_method(scalar.endian);
            (
                quote! {
                    let controller_offset = #controller_offset
                        .ok_or_else(|| #runtime::WriteError::Schema(
                            #error::Layout(#runtime::LayoutError { field: #field_name }),
                        ))?;
                    let controller_bytes = self.writer
                        .read_at::<#width>(controller_offset)
                        .ok_or_else(|| #runtime::WriteError::Schema(
                            #error::Layout(#runtime::LayoutError { field: #field_name }),
                        ))?;
                    let controller = #wire_ty::#decode(controller_bytes);
                    let mut #offset = usize::try_from(controller).map_err(|_| {
                        #runtime::WriteError::Schema(
                            #error::Layout(#runtime::LayoutError { field: #field_name }),
                        )
                    })?;
                },
                true,
            )
        }
        None if has_dynamic_prefix => (quote!(let mut #offset = self.writer.position();), true),
        None => (
            quote! {
                let mut #offset = #static_offset.ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    )
                })?;
            },
            false,
        ),
    };
    let pad = field
        .layout
        .pad_before
        .as_ref()
        .map(|pad| {
            quote! {
                #offset = #offset.checked_add(#pad).ok_or_else(|| {
                    #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    )
                })?;
            }
        })
        .unwrap_or_default();
    let align = field
        .layout
        .align_before
        .as_ref()
        .map(|align| {
            quote! {
                #offset = #runtime::__private::checked_align(#offset, #align)
                    .ok_or_else(|| #runtime::WriteError::Schema(
                        #error::Layout(#runtime::LayoutError { field: #field_name }),
                    ))?;
            }
        })
        .unwrap_or_default();
    let forward_check = requires_forward.then(|| {
        quote! {
            if #offset < self.writer.position() {
                return Err(#runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name }),
                ));
            }
        }
    });
    quote! {
        #base_setup
        #pad
        #align
        #forward_check
        let written = self.writer.position();
        if #offset > written {
            self.writer.fill_at(written, #offset - written, 0)?;
        }
    }
}

fn render_length_patch(
    schema: &Schema,
    controller: &syn::Ident,
    dependent: &syn::Ident,
    length: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let controller_field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated length controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated length controller is scalar")
    };
    let controller_name = controller.unraw().to_string();
    let controller_offset = builder_offset(&controller_field.offset, runtime);
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let encode = to_bytes_method(scalar.endian);
    let decode = super::from_bytes_method(scalar.endian);
    let is_first = schema
        .length_dependents(controller)
        .next()
        .is_some_and(|field| field.name == *dependent);
    if is_first {
        quote! {
            let controller_offset = #controller_offset.ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let controller_value = #wire_ty::try_from(#length).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            self.writer
                .write_at(controller_offset, &controller_value.#encode())?;
        }
    } else {
        quote! {
            let controller_offset = #controller_offset.ok_or_else(|| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let controller_bytes = self.writer
                .read_at::<{ ::core::mem::size_of::<#wire_ty>() }>(controller_offset)
                .ok_or_else(|| #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                ))?;
            let expected = usize::try_from(#wire_ty::#decode(controller_bytes)).map_err(|_| {
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #controller_name }),
                )
            })?;
            let actual = #length;
            if actual != expected {
                return Err(#runtime::WriteError::Schema(#error::LengthConflict {
                    controller: #controller_name,
                    expected,
                    actual,
                }));
            }
        }
    }
}
fn render_count_patch(
    schema: &Schema,
    controller: &syn::Ident,
    count: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let controller_field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated item count controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated item count controller is scalar")
    };
    let controller_name = controller.unraw().to_string();
    let controller_offset = builder_offset(&controller_field.offset, runtime);
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let encode = to_bytes_method(scalar.endian);
    quote! {
        let controller_offset = #controller_offset.ok_or_else(|| {
            #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #controller_name }),
            )
        })?;
        let controller_value = #wire_ty::try_from(#count).map_err(|_| {
            #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #controller_name }),
            )
        })?;
        self.writer
            .write_at(controller_offset, &controller_value.#encode())?;
    }
}

fn render_presence_patch(
    schema: &Schema,
    flag: &syn::Ident,
    present: TokenStream,
    error: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let flag_field = schema
        .fields
        .iter()
        .find(|field| field.name == *flag)
        .expect("validated logical flag");
    let FieldKind::Flag(flag_field) = &flag_field.kind else {
        unreachable!("validated logical flag kind")
    };
    let controller = &flag_field.controller;
    let controller_field = schema
        .fields
        .iter()
        .find(|field| field.name == *controller)
        .expect("validated presence controller");
    let FieldKind::Scalar(scalar) = &controller_field.kind else {
        unreachable!("validated presence controller is scalar")
    };
    let controller_name = controller.unraw().to_string();
    let controller_offset = builder_offset(&controller_field.offset, runtime);
    let wire_ty = scalar_type_tokens(scalar.wire_type);
    let encode = to_bytes_method(scalar.endian);
    quote! {
        let controller_offset = #controller_offset.ok_or_else(|| {
            #runtime::WriteError::Schema(
                #error::Layout(#runtime::LayoutError { field: #controller_name }),
            )
        })?;
        let controller_value: #wire_ty = if #present { 1 } else { 0 };
        self.writer
            .write_at(controller_offset, &controller_value.#encode())?;
    }
}

fn add_geometry_state_bounds(
    schema: &Schema,
    field: &super::model::Field,
    slots: &[Slot],
    target_index: usize,
    generics: &mut syn::Generics,
    runtime: &TokenStream,
) {
    let mut dependencies = Vec::new();
    if let Some(Position::Field(controller)) = &field.layout.position {
        dependencies.push(controller.clone());
    }
    if field.layout.position.is_some()
        || field
            .offset
            .terms
            .iter()
            .any(|term| matches!(term, SizeTerm::Dynamic))
    {
        let field_index = schema
            .fields
            .iter()
            .position(|candidate| candidate.name == field.name)
            .expect("field belongs to schema");
        if let Some(previous) = schema.fields[..field_index]
            .iter()
            .rev()
            .find(|candidate| slots.iter().any(|slot| slot.field == candidate.name))
        {
            dependencies.push(previous.name.clone());
        }
    }
    for dependency in dependencies {
        if let Some((index, slot)) = slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.field == dependency)
            && index != target_index
        {
            let state = &slot.state;
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#state: #runtime::__private::IsSet));
        }
    }
}

fn add_offset_bounds(generics: &mut syn::Generics, offset: &LayoutOffset, runtime: &TokenStream) {
    for term in &offset.terms {
        if let SizeTerm::Nested(ty) = term {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #runtime::WireBuilder));
        }
    }
}
fn writer_type(
    writer: &syn::Ident,
    original_arguments: &[TokenStream],
    output: &syn::Ident,
    states: &[TokenStream],
) -> TokenStream {
    let mut arguments = original_arguments.to_vec();
    arguments.push(quote!(#output));
    arguments.extend(states.iter().cloned());
    quote!(#writer<#(#arguments),*>)
}

fn generic_arguments(generics: &syn::Generics) -> Vec<TokenStream> {
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
