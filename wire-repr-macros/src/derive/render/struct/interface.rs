//! Generated struct public interface and trait wiring rendering.

mod borrowed;
#[path = "interface/owned.rs"]
mod owned;

use proc_macro2::{Ident, TokenStream};

pub(super) struct ReadFragments {
    pub(super) request_declarations: TokenStream,
    pub(super) request_impls: TokenStream,
    pub(super) inherent_methods: TokenStream,
    pub(super) view_type_impl: TokenStream,
}

pub(super) struct Input<'a> {
    pub(super) identity: Identity<'a>,
    pub(super) surface: Surface<'a>,
    pub(super) types: Types<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) requests: Requests<'a>,
    pub(super) preparation: Preparation<'a>,
    pub(super) capabilities: Capabilities<'a>,
}

pub(super) struct Identity<'a> {
    pub(super) name: &'a Ident,
    pub(super) view: &'a Ident,
    pub(super) decode_error: &'a Ident,
}
pub(super) struct Surface<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) runtime: &'a TokenStream,
}
pub(super) struct Types<'a> {
    pub(super) wire_lifetime: Option<&'a syn::Lifetime>,
    pub(super) view_error_type: &'a TokenStream,
    pub(super) validation_error_type: &'a TokenStream,
    pub(super) association_error_type: &'a TokenStream,
    pub(super) plan_type: &'a TokenStream,
    pub(super) encode_error_type: &'a TokenStream,
}
pub(super) struct Operation<'a> {
    pub(super) input_ty: Option<&'a syn::Path>,
    pub(super) name: Option<&'a Ident>,
    pub(super) parse: Option<&'a Ident>,
    pub(super) prepare: Option<&'a Ident>,
    pub(super) value: &'a Ident,
    pub(super) encode_request: &'a Ident,
}
pub(super) struct Requests<'a> {
    pub(super) view_input: &'a Ident,
    pub(super) direct_view: &'a Ident,
    pub(super) unchecked_view: &'a Ident,
    pub(super) cursor_input: &'a Ident,
    pub(super) direct_cursor: &'a Ident,
    pub(super) unchecked_cursor: &'a Ident,
}
pub(super) struct Preparation<'a> {
    pub(super) helper: &'a Ident,
    pub(super) body: &'a TokenStream,
    pub(super) field_parameters: &'a [TokenStream],
    pub(super) destructure: &'a [TokenStream],
    pub(super) field_names: &'a [&'a Ident],
    pub(super) builder_method: &'a TokenStream,
}
pub(super) struct Capabilities<'a> {
    pub(super) fixed_sequence_width: Option<&'a TokenStream>,
    pub(super) has_validation: bool,
}

pub(super) fn render(input: Input<'_>) -> TokenStream {
    if cfg!(feature = "bytes") {
        owned::render(input)
    } else {
        borrowed::render(input)
    }
}

#[allow(dead_code)]
pub(super) fn render_with_read(input: Input<'_>, read: ReadFragments) -> TokenStream {
    if cfg!(feature = "bytes") {
        owned::render_with_read(input, read)
    } else {
        borrowed::render_with_read(input, read)
    }
}
