mod bitfield;
mod builder;
mod computed;
mod enumeration;
mod model;
mod recursive;
mod view;
mod writer;

use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;

pub(super) fn render_view(
    input: syn::DeriveInput,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    if enumeration::is_enum(&input) {
        enumeration::render_view(input, runtime)
    } else if bitfield::is_bitfield(&input) {
        bitfield::render_view(input, runtime)
    } else {
        let schema = model::Schema::parse(input, "WireView")?;
        let recursive = recursive::render_bodies(&schema, runtime)?;
        let ordinary = view::render(&schema, runtime)?;
        Ok(quote!(#ordinary #recursive))
    }
}

pub(super) fn render_builder(
    input: syn::DeriveInput,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    if enumeration::is_enum(&input) {
        enumeration::render_builder(input, runtime)
    } else if bitfield::is_bitfield(&input) {
        bitfield::render_builder(input, runtime)
    } else {
        let schema = model::Schema::parse(input, "WireBuilder")?;
        let detached = builder::render(&schema, runtime)?;
        let progressive = writer::render(&schema, runtime);
        Ok(quote!(#detached #progressive))
    }
}

fn pascal(identifier: &syn::Ident) -> syn::Ident {
    let text = identifier.unraw().to_string();
    let mut output = String::with_capacity(text.len());
    let mut uppercase = true;
    for character in text.chars() {
        if character == '_' {
            uppercase = true;
        } else if uppercase {
            output.extend(character.to_uppercase());
            uppercase = false;
        } else {
            output.push(character);
        }
    }
    format_ident!("{output}")
}

fn fresh_type_ident(generics: &syn::Generics, stem: &str) -> syn::Ident {
    for suffix in 0usize.. {
        let candidate = if suffix == 0 {
            format_ident!("__WireRepr{stem}", span = Span::mixed_site())
        } else {
            format_ident!("__WireRepr{stem}{suffix}", span = Span::mixed_site())
        };
        let taken = generics.params.iter().any(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => parameter.ident == candidate,
            syn::GenericParam::Const(parameter) => parameter.ident == candidate,
            syn::GenericParam::Lifetime(_) => false,
        });
        if !taken {
            return candidate;
        }
    }
    unreachable!("usize suffix space is finite but cannot be exhausted by Rust generics")
}
fn fresh_lifetime(generics: &syn::Generics, stem: &str) -> syn::Lifetime {
    let mut used = BTreeSet::new();
    collect_token_identifiers(generics.to_token_stream(), &mut used);
    for suffix in 0usize.. {
        let name = if suffix == 0 {
            format!("'__wire_repr_{stem}")
        } else {
            format!("'__wire_repr_{stem}_{suffix}")
        };
        let candidate = syn::Lifetime::new(&name, Span::mixed_site());
        if !used.contains(&candidate.ident.to_string()) {
            return candidate;
        }
    }
    unreachable!("usize suffix space is finite but cannot be exhausted by Rust generics")
}

fn fresh_schema_lifetime(
    schema: &model::Schema,
    generics: &syn::Generics,
    stem: &str,
) -> syn::Lifetime {
    fresh_lifetime(generics, &format!("{}_{stem}", snake(&schema.name)))
}

fn fresh_field_ident(schema: &model::Schema, stem: &str) -> syn::Ident {
    for suffix in 0usize.. {
        let candidate = if suffix == 0 {
            format_ident!("__wire_repr_{stem}", span = Span::mixed_site())
        } else {
            format_ident!("__wire_repr_{stem}_{suffix}", span = Span::mixed_site())
        };
        let taken = schema.fields.iter().any(|field| field.name == candidate);
        if !taken {
            return candidate;
        }
    }
    unreachable!("usize suffix space is finite but cannot be exhausted by Rust fields")
}

fn private_ident(schema: &model::Schema, stem: &str) -> syn::Ident {
    let schema_name = snake(&schema.name);
    let used = schema_source_identifiers(schema);
    for suffix in 0usize.. {
        let text = if suffix == 0 {
            format!("__wire_repr_{schema_name}_{stem}")
        } else {
            format!("__wire_repr_{schema_name}_{stem}_{suffix}")
        };
        if !used.contains(&text) {
            return format_ident!("{text}", span = Span::mixed_site());
        }
    }
    unreachable!("usize suffix space cannot be exhausted by schema identifiers")
}

fn schema_source_identifiers(schema: &model::Schema) -> BTreeSet<String> {
    let mut identifiers = BTreeSet::new();
    identifiers.insert(schema.name.unraw().to_string());
    collect_token_identifiers(schema.generics.to_token_stream(), &mut identifiers);
    for field in &schema.fields {
        identifiers.insert(field.name.unraw().to_string());
        collect_token_identifiers(field.ty.to_token_stream(), &mut identifiers);
        if let Some(constant) = field.kind.constant() {
            collect_token_identifiers(constant.to_token_stream(), &mut identifiers);
        }
    }
    for validator in &schema.validators {
        collect_token_identifiers(validator.to_token_stream(), &mut identifiers);
    }
    identifiers
}

fn collect_token_identifiers(tokens: TokenStream, identifiers: &mut BTreeSet<String>) {
    for token in tokens {
        match token {
            TokenTree::Group(group) => {
                collect_token_identifiers(group.stream(), identifiers);
            }
            TokenTree::Ident(identifier) => {
                identifiers.insert(identifier.unraw().to_string());
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

fn snake(identifier: &syn::Ident) -> String {
    let text = identifier.unraw().to_string();
    let mut output = String::with_capacity(text.len());
    for (index, character) in text.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.extend(character.to_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}
fn view_offset(offset: &model::LayoutOffset, runtime: &TokenStream) -> TokenStream {
    let parts = offset.terms.iter().map(|term| match term {
        model::SizeTerm::Fixed(width) => quote!(Some(#width)),
        model::SizeTerm::Expr(width) => quote!(Some(#width)),
        model::SizeTerm::Nested(ty) => quote!(<#ty as #runtime::WireView>::FIXED_SIZE),
        model::SizeTerm::Dynamic => quote!(None),
    });
    quote!(#runtime::__private::checked_optional_sum([#(#parts),*]))
}
fn builder_offset(offset: &model::LayoutOffset, runtime: &TokenStream) -> TokenStream {
    let parts = offset.terms.iter().map(|term| match term {
        model::SizeTerm::Fixed(width) => quote!(Some(#width)),
        model::SizeTerm::Expr(width) => quote!(Some(#width)),
        model::SizeTerm::Nested(ty) => quote!(<#ty as #runtime::WireBuilder>::FIXED_SIZE),
        model::SizeTerm::Dynamic => quote!(None),
    });
    quote!(#runtime::__private::checked_optional_sum([#(#parts),*]))
}

fn view_optional_size(schema: &model::Schema, runtime: &TokenStream) -> TokenStream {
    let parts = schema.size_terms().into_iter().map(|term| match term {
        model::SizeTerm::Fixed(width) => quote!(Some(#width)),
        model::SizeTerm::Expr(width) => quote!(Some(#width)),
        model::SizeTerm::Nested(ty) => quote!(<#ty as #runtime::WireView>::FIXED_SIZE),
        model::SizeTerm::Dynamic => quote!(None),
    });
    quote!(#runtime::__private::checked_optional_sum([#(#parts),*]))
}

fn builder_optional_size(schema: &model::Schema, runtime: &TokenStream) -> TokenStream {
    let parts = schema.size_terms().into_iter().map(|term| match term {
        model::SizeTerm::Fixed(width) => quote!(Some(#width)),
        model::SizeTerm::Expr(width) => quote!(Some(#width)),
        model::SizeTerm::Nested(ty) => quote!(<#ty as #runtime::WireBuilder>::FIXED_SIZE),
        model::SizeTerm::Dynamic => quote!(None),
    });
    quote!(#runtime::__private::checked_optional_sum([#(#parts),*]))
}

fn scalar_type_tokens(ty: model::ScalarType) -> TokenStream {
    match ty {
        model::ScalarType::U8 => quote!(u8),
        model::ScalarType::I8 => quote!(i8),
        model::ScalarType::U16 => quote!(u16),
        model::ScalarType::I16 => quote!(i16),
        model::ScalarType::U32 => quote!(u32),
        model::ScalarType::I32 => quote!(i32),
        model::ScalarType::U64 => quote!(u64),
        model::ScalarType::I64 => quote!(i64),
        model::ScalarType::U128 => quote!(u128),
        model::ScalarType::I128 => quote!(i128),
        model::ScalarType::F32 => quote!(f32),
        model::ScalarType::F64 => quote!(f64),
    }
}

fn value_type_tokens(ty: model::ValueType) -> TokenStream {
    match ty {
        model::ValueType::Scalar(ty) => scalar_type_tokens(ty),
        model::ValueType::Usize => quote!(usize),
        model::ValueType::Isize => quote!(isize),
        model::ValueType::Bool => quote!(bool),
        model::ValueType::Char => quote!(char),
    }
}

fn from_bytes_method(endian: model::Endian) -> syn::Ident {
    match endian {
        model::Endian::Native => format_ident!("from_ne_bytes"),
        model::Endian::Little => format_ident!("from_le_bytes"),
        model::Endian::Big => format_ident!("from_be_bytes"),
    }
}

fn to_bytes_method(endian: model::Endian) -> syn::Ident {
    match endian {
        model::Endian::Native => format_ident!("to_ne_bytes"),
        model::Endian::Little => format_ident!("to_le_bytes"),
        model::Endian::Big => format_ident!("to_be_bytes"),
    }
}
