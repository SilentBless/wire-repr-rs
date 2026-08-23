use super::WireType;

pub(super) fn parse(source: &str) -> syn::Result<WireType> {
    WireType::parse(syn::parse_str(source).unwrap())
}

mod bitfield;
mod computation;
mod enumeration;
mod structure;
