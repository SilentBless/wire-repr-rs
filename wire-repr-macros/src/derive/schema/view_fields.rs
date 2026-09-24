use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn render_wire_fields(
    impl_header: TokenStream,
    runtime: &TokenStream,
    schema: &TokenStream,
    fields: &TokenStream,
    range: &TokenStream,
    state: TokenStream,
    inline: bool,
) -> TokenStream {
    let inline = inline.then(|| quote!(#[inline(always)]));
    quote! {
        // SAFETY: the view pairs an exact framed input with the state used to resolve routes.
        #[allow(unsafe_code)]
        #impl_header {
            type Fields = #fields;
            type SelectionRoot = #schema;

            #inline
            fn fields(&self) -> Self::Fields {
                // SAFETY: the generated root prefix matches this view's SelectionRoot.
                unsafe {
                    <#schema as #runtime::__private::WireFieldSchema>::fields::<
                        #runtime::__private::FieldRouteEnd<#schema>
                    >()
                }
            }

            #inline
            fn field_range(&self, index: usize) -> Option<::core::ops::Range<usize>> {
                #range
            }

            unsafe fn resolve_field_route<Route>(&self) -> Option<::core::ops::Range<usize>>
            where
                Route: #runtime::__private::FieldRoute<Root = Self::SelectionRoot>,
            {
                // SAFETY: route resolution uses this view's input and the state framed for it.
                unsafe { Route::resolve::<#schema>(self.as_ref(), #state) }
            }
        }
    }
}
