mod recursive;

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::ext::IdentExt;
use syn::{
    Data, DeriveInput, Expr, Fields, GenericParam, Generics, Ident, Type, Visibility, parse_quote,
};

use super::model::{Endian, ScalarType};
use super::{
    fresh_lifetime, fresh_type_ident, from_bytes_method, scalar_type_tokens, to_bytes_method,
};

pub(super) fn is_enum(input: &DeriveInput) -> bool {
    matches!(input.data, Data::Enum(_))
}

pub(super) fn render_view(input: DeriveInput, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let schema = EnumSchema::parse(input, "WireView")?;
    let recursive_slots = schema
        .known_variants()
        .map(|variant| super::recursive::root_slot_marker(&variant.body, &schema.name, runtime))
        .collect::<syn::Result<Vec<_>>>()?;
    if recursive_slots.iter().any(Option::is_some) {
        return recursive::render_view(schema, runtime, recursive_slots);
    }
    let vis = &schema.vis;
    let name = &schema.name;
    let view_trait = format_ident!("{}View", name.unraw());
    let view_error = format_ident!("{}ViewError", name.unraw());
    let variant_view = format_ident!("{}Variant", name.unraw());
    let state = format_ident!("__WireRepr{}ViewState", name.unraw());
    let fields_type = format_ident!("{}Fields", name.unraw());
    let view_impl = format_ident!("__WireRepr{}ViewImpl", name.unraw());
    let selector_ty = scalar_type_tokens(schema.selector);
    let selector_width = schema.selector.width();
    let decode = from_bytes_method(schema.endian);
    let view_lifetime = fresh_lifetime(&schema.generics, "enum_view");
    let variant_lifetime = fresh_lifetime(&schema.generics, "enum_variant");
    let method_lifetime = fresh_lifetime(&schema.generics, "enum_field");
    let sequence_lifetime = fresh_lifetime(&schema.generics, "enum_sequence");
    let backing = fresh_type_ident(&schema.generics, "Backing");

    let known = schema.known_variants().collect::<Vec<_>>();
    let type_stems = variant_type_stems(&known, &[]);
    let error_variant_names = type_stems
        .iter()
        .map(|stem| format_ident!("Variant{stem}"))
        .collect::<Vec<_>>();
    let selector_values = known
        .iter()
        .map(|variant| variant.value.as_ref().expect("known selector"))
        .collect::<Vec<_>>();
    let selector_count = selector_values.len();
    let selector_validation = quote! {
        let _values: [#selector_ty; #selector_count] = [#(#selector_values),*];
    };
    let duplicate_checks = selector_values
        .iter()
        .enumerate()
        .flat_map(|(left, value)| {
            let selector_ty = &selector_ty;
            selector_values.iter().skip(left + 1).map(move |other| {
                quote!({
                    let left: #selector_ty = #value;
                    let right: #selector_ty = #other;
                    left == right
                })
            })
        })
        .collect::<Vec<_>>();
    let duplicate_selector = quote!(false #(|| #duplicate_checks)*);
    let state_parameters = (0..known.len())
        .map(|index| format_ident!("__WireReprState{index}"))
        .collect::<Vec<_>>();
    let error_parameters = (0..known.len())
        .map(|index| format_ident!("__WireReprError{index}"))
        .collect::<Vec<_>>();
    let view_parameters = (0..known.len())
        .map(|index| format_ident!("__WireReprView{index}"))
        .collect::<Vec<_>>();

    let mut bounded = schema.generics.clone();
    for variant in &known {
        let body = &variant.body;
        bounded
            .make_where_clause()
            .predicates
            .push(parse_quote!(#body: #runtime::WireView));
    }
    let (impl_generics, type_generics, where_clause) = bounded.split_for_impl();
    let self_type = quote!(#name #type_generics);
    let marker = quote!(fn() -> #self_type);
    let state_types = known.iter().map(|variant| {
        let body = &variant.body;
        quote!(<#body as #runtime::WireView>::State)
    });
    let error_types = known.iter().map(|variant| {
        let body = &variant.body;
        quote!(<#body as #runtime::WireView>::Error)
    });
    let state_type = quote!(#state<#(#state_types),*>);
    let error_type = quote!(#view_error<#(#error_types),*>);

    let state_variants = known
        .iter()
        .zip(&state_parameters)
        .map(|(variant, parameter)| {
            let variant_name = &variant.name;
            quote!(#variant_name(#parameter),)
        });
    let view_variants = known
        .iter()
        .zip(&view_parameters)
        .map(|(variant, parameter)| {
            let variant_name = &variant.name;
            quote!(#variant_name(#parameter),)
        });
    let error_variants = known
        .iter()
        .zip(&error_parameters)
        .zip(&error_variant_names)
        .map(|((variant, parameter), error_variant)| {
            let message = format!("enum variant `{}` failed", variant.name.unraw());
            quote! {
                #[error(#message)]
                #error_variant(#[source] #parameter),
            }
        });
    let unknown_state = schema.unknown().map(|_| quote!(Unknown(#selector_ty),));
    let unknown_view = schema.unknown().map(|_| {
        quote! {
            Unknown {
                selector: #selector_ty,
                body: &#variant_lifetime [u8],
            },
        }
    });

    let state_declaration = quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        #vis enum #state<#(#state_parameters),*> {
            #(#state_variants)*
            #unknown_state
        }
    };
    let variant_declaration_generics = if schema.unknown().is_some() {
        quote!(<#variant_lifetime, #(#view_parameters),*>)
    } else {
        quote!(<#(#view_parameters),*>)
    };
    let variant_declaration = quote! {
        #[doc = "Borrowed static enum variant generated for this schema."]
        #[allow(non_camel_case_types)]
        #vis enum #variant_view #variant_declaration_generics {
            #(#view_variants)*
            #unknown_view
        }
    };
    let recursive_error_arms = error_variant_names
        .iter()
        .map(|variant| {
            quote!(Self::#variant(source) => {
                source.flatten_recursive(fallback_offset)
            },)
        })
        .collect::<Vec<_>>();
    let error_declaration = quote! {
        #[derive(Debug, #runtime::__private::ThisError)]
        #vis enum #view_error<#(#error_parameters: ::core::error::Error + 'static),*> {
            #[error(transparent)]
            NeedMore(#[from] #runtime::NeedMore),
            #[error(transparent)]
            Layout(#[from] #runtime::LayoutError),
            #[error(transparent)]
            InvalidFrame(#[from] #runtime::InvalidFrameExtent),
            #[error(transparent)]
            Trailing(#[from] #runtime::TrailingBytes),
            #(#error_variants)*
            #[error("unknown selector {selector} at byte offset {offset}")]
            UnknownSelector { selector: #selector_ty, offset: usize },
            #[error("enum contains duplicate selector values")]
            DuplicateSelector,
        }

        impl<#(#error_parameters),*> #runtime::__private::FlattenRecursiveError
            for #view_error<#(#error_parameters),*>
        where
            #(
                #error_parameters:
                    ::core::error::Error
                    + #runtime::__private::FlattenRecursiveError
                    + 'static,
            )*
        {
            fn flatten_recursive(
                self,
                fallback_offset: usize,
            ) -> #runtime::__private::RecursiveError {
                match self {
                    Self::NeedMore(source) => {
                        #runtime::__private::RecursiveError::NeedMore(source)
                    }
                    Self::Layout(source) => {
                        #runtime::__private::RecursiveError::Layout(source)
                    }
                    Self::InvalidFrame(source) => {
                        #runtime::__private::RecursiveError::InvalidFrame(source)
                    }
                    Self::Trailing(source) => {
                        #runtime::__private::RecursiveError::Trailing(source)
                    }
                    #(#recursive_error_arms)*
                    Self::UnknownSelector { offset, .. } => {
                        #runtime::__private::RecursiveError::UnknownSelector { offset }
                    }
                    Self::DuplicateSelector => {
                        #runtime::__private::RecursiveError::Child {
                            offset: fallback_offset,
                        }
                    }
                }
            }
        }
    };
    let wireview_flatten_arms = known
        .iter()
        .zip(&error_variant_names)
        .map(|(variant, error_variant)| {
            let body = &variant.body;
            quote!(#view_error::#error_variant(source) => {
                <#body as #runtime::WireView>::flatten_recursive_error(
                    source,
                    fallback_offset,
                )
            },)
        })
        .collect::<Vec<_>>();

    let frame_offset = format_ident!("__wire_repr_frame_offset");
    let frame_arms = known
        .iter()
        .zip(&error_variant_names)
        .map(|(variant, error_variant)| {
            let variant_name = &variant.name;
            let value = variant.value.as_ref().expect("known variant has value");
            let body = &variant.body;
            quote! {
                value if value == { let selector: #selector_ty = #value; selector } => {
                    let body_offset = #frame_offset
                        .checked_add(#selector_width)
                        .ok_or(#runtime::LayoutError {
                            field: stringify!(#variant_name),
                        })?;
                    let frame = <#body as #runtime::WireView>::frame(
                        &input[#selector_width..],
                        body_offset,
                    )
                    .map_err(#view_error::#error_variant)?;
                    let (state, body_consumed) = frame.into_parts();
                    if body_consumed > input.len() - #selector_width {
                        return Err(#view_error::InvalidFrame(#runtime::InvalidFrameExtent {
                            offset: body_offset,
                            consumed: body_consumed,
                            available: input.len() - #selector_width,
                        }));
                    }
                    let consumed = #selector_width
                        .checked_add(body_consumed)
                        .ok_or(#runtime::LayoutError {
                            field: stringify!(#variant_name),
                        })?;
                    #runtime::Frame::new(#state::#variant_name(state), consumed)
                }
            }
        });
    let unknown_frame = if schema.unknown().is_some() {
        quote!(value => #runtime::Frame::new(#state::Unknown(value), input.len()),)
    } else {
        quote! {
            value => {
                return Err(#view_error::UnknownSelector {
                    selector: value,
                    offset: #frame_offset,
                });
            }
        }
    };

    let selector_arms = known
        .iter()
        .map(|variant| {
            let variant_name = &variant.name;
            let value = variant.value.as_ref().expect("known variant has value");
            quote!(#state::#variant_name(_) => { let selector: #selector_ty = #value; selector },)
        })
        .collect::<Vec<_>>();
    let unknown_selector_arm = schema
        .unknown()
        .map(|_| quote!(#state::Unknown(value) => *value,));
    let variant_arms = known
        .iter()
        .map(|variant| {
            let variant_name = &variant.name;
            let body = &variant.body;
            quote! {
                #state::#variant_name(state) => {
                    // SAFETY: enum framing produced this state for the exact bytes after the selector.
                    let body = unsafe {
                        <#body as #runtime::WireView>::from_validated_parts(
                            &self.as_bytes()[#selector_width..],
                            state,
                        )
                    };
                    #variant_view::#variant_name(body)
                }
            }
        })
        .collect::<Vec<_>>();
    let unknown_variant_arm = schema.unknown().map(|_| {
        quote! {
            #state::Unknown(selector) => #variant_view::Unknown {
                selector: *selector,
                body: &self.as_bytes()[#selector_width..],
            },
        }
    });
    let method_body_views = known.iter().map(|variant| {
        let body = &variant.body;
        quote!(<#body as #runtime::WireView>::View<#method_lifetime>)
    });
    let method_variant_type = if schema.unknown().is_some() {
        quote!(#variant_view<#method_lifetime, #(#method_body_views),*>)
    } else {
        quote!(#variant_view<#(#method_body_views),*>)
    };
    let render_view_methods = |state_ref: TokenStream| {
        quote! {
            #[inline(always)]
            fn selector(&self) -> #selector_ty {
                match #state_ref {
                    #(#selector_arms)*
                    #unknown_selector_arm
                }
            }

            #[allow(unsafe_code)]
            #[inline(always)]
            fn variant<#method_lifetime>(
                &#method_lifetime self,
            ) -> #method_variant_type {
                match #state_ref {
                    #(#variant_arms)*
                    #unknown_variant_arm
                }
            }
        }
    };
    let retained_view_methods = render_view_methods(quote!(&self.state));
    let projected_view_methods = render_view_methods(quote!(self.state));

    let trait_body_views = known.iter().map(|variant| {
        let body = &variant.body;
        quote!(<#body as #runtime::WireView>::View<#method_lifetime>)
    });
    let trait_variant_type = if schema.unknown().is_some() {
        quote!(#variant_view<#method_lifetime, #(#trait_body_views),*>)
    } else {
        quote!(#variant_view<#(#trait_body_views),*>)
    };
    let schema_arguments = generic_arguments(&bounded);
    let field_prefix = fresh_type_ident(&bounded, "FieldPrefix");
    let mut fields_generics = bounded.clone();
    fields_generics
        .params
        .push(GenericParam::Type(parse_quote!(#field_prefix)));
    fields_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#field_prefix: #runtime::__private::FieldPrefix));
    let (fields_impl, _, fields_where) = fields_generics.split_for_impl();
    let root_fields_type = quote!(
        #fields_type<
            #(#schema_arguments,)*
            #runtime::__private::FieldRouteEnd<#self_type>
        >
    );
    let trait_declaration = quote! {
        #[doc = "Exact-source view API generated for this static enum schema."]
        #vis trait #view_trait #impl_generics:
            #runtime::__private::WireFields<
                Fields = #root_fields_type,
                SelectionRoot = #self_type,
            >
            #where_clause
        {
            fn as_bytes(&self) -> &[u8];
            fn selector(&self) -> #selector_ty;
            fn variant<#method_lifetime>(
                &#method_lifetime self,
            ) -> #trait_variant_type;
        }
    };
    let retained_type = quote!(#view_impl<#(#schema_arguments,)* #backing, #state_type, #marker>);
    let projected_type = quote!(
        #view_impl<
            #(#schema_arguments,)*
            &#view_lifetime [u8],
            &#view_lifetime #state_type,
            #marker
        >
    );
    let mut retained_generics = bounded.clone();
    retained_generics
        .params
        .push(GenericParam::Type(parse_quote!(#backing)));
    retained_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let (retained_impl, _, retained_where) = retained_generics.split_for_impl();
    let mut projected_generics = bounded.clone();
    projected_generics
        .params
        .insert(0, parse_quote!(#view_lifetime));
    let (projected_impl, _, projected_where) = projected_generics.split_for_impl();
    let holder = fresh_type_ident(&schema.generics, "Holder");
    let marker_type = fresh_type_ident(&schema.generics, "Marker");
    let fixed_size = if schema.unknown().is_some() {
        quote!(None)
    } else {
        let body_sizes = known.iter().map(|variant| {
            let body = &variant.body;
            quote!(<#body as #runtime::WireView>::FIXED_SIZE)
        });
        quote!(
            #runtime::__private::checked_optional_sum([
                Some(#selector_width),
                #runtime::__private::checked_optional_equal([#(#body_sizes),*]),
            ])
        )
    };
    let mut common_generics = bounded.clone();
    let leading_extent = if schema.unknown().is_some() {
        quote!(false)
    } else {
        let body_extents = known.iter().map(|variant| {
            let body = &variant.body;
            quote!(<#body as #runtime::WireView>::LEADING_EXTENT)
        });
        quote!(true #(&& #body_extents)*)
    };
    common_generics.params.push(parse_quote!(#backing));
    common_generics.params.push(parse_quote!(#holder));
    common_generics.params.push(parse_quote!(#marker_type));
    common_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let (common_impl, common_types, common_where) = common_generics.split_for_impl();
    let trait_path = quote!(#view_trait #type_generics);

    let sequence_methods = if schema.unknown().is_none() {
        quote! {
            /// Returns a lazy facade over consecutive static enum representations.
            #vis fn views<#sequence_lifetime>(
                input: &#sequence_lifetime [u8],
            ) -> Result<
                #runtime::VariableViews<#sequence_lifetime, Self>,
                #runtime::SequenceError<<Self as #runtime::WireView>::Error>,
            > {
                if !<Self as #runtime::WireView>::LEADING_EXTENT {
                    return Err(#runtime::SequenceError::Unavailable);
                }
                Ok(#runtime::VariableViews::new(input))
            }

            /// Frames the first enum and returns a cursor over the suffix.
            #vis fn cursor<#sequence_lifetime>(
                input: &#sequence_lifetime [u8],
            ) -> Result<
                (
                    <Self as #runtime::__private::WireSelect>::Root<&#sequence_lifetime [u8]>,
                    #runtime::Cursor<#sequence_lifetime>,
                ),
                #runtime::SequenceError<<Self as #runtime::WireView>::Error>,
            > {
                if !<Self as #runtime::WireView>::LEADING_EXTENT {
                    return Err(#runtime::SequenceError::Unavailable);
                }
                let mut cursor = #runtime::Cursor::new(input);
                let view = cursor.read::<Self>()?;
                Ok((view, cursor))
            }

            /// Frames this enum at the cursor without advancing on failure.
            #vis fn next<#sequence_lifetime>(
                cursor: &mut #runtime::Cursor<#sequence_lifetime>,
            ) -> Result<
                <Self as #runtime::__private::WireSelect>::Root<&#sequence_lifetime [u8]>,
                #runtime::SequenceError<<Self as #runtime::WireView>::Error>,
            > {
                if !<Self as #runtime::WireView>::LEADING_EXTENT {
                    return Err(#runtime::SequenceError::Unavailable);
                }
                cursor.read::<Self>()
            }
        }
    } else {
        TokenStream::new()
    };
    let leading_impl = TokenStream::new();

    Ok(quote! {
        #state_declaration
        #variant_declaration
        #error_declaration

        #[doc = "Typed physical enum paths generated for this schema."]
        #vis struct #fields_type #fields_impl #fields_where {
            pub selector: #runtime::__private::FieldPath<
                <#field_prefix as #runtime::__private::FieldPrefix>::Append<0>
            >,
            pub body: #runtime::__private::FieldPath<
                <#field_prefix as #runtime::__private::FieldPrefix>::Append<1>
            >,
            marker: ::core::marker::PhantomData<fn() -> (#self_type, #field_prefix)>,
        }

        // SAFETY: generated enum routes preserve their root prefix and map selector/body ordinals
        // to ranges established by exact enum framing.
        #[allow(unsafe_code)]
        unsafe impl #impl_generics #runtime::__private::WireFieldSchema
            for #self_type #where_clause
        {
            type Fields<#field_prefix: #runtime::__private::FieldPrefix> =
                #fields_type<#(#schema_arguments,)* #field_prefix>;

            unsafe fn fields<#field_prefix: #runtime::__private::FieldPrefix>()
                -> Self::Fields<#field_prefix>
            {
                #fields_type {
                    selector: unsafe {
                        // SAFETY: this route is emitted from the matching enum and root prefix.
                        #runtime::__private::FieldPath::new()
                    },
                    body: unsafe {
                        // SAFETY: this route is emitted from the matching enum and root prefix.
                        #runtime::__private::FieldPath::new()
                    },
                    marker: ::core::marker::PhantomData,
                }
            }

        }

        #[doc(hidden)]
        #vis struct #view_impl #common_impl #common_where {
            input: #backing,
            represented_length: usize,
            state: #holder,
            marker: ::core::marker::PhantomData<(#marker_type, fn() -> #self_type)>,
        }

        #trait_declaration


        impl #common_impl AsRef<[u8]> for #view_impl #common_types #common_where {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.input.as_ref()[..self.represented_length]
            }
        }

        impl #retained_impl #trait_path for #retained_type #retained_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
            #retained_view_methods
        }

        // SAFETY: this retained view owns the exact input/state pair supplied to route resolution.
        #[allow(unsafe_code)]
        unsafe impl #retained_impl #runtime::__private::WireFields for #retained_type #retained_where {
            type Fields = #root_fields_type;
            type SelectionRoot = #self_type;

            fn fields(&self) -> Self::Fields {
                // SAFETY: the generated root prefix matches this view's SelectionRoot.
                unsafe {
                    <#self_type as #runtime::__private::WireFieldSchema>::fields::<
                        #runtime::__private::FieldRouteEnd<#self_type>
                    >()
                }
            }

            fn field_range(&self, index: usize) -> Option<::core::ops::Range<usize>> {
                match index {
                    0 => Some(0..#selector_width),
                    1 => Some(#selector_width..self.represented_length),
                    _ => None,
                }
            }

            unsafe fn resolve_field_route<Route>(&self) -> Option<::core::ops::Range<usize>>
            where
                Route: #runtime::__private::FieldRoute<Root = Self::SelectionRoot>,
            {
                // SAFETY: this view owns the exact input and state passed to the route.
                unsafe { Route::resolve::<#self_type>(self.as_ref(), &self.state) }
            }
        }

        impl #projected_impl #trait_path for #projected_type #projected_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
            #projected_view_methods
        }

        // SAFETY: this projected view borrows the exact input/state pair supplied to route resolution.
        #[allow(unsafe_code)]
        unsafe impl #projected_impl #runtime::__private::WireFields for #projected_type #projected_where {
            type Fields = #root_fields_type;
            type SelectionRoot = #self_type;

            fn fields(&self) -> Self::Fields {
                // SAFETY: the generated root prefix matches this view's SelectionRoot.
                unsafe {
                    <#self_type as #runtime::__private::WireFieldSchema>::fields::<
                        #runtime::__private::FieldRouteEnd<#self_type>
                    >()
                }
            }

            fn field_range(&self, index: usize) -> Option<::core::ops::Range<usize>> {
                match index {
                    0 => Some(0..#selector_width),
                    1 => Some(#selector_width..self.represented_length),
                    _ => None,
                }
            }

            unsafe fn resolve_field_route<Route>(&self) -> Option<::core::ops::Range<usize>>
            where
                Route: #runtime::__private::FieldRoute<Root = Self::SelectionRoot>,
            {
                // SAFETY: this view borrows the exact input and state passed to the route.
                unsafe { Route::resolve::<#self_type>(self.as_ref(), self.state) }
            }
        }

        impl #common_impl #runtime::ExactWire<#self_type>
            for #view_impl #common_types #common_where
        {
            #[inline(always)]
            fn as_wire_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
        }

        // SAFETY: framing owns every known child state, checks its reported extent against the
        // exact selector-bounded body, and unknown state retains only the decoded selector.
        // Reconstruction receives that state with the same immutable exact span.
        #[allow(unsafe_code)]
        unsafe impl #impl_generics #runtime::WireView for #self_type #where_clause {
            type Error = #error_type;
            type State = #state_type;
            type View<#view_lifetime> = #projected_type;

            const FIXED_SIZE: Option<usize> = {
                #selector_validation
                #fixed_size
            };
            const LEADING_EXTENT: bool = #leading_extent;

            #[inline]
            fn frame(
                input: &[u8],
                #frame_offset: usize,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                if #duplicate_selector {
                    return Err(#view_error::DuplicateSelector);
                }
                if input.len() < #selector_width {
                    return Err(#view_error::NeedMore(#runtime::NeedMore {
                        offset: #frame_offset.saturating_add(input.len()),
                        additional_at_least: #selector_width - input.len(),
                    }));
                }
                let selector = #selector_ty::#decode(
                    input[..#selector_width]
                        .try_into()
                        .expect("selector width checked"),
                );
                Ok(match selector {
                    #(#frame_arms)*
                    #unknown_frame
                })
            }

            #[inline(always)]
            unsafe fn from_validated_parts<#view_lifetime>(
                input: &#view_lifetime [u8],
                state: &#view_lifetime Self::State,
            ) -> Self::View<#view_lifetime> {
                #view_impl {
                    input,
                    represented_length: input.len(),
                    state,
                    marker: ::core::marker::PhantomData,
                }
            }


            unsafe fn selection_field_range(
                input: &[u8],
                _state: &Self::State,
                index: usize,
            ) -> Option<::core::ops::Range<usize>> {
                match index {
                    0 => Some(0..#selector_width),
                    1 => Some(#selector_width..input.len()),
                    _ => None,
                }
            }

            unsafe fn selection_nested_range<Route: #runtime::__private::FieldRoute>(
                _input: &[u8],
                _state: &Self::State,
                _index: usize,
            ) -> Option<::core::ops::Range<usize>> {
                None
            }
            fn flatten_recursive_error(
                error: Self::Error,
                fallback_offset: usize,
            ) -> #runtime::__private::RecursiveError {
                match error {
                    #view_error::NeedMore(source) => {
                        #runtime::__private::RecursiveError::NeedMore(source)
                    }
                    #view_error::Layout(source) => {
                        #runtime::__private::RecursiveError::Layout(source)
                    }
                    #view_error::InvalidFrame(source) => {
                        #runtime::__private::RecursiveError::InvalidFrame(source)
                    }
                    #view_error::Trailing(source) => {
                        #runtime::__private::RecursiveError::Trailing(source)
                    }
                    #(#wireview_flatten_arms)*
                    #view_error::UnknownSelector { offset, .. } => {
                        #runtime::__private::RecursiveError::UnknownSelector { offset }
                    }
                    #view_error::DuplicateSelector => {
                        #runtime::__private::RecursiveError::Child {
                            offset: fallback_offset,
                        }
                    }
                }
            }
        }

        impl #impl_generics #runtime::__private::WireSelect for #self_type #where_clause {
            type Root<#backing> = #retained_type
            where
                #backing: AsRef<[u8]>;

            fn select_view<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<Self::Root<#backing>, Self::Error> {
                let bytes = input.as_ref();
                let frame = <Self as #runtime::WireView>::frame(bytes, 0)?;
                let (state, consumed) = frame.into_parts();
                if consumed > bytes.len() {
                    return Err(#view_error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed,
                        available: bytes.len(),
                    }));
                }
                if consumed < bytes.len() {
                    return Err(#view_error::Trailing(#runtime::TrailingBytes {
                        offset: consumed,
                        trailing: bytes.len() - consumed,
                    }));
                }
                Ok(#view_impl {
                    input,
                    represented_length: consumed,
                    state,
                    marker: ::core::marker::PhantomData,
                })
            }

            fn validated_view<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<Self::Root<#backing>, Self::Error> {
                Self::select_view(input)
            }
            #[allow(unsafe_code)]
            unsafe fn framed_view<#backing: AsRef<[u8]>>(
                input: #backing,
                state: Self::State,
            ) -> Self::Root<#backing> {
                let represented_length = input.as_ref().len();
                #view_impl {
                    input,
                    represented_length,
                    state,
                    marker: ::core::marker::PhantomData,
                }
            }

            fn validate_view<#backing: AsRef<[u8]>>(
                _view: &Self::Root<#backing>,
            ) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        #leading_impl

        impl #impl_generics #self_type #where_clause {
            #sequence_methods

            #[inline]
            #vis fn view<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #error_type> {
                let bytes = input.as_ref();
                let frame = <Self as #runtime::WireView>::frame(bytes, 0)?;
                let (state, consumed) = frame.into_parts();
                if consumed > bytes.len() {
                    return Err(#view_error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed,
                        available: bytes.len(),
                    }));
                }
                if consumed < bytes.len() {
                    return Err(#view_error::Trailing(#runtime::TrailingBytes {
                        offset: consumed,
                        trailing: bytes.len() - consumed,
                    }));
                }
                Ok(#view_impl {
                    input,
                    represented_length: consumed,
                    state,
                    marker: ::core::marker::PhantomData,
                })
            }

            #[inline]
            #vis fn view_unchecked<#backing: AsRef<[u8]>>(
                input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #error_type> {
                Self::view(input)
            }
        }
    })
}

pub(super) fn render_builder(
    input: DeriveInput,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let schema = EnumSchema::parse(input, "WireBuilder")?;
    let recursive_slots = schema
        .known_variants()
        .map(|variant| {
            super::recursive::root_write_slot_marker(&variant.body, &schema.name, runtime)
        })
        .collect::<syn::Result<Vec<_>>>()?;
    if recursive_slots.iter().any(Option::is_some) {
        return recursive::writer::render(schema, runtime);
    }
    if schema.known_variants().any(|variant| {
        super::recursive::contains_root_generic_argument(&variant.body, &schema.name)
    }) {
        return Err(syn::Error::new_spanned(
            &schema.name,
            "recursive enum builders are not supported by the current read-side recursive capability",
        ));
    }
    let vis = &schema.vis;
    let name = &schema.name;
    let selector_ty = scalar_type_tokens(schema.selector);
    let encode = to_bytes_method(schema.endian);
    let builder = format_ident!("{}Builder", name.unraw());
    let writer = format_ident!("{}Writer", name.unraw());
    let done = format_ident!("{}WriterComplete", name.unraw());
    let output = fresh_type_ident(&schema.generics, "Output");

    let known = schema.known_variants().collect::<Vec<_>>();
    let reserved = schema.unknown().is_some().then_some("unknown");
    let method_names = variant_method_names(&known, reserved.as_slice());
    let mut bounded = schema.generics.clone();
    for variant in &known {
        let body = &variant.body;
        bounded
            .make_where_clause()
            .predicates
            .push(parse_quote!(#body: #runtime::WireBuilder));
    }
    let (impl_generics, type_generics, where_clause) = bounded.split_for_impl();
    let selector_values = known
        .iter()
        .map(|variant| variant.value.as_ref().expect("known selector"))
        .collect::<Vec<_>>();
    let selector_count = selector_values.len();
    let selector_validation = quote! {
        let _values: [#selector_ty; #selector_count] = [#(#selector_values),*];
    };
    let self_type = quote!(#name #type_generics);
    let marker = quote!(fn() -> #self_type);
    let reserved = schema.unknown().is_some().then_some("Unknown");
    let type_stems = variant_type_stems(&known, reserved.as_slice());
    let detached_types = type_stems
        .iter()
        .map(|stem| format_ident!("{}{stem}Builder", name.unraw()))
        .collect::<Vec<_>>();
    let write_errors = type_stems
        .iter()
        .map(|stem| format_ident!("{}{stem}WriteError", name.unraw()))
        .collect::<Vec<_>>();
    let child_builder = fresh_type_ident(&schema.generics, "ChildBuilder");
    let build = fresh_type_ident(&schema.generics, "Build");

    let detached_declarations = known.iter().zip(&detached_types).map(|(_, detached)| {
        quote! {
            #[doc(hidden)]
            #vis struct #detached<#child_builder, __WireReprMarker> {
                child: #child_builder,
                marker: ::core::marker::PhantomData<__WireReprMarker>,
            }
        }
    });
    let error_declarations = known.iter().zip(&write_errors).map(|(variant, error)| {
        let message = format!("enum variant `{}` failed", variant.name.unraw());
        quote! {
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #error<__WireReprError: ::core::error::Error + 'static> {
                #[error(#message)]
                Body(#[source] __WireReprError),
            }
        }
    });
    let detached_methods = known.iter().zip(&detached_types).zip(&method_names).map(
        |((variant, detached), method)| {
            let body = &variant.body;
            quote! {
                #[inline(always)]
                #vis fn #method<#build, #child_builder>(
                    self,
                    build: #build,
                ) -> #detached<#child_builder, #marker>
                where
                    #body: #runtime::WireBuilder,
                    #build: FnOnce(<#body as #runtime::WireBuilder>::Builder) -> #child_builder,
                {
                    #detached {
                        child: build(<#body as #runtime::WireBuilder>::builder()),
                        marker: ::core::marker::PhantomData,
                    }
                }
            }
        },
    );
    let write_impls = known
        .iter()
        .zip(&detached_types)
        .zip(&write_errors)
        .map(|((variant, detached), error)| {
            let body = &variant.body;
            let value = variant.value.as_ref().expect("known variant has value");
            let mut write_generics = bounded.clone();
            write_generics.params.push(parse_quote!(#child_builder));
            write_generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#body: #runtime::WireWrite<#child_builder>));
            let (write_impl, _, write_where) = write_generics.split_for_impl();
            quote! {
                impl #write_impl #runtime::WireWrite<
                    #detached<#child_builder, #marker>
                > for #self_type #write_where
                {
                    type Error = #error<<#body as #runtime::WireWrite<#child_builder>>::Error>;

                    fn write<__WireReprOutput: #runtime::Output>(
                        value: #detached<#child_builder, #marker>,
                        output: &mut #runtime::ChildWriter<'_, __WireReprOutput>,
                    ) -> Result<(), #runtime::WriteError<Self::Error, __WireReprOutput::GrowError>> {
                        let selector: #selector_ty = #value;
                        output.write(&selector.#encode())?;
                        match <#body as #runtime::WireWrite<#child_builder>>::write(value.child, output) {
                            Ok(()) => Ok(()),
                            Err(#runtime::WriteError::Schema(error)) => {
                                Err(#runtime::WriteError::Schema(#error::Body(error)))
                            }
                            Err(#runtime::WriteError::Output(error)) => {
                                Err(#runtime::WriteError::Output(error))
                            }
                        }
                    }
                }
            }
        });

    let mut writer_generics = bounded.clone();
    writer_generics.params.push(parse_quote!(#output));
    writer_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#output: #runtime::Output));
    let (writer_impl, writer_types, writer_where) = writer_generics.split_for_impl();
    let progressive_methods = known
        .iter()
        .zip(&write_errors)
        .zip(&method_names)
        .map(|((variant, error), method)| {
        let body = &variant.body;
        let value = variant.value.as_ref().expect("known variant has value");
        quote! {
            #[inline]
            #vis fn #method<#build, #child_builder>(
                mut self,
                build: #build,
            ) -> Result<
                #done #writer_types,
                #runtime::WriteError<
                    #error<<#body as #runtime::WireWrite<#child_builder>>::Error>,
                    <#output as #runtime::Output>::GrowError,
                >,
            >
            where
                #body: #runtime::WireBuilder + #runtime::WireWrite<#child_builder>,
                #build: FnOnce(<#body as #runtime::WireBuilder>::Builder) -> #child_builder,
            {
                let selector: #selector_ty = #value;
                self.writer.write(&selector.#encode())?;
                let body_start = self.writer.position();
                let child_value = build(<#body as #runtime::WireBuilder>::builder());
                let mut child = self.writer.child_at(body_start)?;
                match <#body as #runtime::WireWrite<#child_builder>>::write(child_value, &mut child) {
                    Ok(()) => {}
                    Err(#runtime::WriteError::Schema(error)) => {
                        return Err(#runtime::WriteError::Schema(#error::Body(error)));
                    }
                    Err(#runtime::WriteError::Output(error)) => {
                        return Err(#runtime::WriteError::Output(error));
                    }
                }
                child.finish()?;
                Ok(#done {
                    writer: self.writer,
                    marker: ::core::marker::PhantomData,
                })
            }
        }
    });

    let unknown_error = format_ident!("{}UnknownWriteError", name.unraw());
    let unknown_builder = format_ident!("{}UnknownBuilder", name.unraw());
    let raw_bytes = fresh_type_ident(&schema.generics, "RawBytes");
    let known_values = known
        .iter()
        .map(|variant| variant.value.as_ref().expect("known value"))
        .collect::<Vec<_>>();
    let unknown_declaration = schema.unknown().map(|_| {
        quote! {
            #[derive(Debug, #runtime::__private::ThisError)]
            #vis enum #unknown_error {
                #[error("known selector passed to the unknown variant")]
                KnownSelector,
            }
        }
    });
    let unknown_builder_declaration = schema.unknown().map(|_| {
        quote! {
            #[doc(hidden)]
            #vis struct #unknown_builder<__WireReprBytes, __WireReprMarker> {
                selector: #selector_ty,
                body: __WireReprBytes,
                marker: ::core::marker::PhantomData<__WireReprMarker>,
            }
        }
    });
    let unknown_detached_method = schema.unknown().map(|_| {
        quote! {
            #[inline(always)]
            #vis fn unknown<#raw_bytes: AsRef<[u8]>>(
                self,
                selector: #selector_ty,
                body: #raw_bytes,
            ) -> #unknown_builder<#raw_bytes, #marker> {
                #unknown_builder {
                    selector,
                    body,
                    marker: ::core::marker::PhantomData,
                }
            }
        }
    });
    let unknown_write_impl = schema.unknown().map(|_| {
        let mut write_generics = bounded.clone();
        write_generics.params.push(parse_quote!(#raw_bytes));
        write_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#raw_bytes: AsRef<[u8]>));
        let (write_impl, _, write_where) = write_generics.split_for_impl();
        quote! {
            impl #write_impl #runtime::WireWrite<
                #unknown_builder<#raw_bytes, #marker>
            > for #self_type #write_where
            {
                type Error = #unknown_error;

                fn write<__WireReprOutput: #runtime::Output>(
                    value: #unknown_builder<#raw_bytes, #marker>,
                    output: &mut #runtime::ChildWriter<'_, __WireReprOutput>,
                ) -> Result<(), #runtime::WriteError<Self::Error, __WireReprOutput::GrowError>> {
                    if false #(|| value.selector == { let known: #selector_ty = #known_values; known })* {
                        return Err(#runtime::WriteError::Schema(#unknown_error::KnownSelector));
                    }
                    output.write(&value.selector.#encode())?;
                    output.write(value.body.as_ref())?;
                    Ok(())
                }
            }
        }
    });
    let unknown_method = schema.unknown().map(|_| {
        quote! {
            #[inline]
            #vis fn unknown<__WireReprBytes: AsRef<[u8]>>(
                mut self,
                selector: #selector_ty,
                body: __WireReprBytes,
            ) -> Result<
                #done #writer_types,
                #runtime::WriteError<#unknown_error, <#output as #runtime::Output>::GrowError>,
            > {
                if false #(|| selector == { let known: #selector_ty = #known_values; known })* {
                    return Err(#runtime::WriteError::Schema(#unknown_error::KnownSelector));
                }
                self.writer.write(&selector.#encode())?;
                let body = body.as_ref();
                self.writer.write(body)?;
                Ok(#done {
                    writer: self.writer,
                    marker: ::core::marker::PhantomData,
                })
            }
        }
    });
    let fixed_size = if schema.unknown().is_some() {
        quote!(None)
    } else {
        let body_sizes = known.iter().map(|variant| {
            let body = &variant.body;
            quote!(<#body as #runtime::WireBuilder>::FIXED_SIZE)
        });
        quote!(
            #runtime::__private::checked_optional_sum([
                Some(::core::mem::size_of::<#selector_ty>()),
                #runtime::__private::checked_optional_equal([#(#body_sizes),*]),
            ])
        )
    };

    Ok(quote! {
        #[doc(hidden)]
        #vis struct #builder<__WireReprMarker> {
            marker: ::core::marker::PhantomData<__WireReprMarker>,
        }
        #(#detached_declarations)*
        #unknown_builder_declaration
        #(#error_declarations)*
        #unknown_declaration

        impl #impl_generics #builder<#marker> #where_clause {
            #(#detached_methods)*
            #unknown_detached_method
        }

        #(#write_impls)*

        #unknown_write_impl
        #vis struct #writer #writer_impl #writer_where {
            writer: #runtime::Writer<#output>,
            marker: ::core::marker::PhantomData<#marker>,
        }

        #vis struct #done #writer_impl #writer_where {
            writer: #runtime::Writer<#output>,
            marker: ::core::marker::PhantomData<#marker>,
        }

        impl #writer_impl #writer #writer_types #writer_where {
            #(#progressive_methods)*
            #unknown_method
        }

        impl #writer_impl #done #writer_types #writer_where {
            #[inline(always)]
            #vis fn finish(self) -> Result<#runtime::Written<#output>, #runtime::OutputError<<#output as #runtime::Output>::GrowError>> {
                Ok(self.writer.finish())
            }
        }

        impl #impl_generics #runtime::WireBuilder for #self_type #where_clause {
            const FIXED_SIZE: Option<usize> = {
                #selector_validation
                #fixed_size
            };
            type Builder = #builder<#marker>;

            #[inline(always)]
            fn builder() -> Self::Builder {
                let _ = <Self as #runtime::WireBuilder>::FIXED_SIZE;
                #builder { marker: ::core::marker::PhantomData }
            }

        }

        impl #impl_generics #self_type #where_clause {
            #[inline(always)]
            #vis fn builder<#output: #runtime::Output>(output: #output) -> #writer #writer_types {
                let _ = <Self as #runtime::WireBuilder>::FIXED_SIZE;
                #writer {
                    writer: #runtime::Writer::new(output),
                    marker: ::core::marker::PhantomData,
                }
            }
        }
    })
}

struct EnumSchema {
    vis: Visibility,
    name: Ident,
    generics: Generics,
    selector: ScalarType,
    endian: Endian,
    variants: Vec<Variant>,
}

struct Variant {
    name: Ident,
    body: Type,
    value: Option<Expr>,
    unknown: bool,
}

impl EnumSchema {
    fn parse(input: DeriveInput, owner: &str) -> syn::Result<Self> {
        let (selector_type, declared_endian) = parse_item_attributes(&input.attrs, owner)?;
        let selector = scalar_from_type(&selector_type)?;
        if !selector.is_unsigned_integer() {
            return Err(syn::Error::new_spanned(
                selector_type,
                "enum selector must be an unsigned fixed-width integer",
            ));
        }
        let endian = super::model::scalar_endian(selector, declared_endian, &selector_type)?;
        let Data::Enum(data) = input.data else {
            return Err(syn::Error::new_spanned(
                input.ident,
                format!("{owner} enum renderer requires an enum"),
            ));
        };
        let mut variants = Vec::with_capacity(data.variants.len());
        let mut values = BTreeSet::new();
        let mut unknown_seen = false;
        for variant in data.variants {
            let (value, unknown) = parse_variant_attributes(&variant.attrs)?;
            if unknown {
                if unknown_seen {
                    return Err(syn::Error::new_spanned(
                        &variant.ident,
                        "only one #[wire(unknown)] variant is allowed",
                    ));
                }
                unknown_seen = true;
            }
            let Fields::Unnamed(fields) = variant.fields else {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    "static enum variants require exactly one unnamed body field",
                ));
            };
            if fields.unnamed.len() != 1 {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    "static enum variants require exactly one unnamed body field",
                ));
            }
            let body = fields.unnamed.into_iter().next().expect("one field").ty;
            if unknown {
                if value.is_some() {
                    return Err(syn::Error::new_spanned(
                        &variant.ident,
                        "unknown variant cannot declare a selector value",
                    ));
                }
                if !is_wire_bytes(&body) {
                    return Err(syn::Error::new_spanned(
                        &body,
                        "unknown variant body must be wire::Bytes",
                    ));
                }
            } else {
                let Some(value) = value else {
                    return Err(syn::Error::new_spanned(
                        &variant.ident,
                        "known enum variant requires #[wire(value = ...)]",
                    ));
                };
                let key = value.to_token_stream().to_string();
                if !values.insert(key) {
                    return Err(syn::Error::new_spanned(
                        &value,
                        "duplicate enum selector value",
                    ));
                }
                variants.push(Variant {
                    name: variant.ident,
                    body,
                    value: Some(value),
                    unknown,
                });
                continue;
            }
            variants.push(Variant {
                name: variant.ident,
                body,
                value,
                unknown,
            });
        }
        if !variants.iter().any(|variant| !variant.unknown) {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "wire enum requires at least one known variant",
            ));
        }
        if unknown_seen
            && let Some(variant) = variants
                .iter()
                .find(|variant| !variant.unknown && variant.name == "Unknown")
        {
            return Err(syn::Error::new_spanned(
                &variant.name,
                "`Unknown` is reserved when an unknown fallback variant is present",
            ));
        }
        Ok(Self {
            vis: input.vis,
            name: input.ident,
            generics: input.generics,
            selector,
            endian,
            variants,
        })
    }

    fn known_variants(&self) -> impl Iterator<Item = &Variant> {
        self.variants.iter().filter(|variant| !variant.unknown)
    }

    fn unknown(&self) -> Option<&Variant> {
        self.variants.iter().find(|variant| variant.unknown)
    }
}

fn parse_item_attributes(
    attributes: &[syn::Attribute],
    owner: &str,
) -> syn::Result<(Type, Option<Endian>)> {
    let mut selector = None;
    let mut endian = None;
    for attribute in attributes {
        if !attribute.path().is_ident("wire") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("selector") {
                if selector.is_some() {
                    return Err(meta.error("duplicate selector representation"));
                }
                selector = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("le") || meta.path.is_ident("be") {
                if endian.is_some() {
                    return Err(meta.error("duplicate or conflicting endian attribute"));
                }
                endian = Some(if meta.path.is_ident("le") {
                    Endian::Little
                } else {
                    Endian::Big
                });
                return Ok(());
            }
            Err(meta.error(format!("unsupported {owner} enum attribute")))
        })?;
    }
    let selector = selector.ok_or_else(|| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            "static enum requires #[wire(selector = unsigned_type)]",
        )
    })?;
    Ok((selector, endian))
}

fn parse_variant_attributes(attributes: &[syn::Attribute]) -> syn::Result<(Option<Expr>, bool)> {
    let mut value = None;
    let mut unknown = false;
    for attribute in attributes {
        if !attribute.path().is_ident("wire") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("value") {
                if value.is_some() {
                    return Err(meta.error("duplicate enum selector value"));
                }
                value = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("unknown") {
                if unknown {
                    return Err(meta.error("duplicate unknown marker"));
                }
                unknown = true;
                return Ok(());
            }
            Err(meta.error("unsupported enum variant attribute"))
        })?;
    }
    Ok((value, unknown))
}

fn scalar_from_type(ty: &Type) -> syn::Result<ScalarType> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "selector representation must be a primitive integer",
        ));
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            ty,
            "selector representation must be a primitive integer",
        ));
    }
    let name = path.path.segments[0].ident.unraw().to_string();
    ScalarType::from_name(&name)
        .ok_or_else(|| syn::Error::new_spanned(ty, "unsupported enum selector representation"))
}

fn is_wire_bytes(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let mut segments = path.path.segments.iter().rev();
    matches!(
        (segments.next(), segments.next()),
        (Some(bytes), Some(wire)) if bytes.ident == "Bytes" && wire.ident == "wire"
    )
}

fn generic_arguments(generics: &Generics) -> Vec<TokenStream> {
    generics
        .params
        .iter()
        .map(|parameter| match parameter {
            GenericParam::Lifetime(parameter) => {
                let lifetime = &parameter.lifetime;
                quote!(#lifetime)
            }
            GenericParam::Type(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
            GenericParam::Const(parameter) => {
                let ident = &parameter.ident;
                quote!(#ident)
            }
        })
        .collect()
}
fn variant_type_stems(variants: &[&Variant], reserved: &[&str]) -> Vec<String> {
    let mut used = reserved
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    variants
        .iter()
        .map(|variant| {
            let base = super::pascal(&variant.name).to_string();
            if used.insert(base.clone()) {
                return base;
            }
            for suffix in 2usize.. {
                let candidate = format!("{base}{suffix}");
                if used.insert(candidate.clone()) {
                    return candidate;
                }
            }
            unreachable!("usize suffix space cannot exhaust generated type names")
        })
        .collect()
}

fn variant_method_names(variants: &[&Variant], reserved: &[&str]) -> Vec<Ident> {
    let mut used = reserved
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    variants
        .iter()
        .map(|variant| {
            let base = super::snake(&variant.name);
            if used.insert(base.clone()) {
                return format_ident!("{base}");
            }
            for suffix in 2usize.. {
                let candidate = format!("{base}_{suffix}");
                if used.insert(candidate.clone()) {
                    return format_ident!("{candidate}");
                }
            }
            unreachable!("usize suffix space cannot exhaust generated method names")
        })
        .collect()
}
