use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericParam, TypeParam};

use super::model::Schema;
use super::recursive::RecursiveSlot;

pub(super) fn render(
    schema: &Schema,
    slot: &RecursiveSlot,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let demand = super::recursive_demand::validate(schema, &slot.generic)?;
    let vis = &schema.vis;
    let name = &schema.name;
    let root = &slot.generic;
    let (schema_impl, schema_types, schema_where) = schema.generics.split_for_impl();
    let self_type = quote!(#name #schema_types);
    let marker = super::recursive_writer::write_marker(schema, slot);
    let callback = super::fresh_type_ident(&schema.generics, "RecursiveWriteCallback");
    let cursor = super::fresh_type_ident(&schema.generics, "RecursiveWriteCursor");
    let source = super::fresh_type_ident(&schema.generics, "RecursiveBytes");
    let build = super::fresh_type_ident(&schema.generics, "RecursiveBuild");
    let controller = demand.controller_scalar;
    let controller_wire = super::scalar_type_tokens(controller.wire_type);
    let controller_encode = super::to_bytes_method(controller.endian);
    let controller_width = controller.width();
    let scalar = demand.scalar_field;
    let scalar_ty = super::value_type_tokens(&scalar.value_type);
    let scalar_wire = super::scalar_type_tokens(scalar.wire_type);
    let scalar_encode = super::to_bytes_method(scalar.endian);
    let scalar_name = &schema.fields[demand.scalar].name;
    let scalar_layout = &schema.fields[demand.scalar].layout;
    let scalar_pad = scalar_layout
        .pad_before
        .as_ref()
        .map(|pad| quote!(#pad))
        .unwrap_or_else(|| quote!(0usize));
    let scalar_align = scalar_layout.align_before.as_ref();
    let writer_layout = if let Some(align) = scalar_align {
        quote! {
            let mut relative = self
                .output
                .position()
                .checked_sub(self.body_start)
                .and_then(|offset| offset.checked_add(#scalar_pad))
                .ok_or(#runtime::WriteError::Schema(
                    #runtime::__private::RecursiveWriteError::Layout {
                        field: stringify!(#scalar_name),
                    },
                ))?;
            relative = #runtime::__private::checked_align(relative, #align).ok_or(
                #runtime::WriteError::Schema(
                    #runtime::__private::RecursiveWriteError::Layout {
                        field: stringify!(#scalar_name),
                    },
                ),
            )?;
            let position = self.body_start.checked_add(relative).ok_or(
                #runtime::WriteError::Schema(
                    #runtime::__private::RecursiveWriteError::Layout {
                        field: stringify!(#scalar_name),
                    },
                ),
            )?;
        }
    } else {
        quote! {
            let _ = self.body_start;
            let position = self
                .output
                .position()
                .checked_add(#scalar_pad)
                .ok_or(#runtime::WriteError::Schema(
                    #runtime::__private::RecursiveWriteError::Layout {
                        field: stringify!(#scalar_name),
                    },
                ))?;
        }
    };
    let controller_name = &schema.fields[demand.controller].name;
    let left_name = &schema.fields[demand.left].name;
    let bytes_name = &schema.fields[demand.bytes].name;
    let right_name = &schema.fields[demand.right].name;
    let stage0 = format_ident!("{}WriterStage0", marker);
    let stage1 = format_ident!("{}WriterStage1", marker);
    let stage2 = format_ident!("{}WriterStage2", marker);
    let stage3 = format_ident!("{}WriterStage3", marker);
    let complete = format_ident!("{}WriterComplete", marker);
    let slot_index = slot.index;
    let mut impl_generics = schema.generics.clone();
    impl_generics
        .params
        .push(GenericParam::Type(TypeParam::from(callback.clone())));
    let (body_impl, _, body_where) = impl_generics.split_for_impl();
    let error = quote! {
        #runtime::WriteError<
            #runtime::__private::RecursiveWriteError,
            <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
        >
    };

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #marker<#root>(::core::marker::PhantomData<fn() -> #root>);

        impl #schema_impl #runtime::__private::RecursiveWriteSlot<#slot_index>
            for #self_type #schema_where
        {
            type Marker = #marker<#root>;
        }

        #[doc(hidden)]
        #vis struct #stage0<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            controller_offset: usize,
            body_start: usize,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        #[doc(hidden)]
        #vis struct #stage1<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            controller_offset: usize,
            body_start: usize,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        #[doc(hidden)]
        #vis struct #stage2<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            body_start: usize,
            marker: ::core::marker::PhantomData<fn() -> (#callback, #root)>,
        }

        #[doc(hidden)]
        #vis struct #stage3<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
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

        impl<#cursor, #callback, #root> #stage0<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
            #callback: #runtime::__private::RecursiveWrite,
        {
            pub fn #left_name<#build>(self, build: #build) -> Result<
                #stage1<#cursor, #callback, #root>,
                #error,
            >
            where
                #build: FnOnce(
                    <#callback as #runtime::__private::RecursiveWrite>::Writer<#cursor>,
                ) -> Result<
                    <#callback as #runtime::__private::RecursiveWrite>::Complete<#cursor>,
                    #error,
                >,
            {
                let writer = <#callback as #runtime::__private::RecursiveWrite>::writer(
                    self.output,
                )?;
                let complete = build(writer)?;
                let output = <#callback as #runtime::__private::RecursiveWrite>::finish(
                    complete,
                )?;
                Ok(#stage1 {
                    output,
                    controller_offset: self.controller_offset,
                    body_start: self.body_start,
                    marker: ::core::marker::PhantomData,
                })
            }
        }

        impl<#cursor, #callback, #root> #stage1<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            pub fn #bytes_name<#source: AsRef<[u8]>>(
                mut self,
                source: #source,
            ) -> Result<#stage2<#cursor, #callback, #root>, #error> {
                let source = source.as_ref();
                let extent = #controller_wire::try_from(source.len()).map_err(|_| {
                    #runtime::WriteError::Schema(
                        #runtime::__private::RecursiveWriteError::CountOverflow {
                            field: stringify!(#controller_name),
                            count: source.len(),
                        },
                    )
                })?;
                self.output.write(source)?;
                self.output.patch_at(
                    self.controller_offset,
                    &extent.#controller_encode(),
                )?;
                Ok(#stage2 {
                    output: self.output,
                    body_start: self.body_start,
                    marker: ::core::marker::PhantomData,
                })
            }
        }

        impl<#cursor, #callback, #root> #stage2<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            pub fn #scalar_name(
                mut self,
                value: #scalar_ty,
            ) -> Result<#stage3<#cursor, #callback, #root>, #error> {
                #writer_layout
                self.output.fill_to(position)?;
                let value: #scalar_wire = value;
                self.output.write(&value.#scalar_encode())?;
                Ok(#stage3 {
                    output: self.output,
                    marker: ::core::marker::PhantomData,
                })
            }
        }

        impl<#cursor, #callback, #root> #stage3<#cursor, #callback, #root>
        where
            #cursor: #runtime::__private::RecursiveCursor,
            #callback: #runtime::__private::RecursiveWrite,
        {
            pub fn #right_name<#build>(self, build: #build) -> Result<
                #complete<#cursor, #callback, #root>,
                #error,
            >
            where
                #build: FnOnce(
                    <#callback as #runtime::__private::RecursiveWrite>::Writer<#cursor>,
                ) -> Result<
                    <#callback as #runtime::__private::RecursiveWrite>::Complete<#cursor>,
                    #error,
                >,
            {
                let writer = <#callback as #runtime::__private::RecursiveWrite>::writer(
                    self.output,
                )?;
                let complete = build(writer)?;
                let output = <#callback as #runtime::__private::RecursiveWrite>::finish(
                    complete,
                )?;
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
                = #stage0<#cursor, #callback, #root>;
            type Complete<#cursor: #runtime::__private::RecursiveCursor>
                = #complete<#cursor, #callback, #root>;

            fn writer<#cursor: #runtime::__private::RecursiveCursor>(
                mut output: #cursor,
            ) -> Result<Self::Writer<#cursor>, #runtime::WriteError<
                #runtime::__private::RecursiveWriteError,
                <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
            >> {
                let controller_offset = output.position();
                let body_start = controller_offset;
                output.write(&[0u8; #controller_width])?;
                Ok(#stage0 {
                    output,
                    controller_offset,
                    body_start,
                    marker: ::core::marker::PhantomData,
                })
            }

            fn finish<#cursor: #runtime::__private::RecursiveCursor>(
                complete: Self::Complete<#cursor>,
            ) -> Result<#cursor, #runtime::WriteError<
                #runtime::__private::RecursiveWriteError,
                <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
            >> {
                Ok(complete.output)
            }
        }
    })
}
