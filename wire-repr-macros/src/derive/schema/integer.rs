mod controller;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::model::{Scalar, ScalarEncoding};

pub(super) use controller::{
    detached_expansions, detached_state, progressive_expansions, render_count_patch,
    render_detached_length_patch, render_length_patch, retained_controller_offsets,
    variable_controllers,
};

pub(super) fn error_suffix(scalar: &Scalar) -> &'static str {
    match scalar.encoding {
        ScalarEncoding::Uleb128 => "Uleb128",
        ScalarEncoding::Sleb128 => "Sleb128",
        ScalarEncoding::Vu128 => "Vu128",
        ScalarEncoding::Fixed => unreachable!("fixed scalars have no variable integer error"),
    }
}

pub(super) fn decode_scalar(
    scalar: &Scalar,
    input: TokenStream,
    absolute_offset: TokenStream,
    runtime: &TokenStream,
) -> TokenStream {
    let bits = (scalar.width() * 8) as u32;
    let decode = match scalar.encoding {
        ScalarEncoding::Uleb128 => {
            quote!(#runtime::__private::decode_uleb128::<#bits>(#input, #absolute_offset))
        }
        ScalarEncoding::Sleb128 => {
            quote!(#runtime::__private::decode_sleb128::<#bits>(#input, #absolute_offset))
        }
        ScalarEncoding::Vu128 => {
            quote!(#runtime::__private::decode_vu128::<#bits>(#input, #absolute_offset))
        }
        ScalarEncoding::Fixed => unreachable!("fixed scalars do not use variable integer decoding"),
    };
    if scalar.zigzag {
        quote!(#decode.map(|(value, len)| (#runtime::__private::zigzag_decode(value), len)))
    } else {
        decode
    }
}

pub(super) fn decode_field(
    scalar: &Scalar,
    input: &syn::Ident,
    start: &syn::Ident,
    absolute_offset: &syn::Ident,
    input_end: &syn::Ident,
    available: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    let decode = decode_scalar(
        scalar,
        quote!(#available),
        quote!(#absolute_offset),
        runtime,
    );
    let error = format_ident!("{}Error", error_suffix(scalar));
    quote! {
        #input.get(#start..)
            .ok_or_else(|| #runtime::#error::NeedMore(#runtime::NeedMore {
                offset: #input_end,
                additional_at_least: #start
                    .saturating_sub(#input.len())
                    .saturating_add(1),
            }))
            .and_then(|#available| #decode)
    }
}

pub(super) fn read_error_variant(
    scalar: &Scalar,
    variant: &syn::Ident,
    message: &str,
    runtime: &TokenStream,
) -> TokenStream {
    let error = format_ident!("{}Error", error_suffix(scalar));
    quote! {
        #[doc = "The field contains an incomplete or overflowing variable-width integer."]
        #[error(#message)]
        #variant(#[source] #runtime::#error),
    }
}

pub(super) fn flatten_read_error(scalar: &Scalar, runtime: &TokenStream) -> TokenStream {
    let error = format_ident!("{}Error", error_suffix(scalar));
    quote! {
        match source {
            #runtime::#error::NeedMore(source) => {
                #runtime::__private::RecursiveError::NeedMore(source)
            }
            #runtime::#error::Overflow { offset, .. } => {
                #runtime::__private::RecursiveError::Child { offset }
            }
        }
    }
}

pub(super) fn scalar_value(
    scalar: &Scalar,
    start: &TokenStream,
    end: &TokenStream,
    runtime: &TokenStream,
) -> TokenStream {
    let wire_ty = super::scalar_type_tokens(scalar.wire_type);
    let decode = decode_scalar(
        scalar,
        quote!(&self.as_bytes()[#start..#end]),
        quote!(0),
        runtime,
    );
    quote!(#decode.expect("framed variable-width integer is valid").0 as #wire_ty)
}

pub(super) fn encode_scalar(
    scalar: &Scalar,
    value: TokenStream,
    runtime: &TokenStream,
) -> TokenStream {
    let value = if scalar.zigzag {
        quote!(#runtime::__private::zigzag_encode((#value) as i128))
    } else if scalar.encoding == ScalarEncoding::Sleb128 {
        quote!((#value) as i128)
    } else {
        quote!((#value) as u128)
    };
    match scalar.encoding {
        ScalarEncoding::Uleb128 => quote!(#runtime::__private::encode_uleb128(#value)),
        ScalarEncoding::Sleb128 => quote!(#runtime::__private::encode_sleb128(#value)),
        ScalarEncoding::Vu128 => quote!(#runtime::__private::encode_vu128(#value)),
        ScalarEncoding::Fixed => unreachable!("fixed scalars do not use variable integer encoding"),
    }
}

pub(super) fn scalar_bytes(
    scalar: &Scalar,
    value: &syn::Ident,
    runtime: &TokenStream,
) -> TokenStream {
    if scalar.is_variable() {
        let encoded = encode_scalar(scalar, quote!(#value), runtime);
        quote!(#encoded.as_ref())
    } else {
        let encode = super::to_bytes_method(scalar.endian);
        quote!(&#value.#encode())
    }
}
