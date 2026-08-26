use proc_macro2::TokenStream;
use quote::quote;
use syn::{Expr, ExprCall, ExprPath, Ident};

use super::model::Computed;

pub(super) fn render_call(
    computed: &Computed,
    view: &Ident,
    destination: &Ident,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let Expr::Call(call) = &computed.expression else {
        return Err(syn::Error::new_spanned(
            &computed.expression,
            "computed callback must be a function call",
        ));
    };
    let function = &call.func;
    let arguments = call
        .args
        .iter()
        .map(|argument| render_argument(argument, view, destination, runtime))
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote!(#function(#(#arguments),*)))
}

fn render_argument(
    argument: &Expr,
    view: &Ident,
    destination: &Ident,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    if let Expr::Call(call) = argument {
        return render_selection(call, view, destination, runtime);
    }
    if let Expr::Path(path) = argument {
        let field = simple_ident(path)?;
        let field = if field == "self" { destination } else { field };
        return Ok(quote!(#view.#field()));
    }
    Err(syn::Error::new_spanned(
        argument,
        "computed arguments must be logical fields or include/exclude selections",
    ))
}

fn render_selection(
    call: &ExprCall,
    view: &Ident,
    destination: &Ident,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let Expr::Path(function) = call.func.as_ref() else {
        return Err(syn::Error::new_spanned(
            &call.func,
            "selection must use include(...) or exclude(...)",
        ));
    };
    let operation = simple_ident(function)?;
    if operation != "include" && operation != "exclude" {
        return Err(syn::Error::new_spanned(
            operation,
            "selection must use include(...) or exclude(...)",
        ));
    }
    if call.args.is_empty() {
        return Err(syn::Error::new_spanned(
            call,
            "selection requires at least one field",
        ));
    }
    let fields = call
        .args
        .iter()
        .map(|argument| {
            let Expr::Path(path) = argument else {
                return Err(syn::Error::new_spanned(
                    argument,
                    "selection fields must be simple field names",
                ));
            };
            let field = simple_ident(path)?;
            Ok(if field == "self" { destination } else { field })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let first = fields.first().expect("nonempty selection");
    let rest = &fields[1..];
    Ok(quote! {
        #runtime::select(&#view)
            .#operation(|fields| fields.#first #(| fields.#rest)*)
    })
}

fn simple_ident(path: &ExprPath) -> syn::Result<&Ident> {
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            path,
            "expected a simple field name",
        ));
    }
    Ok(&path.path.segments[0].ident)
}
