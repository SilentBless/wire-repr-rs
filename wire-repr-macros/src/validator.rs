//! Validator callback metadata attribute.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{GenericArgument, ItemFn, PathArguments, ReturnType, Type};

pub(crate) fn error_type(callback: &syn::Path) -> syn::Result<syn::Path> {
    let mut marker: syn::Path = syn::parse2(quote!(#callback))?;
    let Some(segment) = marker.segments.last_mut() else {
        return Err(syn::Error::new_spanned(
            callback,
            "validator path cannot be empty",
        ));
    };
    if !matches!(segment.arguments, PathArguments::None) {
        return Err(syn::Error::new_spanned(
            segment,
            "wire validator paths cannot have generic arguments",
        ));
    }
    marker
        .segments
        .push(syn::PathSegment::from(format_ident!("Error")));
    Ok(marker)
}

pub(super) fn expand(function: ItemFn) -> syn::Result<TokenStream> {
    if !function.sig.generics.params.is_empty() || function.sig.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "wire validators cannot be generic",
        ));
    }

    let error = validator_error_type(&function.sig.output)?;
    let visibility = &function.vis;
    let marker = &function.sig.ident;
    let marker_configuration = function.attrs.iter().filter(|attribute| {
        attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
    });

    Ok(quote! {
        #function

        #(#marker_configuration)*
        #[doc(hidden)]
        #[allow(missing_docs)]
        #visibility mod #marker {
            use super::*;

            pub type Error = #error;
        }
    })
}

fn validator_error_type(output: &ReturnType) -> syn::Result<&Type> {
    let ReturnType::Type(_, ty) = output else {
        return Err(syn::Error::new_spanned(
            output,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    let Type::Path(result) = ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            ty,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    let Some(segment) = result.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            result,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    if segment.ident != "Result" {
        return Err(syn::Error::new_spanned(
            segment,
            "wire validators must return `Result<(), Error>`",
        ));
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            segment,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let Some(ok) = types.next() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    let Some(error) = types.next() else {
        return Err(syn::Error::new_spanned(
            arguments,
            "wire validators must return `Result<(), Error>`",
        ));
    };
    if types.next().is_some()
        || !matches!(ok, Type::Tuple(tuple) if tuple.elems.is_empty())
        || arguments.args.len() != 2
    {
        return Err(syn::Error::new_spanned(
            arguments,
            "wire validators must return `Result<(), Error>`",
        ));
    }
    Ok(error)
}

#[cfg(test)]
mod tests {
    use super::{error_type, expand};
    use quote::quote;

    #[test]
    fn exposes_the_callback_error_through_hidden_metadata() {
        let function = syn::parse2(quote! {
            #[cfg(feature = "bytes")]
            pub(crate) fn nonzero(value: u8) -> Result<(), DomainError> {
                let _ = value;
                Ok(())
            }
        })
        .unwrap();
        let expanded = expand(function).unwrap().to_string();

        assert!(expanded.contains("fn nonzero"));
        assert!(expanded.contains("pub (crate) mod nonzero"));
        assert!(expanded.contains("pub type Error = DomainError"));
        assert_eq!(expanded.matches("cfg").count(), 2);
    }

    #[test]
    fn rejects_missing_or_generic_error_contracts() {
        for function in [
            quote!(
                fn invalid() {}
            ),
            quote!(
                fn invalid() -> bool {
                    true
                }
            ),
            quote!(
                fn invalid<T>(value: T) -> Result<(), Error> {
                    let _ = value;
                    Ok(())
                }
            ),
            quote!(
                fn invalid() -> Result<u8, Error> {
                    Ok(0)
                }
            ),
        ] {
            let function = syn::parse2(function).unwrap();
            assert!(expand(function).is_err());
        }
    }

    #[test]
    fn derives_metadata_paths_without_losing_qualification() {
        let callback: syn::Path = syn::parse_quote!(crate::checks::nonzero);
        let error = error_type(&callback).unwrap();
        assert_eq!(
            quote!(#error).to_string(),
            "crate :: checks :: nonzero :: Error"
        );
    }
}
