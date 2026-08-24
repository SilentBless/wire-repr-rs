//! Borrowed-slice dynamic struct interface rendering.

mod read;
mod write;

use super::{Input, ReadFragments};
use proc_macro2::TokenStream;
use quote::quote;

pub(super) struct TypeShape {
    pub(super) impl_generics: TokenStream,
    pub(super) self_type: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    let wire_lifetime = input.types.wire_lifetime;
    let name = input.identity.name;
    let shape = wire_lifetime.map_or_else(
        || TypeShape {
            impl_generics: quote!(),
            self_type: quote!(#name),
        },
        |lifetime| TypeShape {
            impl_generics: quote!(<#lifetime>),
            self_type: quote!(#name<#lifetime>),
        },
    );
    let read = read::render(&input, &shape);

    render_with_read(input, read)
}

pub(super) fn render_with_read(input: Input<'_>, read: ReadFragments) -> TokenStream {
    let wire_lifetime = input.types.wire_lifetime;
    let name = input.identity.name;
    let shape = wire_lifetime.map_or_else(
        || TypeShape {
            impl_generics: quote!(),
            self_type: quote!(#name),
        },
        |lifetime| TypeShape {
            impl_generics: quote!(<#lifetime>),
            self_type: quote!(#name<#lifetime>),
        },
    );
    let write = write::render(&input, &shape);
    let TypeShape {
        impl_generics,
        self_type,
    } = &shape;

    if input.operation.input_ty.is_some() {
        let builder_method = input.preparation.builder_method;
        let ReadFragments {
            request_declarations,
            request_impls,
            inherent_methods: read_methods,
            view_type_impl,
        } = read;
        let write::Fragments {
            encode_request_declaration,
            encode_request_impl,
            inherent_methods: write_methods,
            encode_impl: _,
        } = write;
        quote! {
            #request_declarations
            #encode_request_declaration
            #request_impls
            #encode_request_impl
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #builder_method
                #read_methods
                #write_methods
            }
            #view_type_impl
        }
    } else {
        let ReadFragments {
            request_declarations: _,
            request_impls: _,
            inherent_methods: read_methods,
            view_type_impl,
        } = read;
        let write::Fragments {
            encode_request_declaration: _,
            encode_request_impl: _,
            inherent_methods: write_methods,
            encode_impl,
        } = write;
        quote! {
            #[allow(missing_docs)]
            impl #impl_generics #self_type {
                #read_methods
                #write_methods
            }
            #view_type_impl
            #encode_impl
        }
    }
}
