//! Enum request, cursor, and encoding interface rendering.
use proc_macro2::{Ident, TokenStream};
use syn::{Path, Visibility};

/// Names used by the public request and cursor surface.
pub(super) struct Names<'a> {
    pub(super) request: RequestNames<'a>,
    pub(super) cursor: CursorNames<'a>,
    pub(super) encode_request: &'a Ident,
}

pub(super) struct RequestNames<'a> {
    pub(super) vis: &'a Visibility,
    pub(super) view: &'a Ident,
    pub(super) view_input_request: &'a Ident,
    pub(super) view_request: &'a Ident,
    pub(super) unchecked_view_request: &'a Ident,
}

pub(super) struct CursorNames<'a> {
    pub(super) cursor_input_request: &'a Ident,
    pub(super) cursor: &'a Ident,
    pub(super) unchecked_cursor: &'a Ident,
}

/// Generated type fragments used by the interface surface.
pub(super) struct Types<'a> {
    pub(super) implementation: ImplementationTypes<'a>,
    pub(super) view: ViewTypes<'a>,
    pub(super) encode: EncodeTypes<'a>,
    pub(super) signatures: SignatureTypes<'a>,
}

pub(super) struct ImplementationTypes<'a> {
    pub(super) impl_generics: &'a TokenStream,
    pub(super) self_type: &'a TokenStream,
}

pub(super) struct ViewTypes<'a> {
    pub(super) validation_error: &'a TokenStream,
    pub(super) view_error: &'a TokenStream,
    pub(super) decode_error: &'a Ident,
}

pub(super) struct EncodeTypes<'a> {
    pub(super) encode_error: &'a TokenStream,
    pub(super) plan: &'a TokenStream,
}

pub(super) struct SignatureTypes<'a> {
    pub(super) view_signature: &'a TokenStream,
    pub(super) operation_view_signature: &'a TokenStream,
    pub(super) operation_encode_decl_generics: &'a TokenStream,
    pub(super) operation_encode_value_type: &'a TokenStream,
    pub(super) operation_encode_request_type: &'a TokenStream,
}

/// Optional operation forwarding and encode preparation details.
pub(super) struct Operation<'a> {
    pub(super) input_ty: Option<&'a Path>,
    pub(super) name: Option<&'a Ident>,
    pub(super) parse: Option<&'a Ident>,
    pub(super) prepare: Option<&'a Ident>,
    pub(super) prepare_arms: &'a [TokenStream],
}

/// Ownership-dependent request and cursor choices.
pub(super) struct Mode<'a> {
    pub(super) owned: bool,
    pub(super) static_view_request: &'a TokenStream,
    pub(super) static_cursor: &'a TokenStream,
}

pub(super) struct Input<'a> {
    pub(super) names: Names<'a>,
    pub(super) types: Types<'a>,
    pub(super) operation: Operation<'a>,
    pub(super) mode: Mode<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) inherent: TokenStream,
    pub(super) encode_trait: TokenStream,
}

#[path = "interface/direct.rs"]
mod direct;
#[path = "interface/operation.rs"]
mod operation;

/// Renders either the direct or operation-bound interface surface.
pub(super) fn render(input: Input<'_>) -> Output {
    if input.operation.input_ty.is_some() {
        operation::render(input)
    } else {
        direct::render(input)
    }
}
