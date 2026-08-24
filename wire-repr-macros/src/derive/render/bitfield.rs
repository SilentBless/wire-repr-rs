//! Nominal bitfield derive rendering.

mod error;
mod interface;
mod plan;
mod view;

use super::super::model::WireBitfield;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(super) fn render(model: WireBitfield, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let WireBitfield {
        vis,
        name,
        storage,
        fields,
    } = model;
    let view = format_ident!("{name}View");
    let plan = format_ident!("{name}Plan");
    let field_proxy = format_ident!("{name}Fields");
    let decode_error = format_ident!("{name}Error");
    let encode_error = format_ident!("{name}EncodeError");
    let codec = format_ident!("{}", storage.codec);
    let storage_type = format_ident!("{}", storage.ty);
    let owned_mode = cfg!(feature = "bytes");

    let decode_error_declaration = error::render(error::Input {
        vis: &vis,
        name: &decode_error,
        kind: error::Kind::Decode,
    });
    let view_declaration = view::render(view::Input {
        schema: view::Schema {
            vis: &vis,
            name: &name,
            fields: &fields,
        },
        types: view::Types {
            view: &view,
            error: &decode_error,
            codec: &codec,
            storage_type: &storage_type,
        },
        owned_mode,
        runtime,
    });
    let encode_error_declaration = error::render(error::Input {
        vis: &vis,
        name: &encode_error,
        kind: error::Kind::Encode,
    });
    let plan_declaration = plan::render(plan::Input {
        schema: plan::Schema {
            vis: &vis,
            name: &name,
            fields: &fields,
        },
        types: plan::Types {
            plan: &plan,
            encode_error: &encode_error,
            codec: &codec,
            storage_type: &storage_type,
        },
        mode: plan::Mode {
            runtime,
            owned_mode,
        },
    });
    let inherent_impl = interface::render(interface::Input {
        types: interface::Types {
            name: &name,
            view: &view,
            plan: &plan,
            error: &decode_error,
            encode_error: &encode_error,
            codec: &codec,
        },
        surface: interface::Surface {
            vis: &vis,
            runtime,
            owned_mode,
        },
    });

    Ok(quote! {
        #decode_error_declaration
        #view_declaration

        /// Generated field-selection proxy for this nominal bitfield.
        #[doc(hidden)]
        #vis struct #field_proxy<S: #runtime::MarkerScope = #runtime::RootScope>(
            ::core::marker::PhantomData<fn() -> S>
        );

        impl<S: #runtime::MarkerScope> Copy for #field_proxy<S> {
        }

        impl<S: #runtime::MarkerScope> Clone for #field_proxy<S> {
            fn clone(&self) -> Self {
                *self
            }
        }

        #[allow(missing_docs)]
        impl<S: #runtime::MarkerScope> #field_proxy<S> {
            #[doc(hidden)]
            #vis fn __wire_repr_new() -> Self {
                Self(::core::marker::PhantomData)
            }
        }

        #encode_error_declaration
        #plan_declaration
        #inherent_impl
    })
}
