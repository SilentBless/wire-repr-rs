use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{Expr, ExprCall, ExprPath, Ident, Member};

use super::model::{Computed, FieldKind, Schema};

pub(super) fn requires_view(computed: &Computed) -> bool {
    !matches!(&computed.expression, Expr::Call(call) if call.args.is_empty())
}

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
    let paths = call
        .args
        .iter()
        .map(|argument| selection_path(argument, destination))
        .collect::<syn::Result<Vec<_>>>()?;
    let rendered = paths
        .iter()
        .map(|path| render_selection_path(path, quote!(fields), 0))
        .collect::<Vec<_>>();
    let first = rendered.first().expect("nonempty selection");
    let rest = &rendered[1..];
    Ok(quote! {
        #runtime::select(&#view)
            .#operation(|fields| #first #(| #rest)*)
    })
}

fn selection_path(argument: &Expr, destination: &Ident) -> syn::Result<Vec<Ident>> {
    fn collect(expression: &Expr, output: &mut Vec<Ident>) -> syn::Result<()> {
        match expression {
            Expr::Path(path) => {
                output.push(simple_ident(path)?.clone());
                Ok(())
            }
            Expr::Field(field) => {
                collect(&field.base, output)?;
                let Member::Named(name) = &field.member else {
                    return Err(syn::Error::new_spanned(
                        &field.member,
                        "selection paths require named fields",
                    ));
                };
                output.push(name.clone());
                Ok(())
            }
            _ => Err(syn::Error::new_spanned(
                expression,
                "selection fields must be physical field paths",
            )),
        }
    }

    let mut path = Vec::new();
    collect(argument, &mut path)?;
    if path.len() == 1 && path[0] == "self" {
        path[0] = destination.clone();
    } else if path.iter().any(|field| field == "self") {
        return Err(syn::Error::new_spanned(
            argument,
            "`self` is only valid as a root selection field",
        ));
    }
    Ok(path)
}

fn render_selection_path(path: &[Ident], base: TokenStream, depth: usize) -> TokenStream {
    let field = &path[0];
    if path.len() == 1 {
        return quote!(#base.#field);
    }
    let nested = syn::Ident::new(&format!("__wire_repr_nested_fields_{depth}"), field.span());
    let rest = render_selection_path(&path[1..], quote!(#nested), depth + 1);
    quote!(#base.#field.fields(|#nested| #rest))
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

#[derive(Clone, Copy)]
pub(super) enum PatchWriter<'a> {
    Detached { start: &'a Ident },
    Progressive,
}

pub(super) fn render_patches(
    schema: &Schema,
    runtime: &TokenStream,
    self_type: &TokenStream,
    error: &Ident,
    writer: PatchWriter<'_>,
) -> Vec<TokenStream> {
    schema
        .computed_fields()
        .filter_map(|field| {
            let computed = field.kind.computed()?;
            let FieldKind::Scalar(scalar) = &field.kind else {
                unreachable!("computed destination is scalar")
            };
            let name = &field.name;
            let field_name = name.unraw().to_string();
            let offset = super::builder_offset(&field.offset, runtime);
            let view = super::private_ident(
                schema,
                &format!("{}_computed_view", name.unraw()),
            );
            let semantic = super::private_ident(
                schema,
                &format!("{}_computed_value", name.unraw()),
            );
            let value_ty = super::value_type_tokens(&scalar.value_type);
            let wire_ty = super::scalar_type_tokens(scalar.wire_type);
            let encode = super::to_bytes_method(scalar.endian);
            let layout_error = quote!(
                #runtime::WriteError::Schema(
                    #error::Layout(#runtime::LayoutError { field: #field_name })
                )
            );
            let call = render_call(computed, &view, name, runtime)
                .expect("validated computed callback expression");
            let calculate = if computed.error.is_some() {
                let variant = computed_error_ident(name);
                quote!(#call.map_err(|error| #runtime::WriteError::Schema(
                    #error::#variant(error)
                ))?)
            } else {
                call
            };
            let view_binding = if !requires_view(computed) {
                TokenStream::new()
            } else {
                match writer {
                    PatchWriter::Detached { start } => quote! {
                        let bytes = writer.bytes_from(#start).ok_or_else(|| { #layout_error })?;
                        let #view = <#self_type as #runtime::__private::WireSelect>::select_view(bytes)
                            .map_err(|_| #layout_error)?;
                    },
                    PatchWriter::Progressive => quote! {
                        let #view = <#self_type as #runtime::__private::WireSelect>::select_view(
                            self.writer.as_bytes(),
                        )
                        .map_err(|_| #layout_error)?;
                    },
                }
            };
            let encoded = if scalar.value_type.is_converted() {
                let conversion =
                    super::write_fields::convert_to_wire(scalar, &semantic, &wire_ty);
                quote!(#conversion.ok_or_else(|| #layout_error)?)
            } else {
                quote!(#semantic)
            };
            let patch = match writer {
                PatchWriter::Detached { start } => quote! {
                    let relative = #offset.ok_or_else(|| #layout_error)?;
                    let offset = #start.checked_add(relative).ok_or_else(|| { #layout_error })?;
                    writer.patch_at(offset, &encoded.#encode())?;
                },
                PatchWriter::Progressive => quote! {
                    let offset = #offset.ok_or_else(|| #layout_error)?;
                    self.writer.write_at(offset, &encoded.#encode())?;
                },
            };
            Some(quote! {
                let #semantic: #value_ty = {
                    #view_binding
                    #calculate
                };
                let encoded: #wire_ty = #encoded;
                #patch
            })
        })
        .collect()
}

pub(super) fn computed_error_ident(field: &syn::Ident) -> syn::Ident {
    format_ident!("{}Computed", super::pascal(field))
}
