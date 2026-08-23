//! Derive source model.

mod bitfield;
mod codec;
mod enumeration;
mod preparation;
mod structure;
mod syntax;

pub(super) use enumeration::parse_enum_tag;
pub(super) use preparation::{
    Computation, ComputationArgument, ComputationByteSelection, ComputationFieldPath,
    StructPreparation, validate_byte_fields, validate_computations, validate_positions,
};

use proc_macro2::TokenStream;
use syn::{Data, DeriveInput, GenericParam, Generics, Ident, Lifetime, Path, Type, Visibility};

pub(super) enum WireType {
    Struct(Box<WireStruct>),
    Enum(WireEnum),
    Bitfield(WireBitfield),
}

pub(super) struct WireBitfield {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) storage: TagCodec,
    pub(super) fields: Vec<BitfieldField>,
}

pub(super) struct BitfieldField {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) start: u32,
    pub(super) end: u32,
}

pub(super) struct WireStruct {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) wire_lifetime: Option<Lifetime>,
    pub(super) operation_input: Option<OperationInput>,
    pub(super) validators: Vec<Path>,
    pub(super) validation_error: Option<Type>,
    pub(super) fields: Vec<Field>,
    pub(super) preparation: StructPreparation,
}

pub(super) struct WireEnum {
    pub(super) vis: Visibility,
    pub(super) name: Ident,
    pub(super) wire_lifetime: Option<Lifetime>,
    pub(super) tag: EnumTag,
    pub(super) unknown: UnknownPolicy,
    pub(super) operation_input: Option<OperationInput>,
    pub(super) variants: Vec<Variant>,
}

pub(super) struct Field {
    pub(super) name: Ident,
    pub(super) ty: Type,
    pub(super) kind: FieldKind,
    pub(super) position: Option<FieldPosition>,
    pub(super) padding_before: usize,
    pub(super) alignment_before: Option<usize>,
    pub(super) operation_input: Option<Ident>,
    pub(super) validators: Vec<Path>,
    pub(super) computation: Option<Computation>,
}

pub(super) enum FieldPosition {
    Static(usize),
    Source(Ident),
}

pub(super) enum FieldKind {
    Fixed(Codec),
    Nested,
    Prefix(Path),
    Bytes { source: Ident },
    Rest,
}

pub(super) struct Variant {
    pub(super) name: Ident,
    pub(super) selector: VariantSelector,
    pub(super) operation_selector: Option<Path>,
    pub(super) body: Option<Type>,
}

pub(super) enum VariantSelector {
    Integer(u128),
    Bytes(Vec<u8>),
    Unknown,
    Dynamic,
}

pub(super) enum EnumTag {
    Integer(TagCodec),
    Bytes { width: usize },
}

#[derive(Clone, Copy)]
pub(super) enum UnknownPolicy {
    Reject,
    Preserve,
}

pub(super) struct OperationInput {
    pub(super) name: Ident,
    pub(super) ty: Path,
    pub(super) error: Option<Path>,
}

pub(super) enum Codec {
    Builtin(&'static str),
    OwnedBytes(TokenStream),
    Custom(Path),
}

pub(super) struct TagCodec {
    pub(super) codec: String,
    pub(super) builtin: bool,
    pub(super) ty: &'static str,
    pub(super) max: u128,
}

impl WireType {
    pub(super) fn parse(input: DeriveInput) -> syn::Result<Self> {
        match input.data {
            Data::Struct(data) => structure::parse(
                input.attrs,
                input.generics,
                input.vis,
                input.ident,
                data.fields,
            ),
            Data::Enum(data) => enumeration::parse(
                input.attrs,
                input.generics,
                input.vis,
                input.ident,
                data.variants,
            ),
            _ => Err(syn::Error::new_spanned(
                input.ident,
                "Wire supports named structs and enums only",
            )),
        }
    }
}

fn parse_wire_lifetime(generics: &Generics, owner: &str) -> syn::Result<Option<Lifetime>> {
    if generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            generics,
            format!("{owner} do not support where clauses"),
        ));
    }

    let mut lifetime = None;
    for parameter in &generics.params {
        match parameter {
            GenericParam::Lifetime(parameter)
                if lifetime.is_none() && parameter.bounds.is_empty() =>
            {
                lifetime = Some(parameter.lifetime.clone());
            }
            GenericParam::Lifetime(_) => {
                return Err(syn::Error::new_spanned(
                    parameter,
                    format!("{owner} support exactly one unbounded wire lifetime"),
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    parameter,
                    format!("{owner} do not support type or const parameters"),
                ));
            }
        }
    }
    Ok(lifetime)
}

#[cfg(test)]
mod tests;
