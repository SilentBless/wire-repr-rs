use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, Ident, TypeParam};

use super::model::{FieldKind, Schema};
use super::recursive::RecursiveSlot;

pub(super) fn render_bodies(schema: &Schema, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let mut rendered = TokenStream::new();
    for slot in super::recursive::schema_slots(schema) {
        if slot
            .fields
            .iter()
            .any(|&index| matches!(schema.fields[index].kind, FieldKind::Recursive(_)))
        {
            if schema
                .fields
                .iter()
                .any(|field| matches!(field.kind, FieldKind::RawBytes(_)))
            {
                rendered.extend(super::recursive_demand_writer::render(
                    schema, &slot, runtime,
                )?);
            } else {
                rendered.extend(render_object(schema, &slot, runtime)?);
            }
        } else if slot.fields.len() == 1 && schema.fields.len() == 2 && slot.fields[0] == 1 {
            rendered.extend(render_array(schema, &slot, runtime)?);
        }
    }
    Ok(rendered)
}

fn render_object(
    schema: &Schema,
    slot: &RecursiveSlot,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    super::recursive_object::validate(schema, &slot.generic)?;
    let vis = &schema.vis;
    let name = &schema.name;
    let root = &slot.generic;
    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let marker = write_marker(schema, slot);
    let callback = super::fresh_type_ident(&schema.generics, "RecursiveWriteCallback");
    let cursor = super::fresh_type_ident(&schema.generics, "RecursiveWriteCursor");
    let stages = (0..=schema.fields.len())
        .map(|index| format_ident!("{}WriterStage{index}", marker))
        .collect::<Vec<_>>();
    let stage_declarations = stages.iter().map(|stage| {
        quote! {
            #[doc(hidden)]
            #vis struct #stage<#cursor, #callback, #root>
            where
                #cursor: #runtime::__private::RecursiveCursor,
            {
                output: #cursor,
                marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
            }
        }
    });
    let methods = schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let current = &stages[index];
            let next = &stages[index + 1];
            let field_name = &field.name;
            let common_result = quote! {
                Result<
                    #next<#cursor, #callback, #root>,
                    #runtime::WriteError<
                        #runtime::__private::RecursiveWriteError,
                        <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                    >,
                >
            };
            match &field.kind {
                FieldKind::Recursive(_) => {
                    let build = super::fresh_type_ident(&schema.generics, "RecursiveBuild");
                    quote! {
                        impl<#cursor, #callback, #root> #current<#cursor, #callback, #root>
                        where
                            #cursor: #runtime::__private::RecursiveCursor,
                            #callback: #runtime::__private::RecursiveWrite,
                        {
                            #[inline]
                            #vis fn #field_name<#build>(self, build: #build) -> #common_result
                            where
                                #build: FnOnce(
                                    <#callback as #runtime::__private::RecursiveWrite>::Writer<
                                        #cursor,
                                    >,
                                ) -> Result<
                                    <#callback as #runtime::__private::RecursiveWrite>::Complete<
                                        #cursor,
                                    >,
                                    #runtime::WriteError<
                                        #runtime::__private::RecursiveWriteError,
                                        <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                                    >,
                                >,
                            {
                                let writer = <#callback as #runtime::__private::RecursiveWrite>::writer(
                                    self.output,
                                )?;
                                let complete = build(writer)?;
                                let output = <#callback as #runtime::__private::RecursiveWrite>::finish(
                                    complete,
                                )?;
                                Ok(#next {
                                    output,
                                    marker: ::core::marker::PhantomData,
                                })
                            }
                        }
                    }
                }
                FieldKind::Scalar(scalar) => {
                    let value_ty = super::value_type_tokens(&scalar.value_type);
                    let wire_ty = super::scalar_type_tokens(scalar.wire_type);
                    let encode = super::to_bytes_method(scalar.endian);
                    quote! {
                        impl<#cursor, #callback, #root> #current<#cursor, #callback, #root>
                        where
                            #cursor: #runtime::__private::RecursiveCursor,
                        {
                            #[inline]
                            #vis fn #field_name(
                                mut self,
                                value: #value_ty,
                            ) -> #common_result {
                                let value: #wire_ty = value;
                                self.output.write(&value.#encode())?;
                                Ok(#next {
                                    output: self.output,
                                    marker: ::core::marker::PhantomData,
                                })
                            }
                        }
                    }
                }
                FieldKind::Bytes(_) => {
                    let ty = &field.ty;
                    quote! {
                        impl<#cursor, #callback, #root> #current<#cursor, #callback, #root>
                        where
                            #cursor: #runtime::__private::RecursiveCursor,
                        {
                            #[inline]
                            #vis fn #field_name(mut self, value: #ty) -> #common_result {
                                self.output.write(&value)?;
                                Ok(#next {
                                    output: self.output,
                                    marker: ::core::marker::PhantomData,
                                })
                            }
                        }
                    }
                }
                _ => unreachable!("validated recursive object writer field"),
            }
        })
        .collect::<Vec<_>>();
    let first = stages.first().expect("recursive object stage");
    let complete = stages.last().expect("recursive object completion");
    let mut impl_generics = schema.generics.clone();
    impl_generics
        .params
        .push(GenericParam::Type(TypeParam::from(callback.clone())));
    let (body_impl, _, body_where) = impl_generics.split_for_impl();
    let slot_index = slot.index;

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

        impl #schema_impl #runtime::__private::RecursiveWriteSlot<#slot_index>
            for #self_type #schema_where
        {
            type Marker = #marker<#root>;
        }

        #(#stage_declarations)*
        #(#methods)*

        impl #body_impl #runtime::__private::RecursiveWriteBody<#callback, #marker<#root>>
            for #self_type #body_where
        {
            type Writer<#cursor: #runtime::__private::RecursiveCursor>
                = #first<#cursor, #callback, #root>;

            type Complete<#cursor: #runtime::__private::RecursiveCursor>
                = #complete<#cursor, #callback, #root>;

            fn writer<#cursor: #runtime::__private::RecursiveCursor>(
                output: #cursor,
            ) -> Result<
                Self::Writer<#cursor>,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                Ok(#first {
                    output,
                    marker: ::core::marker::PhantomData,
                })
            }

            fn finish<#cursor: #runtime::__private::RecursiveCursor>(
                complete: Self::Complete<#cursor>,
            ) -> Result<
                #cursor,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                Ok(complete.output)
            }
        }
    })
}

fn render_array(
    schema: &Schema,
    slot: &RecursiveSlot,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let array_index = slot.fields[0];
    let array_field = &schema.fields[array_index];
    let FieldKind::Array(array) = &array_field.kind else {
        return Ok(TokenStream::new());
    };
    let count_name = &array.controller;
    let count_field = &schema.fields[0];
    if count_field.name != *count_name {
        return Err(syn::Error::new_spanned(
            count_name,
            "recursive array count must be the physically earlier field",
        ));
    }
    let has_layout = schema.fields.iter().any(|field| {
        field.layout.condition.is_some()
            || field.layout.position.is_some()
            || field.layout.pad_before.is_some()
            || field.layout.align_before.is_some()
    });
    if !schema.validators.is_empty() || schema.fields.len() != 2 || array_index != 1 || has_layout {
        return Ok(TokenStream::new());
    }
    let FieldKind::Scalar(count) = &count_field.kind else {
        return Err(syn::Error::new_spanned(
            &count_field.ty,
            "recursive array count must be an unsigned scalar",
        ));
    };
    if !count.wire_type.is_unsigned_integer()
        || count.constant.is_some()
        || count.computed.is_some()
    {
        return Err(syn::Error::new_spanned(
            &count_field.ty,
            "recursive array count must be a stored unsigned scalar",
        ));
    }

    let vis = &schema.vis;
    let name = &schema.name;
    let root = &slot.generic;
    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let marker = write_marker(schema, slot);
    let writer = format_ident!("{}Writer", marker);
    let complete = format_ident!("{}Complete", marker);
    let items = format_ident!("{}Items", marker);
    let callback = super::fresh_type_ident(&schema.generics, "RecursiveWriteCallback");
    let cursor = super::fresh_type_ident(&schema.generics, "RecursiveWriteCursor");
    let build = super::fresh_type_ident(&schema.generics, "RecursiveBuild");
    let view = super::fresh_type_ident(&schema.generics, "RecursiveView");
    let count_wire = super::scalar_type_tokens(count.wire_type);
    let count_encode = super::to_bytes_method(count.endian);
    let count_width = count.width();
    let count_field_name = count_field.name.unraw().to_string();
    let items_name = &array_field.name;
    let slot_index = slot.index;
    let mut impl_generics = schema.generics.clone();
    impl_generics
        .params
        .push(GenericParam::Type(TypeParam::from(callback.clone())));
    let (body_impl, _, body_where) = impl_generics.split_for_impl();

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

        impl #schema_impl #runtime::__private::RecursiveWriteSlot<#slot_index>
            for #self_type #schema_where
        {
            type Marker = #marker<#root>;
        }

        #[doc(hidden)]
        #vis struct #writer<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            count_offset: usize,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        #[doc(hidden)]
        #vis struct #complete<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        #[doc(hidden)]
        #vis struct #items<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            count: usize,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        impl<#cursor, #callback, #root> #items<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
            #callback: #runtime::__private::RecursiveWrite,
        {
            #[inline]
            #vis fn item<#build>(mut self, build: #build) -> Result<
                Self,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            >
            where
                #build: FnOnce(
                    <#callback as #runtime::__private::RecursiveWrite>::Writer<#cursor>,
                ) -> Result<
                    <#callback as #runtime::__private::RecursiveWrite>::Complete<#cursor>,
                    #runtime::WriteError<
                        #runtime::__private::RecursiveWriteError,
                        <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                    >,
                >,
            {
                let writer = <#callback as #runtime::__private::RecursiveWrite>::writer(
                    self.output,
                )?;
                let complete = build(writer)?;
                self.output = <#callback as #runtime::__private::RecursiveWrite>::finish(
                    complete,
                )?;
                self.count = self.count.checked_add(1).ok_or(
                    #runtime::WriteError::Output(#runtime::OutputError::LengthOverflow),
                )?;
                Ok(self)
            }

            #[inline]
            #vis fn item_view<#view: #runtime::ExactWire<#root>>(
                mut self,
                view: #view,
            ) -> Result<
                Self,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                self.output.write(#runtime::ExactWire::as_wire_bytes(&view))?;
                self.count = self.count.checked_add(1).ok_or(
                    #runtime::WriteError::Output(#runtime::OutputError::LengthOverflow),
                )?;
                Ok(self)
            }
        }

        impl<#cursor, #callback, #root> #writer<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
            #callback: #runtime::__private::RecursiveWrite,
        {
            #[inline]
            #vis fn #items_name<#build>(self, build: #build) -> Result<
                #complete<#cursor, #callback, #root>,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            >
            where
                #build: FnOnce(
                    #items<#cursor, #callback, #root>,
                ) -> Result<
                    #items<#cursor, #callback, #root>,
                    #runtime::WriteError<
                        #runtime::__private::RecursiveWriteError,
                        <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                    >,
                >,
            {
                let items = build(#items {
                    output: self.output,
                    count: 0,
                    marker: ::core::marker::PhantomData,
                })?;
                let recursive_count = u32::try_from(items.count).map_err(|_| {
                    #runtime::WriteError::Schema(
                        #runtime::__private::RecursiveWriteError::CountOverflow {
                            field: #count_field_name,
                            count: items.count,
                        },
                    )
                })?;
                let count = #count_wire::try_from(recursive_count).map_err(|_| {
                    #runtime::WriteError::Schema(
                        #runtime::__private::RecursiveWriteError::CountOverflow {
                            field: #count_field_name,
                            count: items.count,
                        },
                    )
                })?;
                let mut output = items.output;
                output.patch_at(self.count_offset, &count.#count_encode())?;
                Ok(#complete {
                    output,
                    marker: ::core::marker::PhantomData,
                })
            }
        }

        impl #body_impl #runtime::__private::RecursiveWriteBody<#callback, #marker<#root>>
            for #self_type #body_where
        {
            type Writer<#cursor: #runtime::__private::RecursiveCursor>
                = #writer<#cursor, #callback, #root>;

            type Complete<#cursor: #runtime::__private::RecursiveCursor>
                = #complete<#cursor, #callback, #root>;

            fn writer<#cursor: #runtime::__private::RecursiveCursor>(
                mut output: #cursor,
            ) -> Result<
                Self::Writer<#cursor>,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                let count_offset = output.position();
                output.write(&[0u8; #count_width])?;
                Ok(#writer {
                    output,
                    count_offset,
                    marker: ::core::marker::PhantomData,
                })
            }

            fn finish<#cursor: #runtime::__private::RecursiveCursor>(
                complete: Self::Complete<#cursor>,
            ) -> Result<
                #cursor,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                Ok(complete.output)
            }
        }
    })
}

pub(super) fn write_marker(schema: &Schema, slot: &RecursiveSlot) -> Ident {
    format_ident!(
        "__WireRepr{}RecursiveWriteSlot{}",
        schema.name.unraw(),
        slot.index,
    )
}
