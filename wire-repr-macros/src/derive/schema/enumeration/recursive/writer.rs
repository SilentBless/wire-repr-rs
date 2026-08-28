use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;

use super::super::{EnumSchema, variant_method_names, variant_type_stems};

pub(in super::super) fn render(
    mut schema: EnumSchema,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let root = schema.name.clone();
    for variant in &mut schema.variants {
        variant.body = super::super::super::recursive::normalize_root_self(&variant.body, &root);
    }
    if !schema.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &schema.generics,
            "recursive enum roots cannot declare generic parameters",
        ));
    }
    if schema.unknown().is_some() {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive enum roots cannot declare an unknown terminal body",
        ));
    }

    let vis = &schema.vis;
    let name = &schema.name;
    let selector_ty = super::super::super::scalar_type_tokens(schema.selector);
    let encode = super::super::super::to_bytes_method(schema.endian);
    let writer = format_ident!("{}Writer", name.unraw());
    let complete = format_ident!("{}WriterComplete", name.unraw());
    let callback = format_ident!("__WireRepr{}RecursiveWrite", name.unraw());
    let cursor = super::super::super::fresh_type_ident(&schema.generics, "RecursiveWriteCursor");
    let detached = format_ident!("{}Builder", name.unraw());
    let detached_copy = format_ident!("{}Copy", name.unraw());
    let output = super::super::super::fresh_type_ident(&schema.generics, "Output");
    let build = super::super::super::fresh_type_ident(&schema.generics, "Build");
    let child_builder = super::super::super::fresh_type_ident(&schema.generics, "ChildBuilder");
    let view = super::super::super::fresh_type_ident(&schema.generics, "RecursiveView");
    let known = schema.known_variants().collect::<Vec<_>>();
    let type_stems = variant_type_stems(&known, &["Root"]);
    let method_names = variant_method_names(&known, &["copy_from"]);
    let write_slots = known
        .iter()
        .map(|variant| {
            super::super::super::recursive::root_write_slot_marker(
                &variant.body,
                &schema.name,
                runtime,
            )
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let result_alias = format_ident!("{}WriteResult", name.unraw());
    let root_writer_alias = format_ident!("{}RootWriter", name.unraw());
    let root_complete_alias = format_ident!("{}RootWriterComplete", name.unraw());
    let writer_aliases = known
        .iter()
        .zip(&write_slots)
        .zip(&type_stems)
        .filter_map(|((variant, slot), stem)| {
            let slot = slot.as_ref()?;
            let body = &variant.body;
            let writer_alias = format_ident!("{}{}Writer", name.unraw(), stem);
            let complete_alias = format_ident!("{}{}WriterComplete", name.unraw(), stem);
            Some(quote! {
                #[doc = "Initial progressive writer for this recursive variant body."]
                #vis type #writer_alias<#cursor: #runtime::RecursiveCursor> =
                    <#body as #runtime::__private::RecursiveWriteBody<
                        #callback,
                        #slot,
                    >>::Writer<#cursor>;

                #[doc = "Completed progressive writer for this recursive variant body."]
                #vis type #complete_alias<#cursor: #runtime::RecursiveCursor> =
                    <#body as #runtime::__private::RecursiveWriteBody<
                        #callback,
                        #slot,
                    >>::Complete<#cursor>;
            })
        })
        .collect::<Vec<_>>();
    let recursive_methods = known
        .iter()
        .zip(&write_slots)
        .zip(&method_names)
        .map(|((variant, slot), method)| {
            let body = &variant.body;
            let value = variant.value.as_ref().expect("known selector");
            let field = variant.name.unraw().to_string();
            if variant.unit {
                return quote! {
                    #[inline]
                    #vis fn #method(mut self) -> Result<
                        #complete<#cursor>,
                        #runtime::OutputError<
                            <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                        >,
                    > {
                        let selector: #selector_ty = #value;
                        #runtime::__private::RecursiveCursor::write(
                            &mut self.output,
                            &selector.#encode(),
                        )?;
                        Ok(#complete {
                            output: self.output,
                            marker: ::core::marker::PhantomData,
                        })
                    }
                };
            }
            if let Some(slot) = slot {
                quote! {
                    #[inline]
                    #vis fn #method<#build>(mut self, build: #build) -> Result<
                        #complete<#cursor>,
                        #runtime::WriteError<
                            #runtime::__private::RecursiveWriteError,
                            <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                        >,
                    >
                    where
                        #body: #runtime::__private::RecursiveWriteBody<#callback, #slot>,
                        #build: FnOnce(
                            <#body as #runtime::__private::RecursiveWriteBody<
                                #callback,
                                #slot,
                            >>::Writer<#cursor>,
                        ) -> Result<
                            <#body as #runtime::__private::RecursiveWriteBody<
                                #callback,
                                #slot,
                            >>::Complete<#cursor>,
                            #runtime::WriteError<
                                #runtime::__private::RecursiveWriteError,
                                <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                            >,
                        >,
                    {
                        let selector: #selector_ty = #value;
                        #runtime::__private::RecursiveCursor::write(
                            &mut self.output,
                            &selector.#encode(),
                        )?;
                        let body = <#body as #runtime::__private::RecursiveWriteBody<
                            #callback,
                            #slot,
                        >>::writer(self.output)?;
                        let body = build(body)?;
                        let output = <#body as #runtime::__private::RecursiveWriteBody<
                            #callback,
                            #slot,
                        >>::finish(body)?;
                        Ok(#complete {
                            output,
                            marker: ::core::marker::PhantomData,
                        })
                    }
                }
            } else {
                quote! {
                    #[inline]
                    #vis fn #method<#build, #child_builder>(
                        mut self,
                        build: #build,
                    ) -> Result<
                        #complete<#cursor>,
                        #runtime::WriteError<
                            #runtime::__private::RecursiveWriteError,
                            <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                        >,
                    >
                    where
                        #body: #runtime::WireBuilder + #runtime::WireWrite<#child_builder>,
                        #build: FnOnce(<#body as #runtime::WireBuilder>::Builder) -> #child_builder,
                    {
                        let selector: #selector_ty = #value;
                        #runtime::__private::RecursiveCursor::write(
                            &mut self.output,
                            &selector.#encode(),
                        )?;
                        let child = build(<#body as #runtime::WireBuilder>::builder());
                        match #runtime::__private::RecursiveCursor::write_value::<#body, _>(
                            &mut self.output,
                            child,
                        ) {
                            Ok(()) => {}
                            Err(#runtime::WriteError::Schema(_)) => {
                                return Err(#runtime::WriteError::Schema(
                                    #runtime::__private::RecursiveWriteError::Child {
                                        field: #field,
                                    },
                                ));
                            }
                            Err(#runtime::WriteError::Output(error)) => {
                                return Err(#runtime::WriteError::Output(error));
                            }
                        }
                        Ok(#complete {
                            output: self.output,
                            marker: ::core::marker::PhantomData,
                        })
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let callback_impl = quote! {
        impl #runtime::__private::RecursiveWrite for #callback {
            type Writer<#cursor: #runtime::__private::RecursiveCursor>
                = #writer<#cursor>;
            type Complete<#cursor: #runtime::__private::RecursiveCursor>
                = #complete<#cursor>;

            fn writer<#cursor: #runtime::__private::RecursiveCursor>(
                output: #cursor,
            ) -> Result<
                Self::Writer<#cursor>,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                Ok(#writer {
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
    };

    Ok(quote! {
        #[doc = "Initial progressive writer for this recursive root."]
        #vis type #root_writer_alias<#cursor: #runtime::RecursiveCursor> = #writer<#cursor>;

        #[doc = "Completed progressive writer for this recursive root."]
        #vis type #root_complete_alias<#cursor: #runtime::RecursiveCursor> = #complete<#cursor>;

        #[doc = "Result returned while progressively writing this recursive root."]
        #vis type #result_alias<T, #cursor: #runtime::RecursiveCursor> = Result<
            T,
            #runtime::WriteError<
                #runtime::__private::RecursiveWriteError,
                <#cursor as #runtime::RecursiveCursor>::GrowError,
            >,
        >;

        #(#writer_aliases)*

        #[doc(hidden)]
        #vis struct #detached;

        #[doc(hidden)]
        #vis struct #detached_copy<#view> {
            view: #view,
        }

        impl #detached {
            #[inline(always)]
            #vis fn copy_from<#view: #runtime::ExactWire<#name>>(
                self,
                view: #view,
            ) -> #detached_copy<#view> {
                #detached_copy { view }
            }
        }

        impl #runtime::WireBuilder for #name {
            const FIXED_SIZE: Option<usize> = None;
            type Builder = #detached;

            fn builder() -> Self::Builder {
                #detached
            }
        }

        impl<#view: #runtime::ExactWire<#name>> #runtime::WireWrite<#detached_copy<#view>>
            for #name
        {
            type Error = ::core::convert::Infallible;

            fn write<__WireReprOutput: #runtime::Output>(
                value: #detached_copy<#view>,
                output: &mut #runtime::ChildWriter<'_, __WireReprOutput>,
            ) -> Result<
                (),
                #runtime::WriteError<Self::Error, __WireReprOutput::GrowError>,
            > {
                output.write(#runtime::ExactWire::as_wire_bytes(&value.view))?;
                Ok(())
            }
        }

        #[doc(hidden)]
        #vis struct #callback;

        #[doc(hidden)]
        #vis struct #writer<#cursor>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            marker: ::core::marker::PhantomData<fn() -> #name>,
        }

        #[doc(hidden)]
        #vis struct #complete<#cursor>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            output: #cursor,
            marker: ::core::marker::PhantomData<fn() -> #name>,
        }

        impl<#cursor> #writer<#cursor>
        where
            #cursor: #runtime::__private::RecursiveCursor,
        {
            #(#recursive_methods)*

            #[inline]
            #vis fn copy_from<#view: #runtime::ExactWire<#name>>(
                mut self,
                view: #view,
            ) -> Result<
                #complete<#cursor>,
                #runtime::WriteError<
                    #runtime::__private::RecursiveWriteError,
                    <#cursor as #runtime::__private::RecursiveCursor>::GrowError,
                >,
            > {
                #runtime::__private::RecursiveCursor::write(
                    &mut self.output,
                    #runtime::ExactWire::as_wire_bytes(&view),
                )?;
                Ok(#complete {
                    output: self.output,
                    marker: ::core::marker::PhantomData,
                })
            }
        }

        impl<#output: #runtime::Output> #complete<#runtime::Writer<#output>> {
            #[inline(always)]
            #vis fn finish(self) -> Result<
                #runtime::Written<#output>,
                #runtime::OutputError<<#output as #runtime::Output>::GrowError>,
            > {
                Ok(self.output.finish())
            }
        }

        impl #name {
            /// Creates a progressive typestate writer over `output`.
            #[inline(always)]
            #vis fn builder<#output: #runtime::Output>(
                output: #output,
            ) -> #writer<#runtime::Writer<#output>> {
                #writer {
                    output: #runtime::Writer::new(output),
                    marker: ::core::marker::PhantomData,
                }
            }
        }

        #callback_impl
    })
}
