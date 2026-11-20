mod error;
pub(super) mod fields;
mod frame;
mod geometry;
mod methods;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{GenericParam, Generics, LifetimeParam, TypeParam, parse_quote};

use fields::render_wire_fields;

use super::model::{FieldKind, Schema, ValueType};
use super::{
    fresh_field_ident, fresh_schema_lifetime, private_ident, scalar_type_tokens, view_optional_size,
};

pub(super) fn render(schema: &Schema, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let bounded = bounded_generics(schema, runtime);
    let names = names(schema, &bounded);
    let error_signature = error::signature(schema, &names.error, runtime);
    let error_names = error::variant_names(schema);
    let error_declaration = error::render(
        schema,
        &names.error,
        &error_names,
        &error_signature.parameters,
        runtime,
    );
    let descriptor = render_descriptor(schema, &names, &retained_state(schema), runtime);
    let targets = impl_targets(schema, &names, bounded, &descriptor.ty, runtime);

    let descriptor_declaration = &descriptor.declaration;
    let field_schema = render_field_schema(schema, &names, &targets, runtime);
    let view_impls = render_view_impls(schema, &names, &targets, runtime);
    let wire_view_impl = render_wire_view_impl(
        schema,
        &names,
        &targets,
        &descriptor,
        &error_signature.ty,
        &error_names,
        runtime,
    );
    let select_impl = render_select_impl(schema, &names, &targets, &error_names, runtime);
    let leading_impl = render_leading_impl(schema, &targets, runtime);
    let inherent_impl =
        render_inherent_impl(schema, &names, &targets, &error_signature.ty, runtime);
    Ok(quote! {
        #error_declaration
        #descriptor_declaration
        #field_schema
        #view_impls
        #wire_view_impl
        #select_impl

        #leading_impl

        #inherent_impl
    })
}

/// Generated type names and hygienic bindings shared by the emitted items.
struct Names {
    view_trait: syn::Ident,
    descriptor: syn::Ident,
    view_impl: syn::Ident,
    state_holder: syn::Ident,
    error: syn::Ident,
    fields_type: syn::Ident,
    backing: syn::Ident,
    view_lifetime: syn::Lifetime,
    input_field: syn::Ident,
    represented_length_field: syn::Ident,
    view_marker: syn::Ident,
    frame_input: syn::Ident,
    frame_offset: syn::Ident,
    current_input: syn::Ident,
    view_input: syn::Ident,
    input_length: syn::Ident,
    frame_result: syn::Ident,
    framed_descriptor: syn::Ident,
    framed_consumed: syn::Ident,
    retained_value: syn::Ident,
}

fn names(schema: &Schema, bounded: &Generics) -> Names {
    let name = &schema.name;
    Names {
        view_trait: format_ident!("{}View", name.unraw()),
        descriptor: format_ident!("__WireRepr{}ViewDescriptor", name.unraw()),
        view_impl: format_ident!("__WireRepr{}ViewImpl", name.unraw()),
        state_holder: super::fresh_type_ident(&schema.generics, "StateHolder"),
        error: format_ident!("{}ViewError", name.unraw()),
        fields_type: format_ident!("{}Fields", name.unraw()),
        backing: super::fresh_type_ident(bounded, "Backing"),
        view_lifetime: fresh_schema_lifetime(schema, bounded, "view"),
        input_field: fresh_field_ident(schema, "input"),
        represented_length_field: private_ident(schema, "represented_length"),
        view_marker: private_ident(schema, "view_marker"),
        frame_input: private_ident(schema, "frame_input"),
        frame_offset: private_ident(schema, "frame_offset"),
        current_input: private_ident(schema, "current_input"),
        view_input: private_ident(schema, "view_input"),
        input_length: private_ident(schema, "input_length"),
        frame_result: private_ident(schema, "frame_result"),
        framed_descriptor: private_ident(schema, "framed_descriptor"),
        framed_consumed: private_ident(schema, "framed_consumed"),
        retained_value: private_ident(schema, "retained_value"),
    }
}

/// Values framing computes once and the descriptor keeps for the getters.
struct Retained {
    /// Field name, framing binding, and value type of each converted scalar.
    char_fields: Vec<(syn::Ident, syn::Ident, TokenStream)>,
    /// Field name and framing binding of each presence flag.
    flags: Vec<(syn::Ident, syn::Ident)>,
    starts: Vec<syn::Ident>,
    ends: Vec<syn::Ident>,
    array_base: Option<syn::Ident>,
    /// Descriptor field and controller binding of each counted array.
    array_counts: Vec<(syn::Ident, syn::Ident)>,
}

fn retained_state(schema: &Schema) -> Retained {
    let char_fields = schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Scalar(scalar) = &field.kind else {
                return None;
            };
            if scalar.constant.is_some() {
                return None;
            }
            let ty = match &scalar.value_type {
                ValueType::Char => quote!(char),
                ValueType::Custom(ty) => quote!(#ty),
                _ => return None,
            };
            Some((
                field.name.clone(),
                private_ident(schema, &format!("scalar_{index}_value")),
                ty,
            ))
        })
        .collect::<Vec<_>>();
    let flags = schema
        .flag_fields()
        .map(|field| {
            (
                field.name.clone(),
                private_ident(schema, &format!("flag_{}_value", field.name.unraw())),
            )
        })
        .collect::<Vec<_>>();
    let starts = schema
        .fields
        .iter()
        .enumerate()
        .filter(|(index, _)| geometry::retains_start(schema, *index))
        .map(|(index, _)| private_ident(schema, &format!("field_{index}_start")))
        .collect::<Vec<_>>();
    let ends = schema
        .fields
        .iter()
        .enumerate()
        .filter(|(index, _)| geometry::retains_end(schema, *index))
        .map(|(index, _)| private_ident(schema, &format!("field_{index}_end")))
        .collect::<Vec<_>>();
    let array_base = schema
        .array_fields()
        .next()
        .map(|_| private_ident(schema, "array_base_offset"));
    let array_counts = schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Array(array) = &field.kind else {
                return None;
            };
            Some((
                private_ident(schema, &format!("array_{index}_count")),
                private_ident(schema, &format!("controller_{}", array.controller.unraw())),
            ))
        })
        .collect::<Vec<_>>();
    Retained {
        char_fields,
        flags,
        starts,
        ends,
        array_base,
        array_counts,
    }
}

/// The state type `frame` returns: its declaration, its instantiated type, and its constructor.
struct Descriptor {
    declaration: TokenStream,
    ty: TokenStream,
    value: TokenStream,
}

fn render_descriptor(
    schema: &Schema,
    names: &Names,
    retained: &Retained,
    runtime: &TokenStream,
) -> Descriptor {
    let vis = &schema.vis;
    let Names {
        descriptor,
        frame_offset,
        ..
    } = names;
    let Retained {
        char_fields,
        flags,
        starts,
        ends,
        array_base,
        array_counts,
    } = retained;
    let nested_fields = schema.nested_fields().collect::<Vec<_>>();
    let descriptor_parameters = (0..nested_fields.len())
        .map(|index| format_ident!("__WireReprState{index}"))
        .collect::<Vec<_>>();
    let converted_parameters = (0..char_fields.len())
        .map(|index| format_ident!("__WireReprValue{index}"))
        .collect::<Vec<_>>();
    let descriptor_array_count_fields =
        array_counts.iter().map(|(field, _)| quote!(#field: usize,));
    let descriptor_char_fields = char_fields
        .iter()
        .zip(&converted_parameters)
        .map(|((field, _, _), ty)| quote!(#field: #ty,));
    let descriptor_flag_fields = flags.iter().map(|(field, _)| quote!(#field: bool,));
    let descriptor_geometry_fields = starts
        .iter()
        .chain(ends)
        .map(|field| quote!(#field: usize,));
    let descriptor_array_base_field = array_base.iter().map(|field| quote!(#field: usize,));
    let descriptor_nested_fields =
        nested_fields
            .iter()
            .zip(&descriptor_parameters)
            .map(|(nested, state)| {
                let field = &nested.name;
                if nested.layout.condition.is_some() {
                    quote!(#field: Option<#state>,)
                } else {
                    quote!(#field: #state,)
                }
            });
    let descriptor_declaration =
        if descriptor_parameters.is_empty() && converted_parameters.is_empty() {
            quote! {
                #[doc(hidden)]
                #vis struct #descriptor {
                    #(#descriptor_flag_fields)*
                    #(#descriptor_geometry_fields)*
                    #(#descriptor_array_base_field)*
                    #(#descriptor_array_count_fields)*
                }
            }
        } else {
            quote! {
                #[doc(hidden)]
                #vis struct #descriptor<
                    #(#descriptor_parameters,)*
                    #(#converted_parameters),*
                > {
                    #(#descriptor_char_fields)*
                    #(#descriptor_flag_fields)*
                    #(#descriptor_nested_fields)*
                    #(#descriptor_geometry_fields)*
                    #(#descriptor_array_base_field)*
                    #(#descriptor_array_count_fields)*
                }
            }
        };
    let descriptor_states = nested_fields.iter().map(|field| {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        quote!(<#ty as #runtime::WireView>::State)
    });
    let descriptor_value_types = char_fields.iter().map(|(_, _, ty)| ty);
    let descriptor_type = if descriptor_parameters.is_empty() && converted_parameters.is_empty() {
        quote!(#descriptor)
    } else {
        quote!(#descriptor<#(#descriptor_states,)* #(#descriptor_value_types),*>)
    };

    let descriptor_char_values = char_fields
        .iter()
        .map(|(field, value, _)| quote!(#field: #value,));
    let descriptor_flag_values = flags.iter().map(|(field, value)| quote!(#field: #value,));
    let descriptor_nested_values = nested_fields.iter().map(|field| {
        let name = &field.name;
        let state = private_ident(schema, &format!("{}_state", name.unraw()));
        quote!(#name: #state,)
    });
    let descriptor_geometry_values = starts.iter().chain(ends).map(|field| quote!(#field,));
    let descriptor_array_base_value = array_base
        .iter()
        .map(|field| quote!(#field: #frame_offset,));
    let descriptor_array_count_values = array_counts
        .iter()
        .map(|(field, value)| quote!(#field: #value,));
    let descriptor_value = quote! {
        #descriptor {
            #(#descriptor_char_values)*
            #(#descriptor_flag_values)*
            #(#descriptor_nested_values)*
            #(#descriptor_geometry_values)*
            #(#descriptor_array_base_value)*
            #(#descriptor_array_count_values)*
        }
    };
    Descriptor {
        declaration: descriptor_declaration,
        ty: descriptor_type,
        value: descriptor_value,
    }
}

/// Generics and implementing types of each generated impl family.
struct ImplTargets {
    bounded: Generics,
    retained_generics: Generics,
    projected_generics: Generics,
    common_generics: Generics,
    schema_arguments: Vec<TokenStream>,
    self_type: TokenStream,
    retained_type: TokenStream,
    projected_type: TokenStream,
    trait_path: TokenStream,
    root_fields_type: TokenStream,
}

fn impl_targets(
    schema: &Schema,
    names: &Names,
    bounded: Generics,
    descriptor_type: &TokenStream,
    runtime: &TokenStream,
) -> ImplTargets {
    let name = &schema.name;
    let Names {
        view_trait,
        view_impl,
        state_holder,
        fields_type,
        backing,
        view_lifetime,
        ..
    } = names;
    let (_, type_generics, _) = bounded.split_for_impl();
    let self_type = quote!(#name #type_generics);
    let schema_arguments = bounded
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
        .collect::<Vec<_>>();

    let mut retained_generics = bounded.clone();
    retained_generics
        .params
        .push(GenericParam::Type(TypeParam::from(backing.clone())));
    retained_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#backing: AsRef<[u8]>));
    let retained_type = quote!(
        #view_impl<#(#schema_arguments,)* #backing, #descriptor_type>
    );

    let mut projected_generics = bounded.clone();
    projected_generics.params.insert(
        0,
        GenericParam::Lifetime(LifetimeParam::new(view_lifetime.clone())),
    );
    let projected_type = quote!(
        #view_impl<
            #(#schema_arguments,)*
            &#view_lifetime [u8],
            &#view_lifetime #descriptor_type
        >
    );

    let mut common_generics = retained_generics.clone();
    common_generics
        .params
        .push(GenericParam::Type(TypeParam::from(state_holder.clone())));
    let trait_path = quote!(#view_trait #type_generics);
    let root_fields_type = quote!(
        #fields_type<
            #(#schema_arguments,)*
            #runtime::__private::FieldRouteEnd<#self_type>
        >
    );
    ImplTargets {
        bounded,
        retained_generics,
        projected_generics,
        common_generics,
        schema_arguments,
        self_type,
        retained_type,
        projected_type,
        trait_path,
        root_fields_type,
    }
}

/// Renders the typed field-path struct and its `WireFieldSchema` impl.
fn render_field_schema(
    schema: &Schema,
    names: &Names,
    targets: &ImplTargets,
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let fields_type = &names.fields_type;
    let ImplTargets {
        bounded,
        schema_arguments,
        self_type,
        ..
    } = targets;
    let (impl_generics, _, where_clause) = bounded.split_for_impl();
    let field_prefix = super::fresh_type_ident(bounded, "FieldPrefix");
    let fields_marker = private_ident(schema, "fields_marker");
    let mut fields_generics = bounded.clone();
    fields_generics
        .params
        .push(GenericParam::Type(TypeParam::from(field_prefix.clone())));
    fields_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#field_prefix: #runtime::__private::FieldPrefix));
    let (fields_impl, _, fields_where) = fields_generics.split_for_impl();
    let field_members = schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.name;
            let route = quote!(
                <#field_prefix as #runtime::__private::FieldPrefix>::Append<#index>
            );
            match &field.kind {
                FieldKind::Nested(nested) => {
                    let ty = &nested.ty;
                    quote!(pub #name: #runtime::__private::NestedField<#route, #ty>,)
                }
                _ => quote!(pub #name: #runtime::__private::FieldPath<#route>,),
            }
        })
        .collect::<Vec<_>>();
    let field_values = schema
        .fields
        .iter()
        .map(|field| {
            let name = &field.name;
            if matches!(&field.kind, FieldKind::Nested(_)) {
                quote!(#name: unsafe {
                    // SAFETY: this route is emitted from the matching schema and prefix family.
                    #runtime::__private::NestedField::new()
                },)
            } else {
                quote!(#name: unsafe {
                    // SAFETY: this route is emitted from the matching schema and prefix family.
                    #runtime::__private::FieldPath::new()
                },)
            }
        })
        .collect::<Vec<_>>();
    quote! {
        #[doc = "Typed physical field paths generated for this schema."]
        #vis struct #fields_type #fields_impl #fields_where {
            #(#field_members)*
            #fields_marker: ::core::marker::PhantomData<fn() -> (#self_type, #field_prefix)>,
        }


        // SAFETY: generated field routes preserve their supplied root prefix and the hooks below
        // resolve only checked descriptor ranges for the matching framed schema.
        unsafe impl #impl_generics #runtime::__private::WireFieldSchema
            for #self_type #where_clause
        {
            type Fields<#field_prefix: #runtime::__private::FieldPrefix> =
                #fields_type<#(#schema_arguments,)* #field_prefix>;

            unsafe fn fields<#field_prefix: #runtime::__private::FieldPrefix>()
                -> Self::Fields<#field_prefix>
            {
                #fields_type {
                    #(#field_values)*
                    #fields_marker: ::core::marker::PhantomData,
                }
            }

        }
    }
}

/// Renders the view struct, the view trait, and the trait's retained and projected impls.
fn render_view_impls(
    schema: &Schema,
    names: &Names,
    targets: &ImplTargets,
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let Names {
        view_trait,
        view_impl,
        state_holder,
        backing,
        input_field,
        represented_length_field,
        view_marker,
        ..
    } = names;
    let ImplTargets {
        self_type,
        retained_type,
        projected_type,
        trait_path,
        root_fields_type,
        ..
    } = targets;
    let (impl_generics, _, where_clause) = targets.bounded.split_for_impl();
    let (retained_impl, _, retained_where) = targets.retained_generics.split_for_impl();
    let (projected_impl, _, projected_where) = targets.projected_generics.split_for_impl();
    let (common_impl, common_types, common_where) = targets.common_generics.split_for_impl();

    let trait_methods = methods::render_declarations(schema, runtime);
    let retained_methods = methods::render_bodies(schema, runtime);
    let projected_methods = retained_methods.clone();
    let range_arms = schema
        .fields
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let start = geometry::field_start(schema, index, runtime);
            let end = geometry::field_end(schema, index, runtime);
            quote!(#index => Some(#start..#end),)
        })
        .collect::<Vec<_>>();
    let range_body = quote!(match index {
        #(#range_arms)*
        _ => None,
    });
    let retained_fields = render_wire_fields(
        quote!(unsafe impl #retained_impl #runtime::__private::WireFields
            for #retained_type #retained_where),
        runtime,
        self_type,
        root_fields_type,
        &range_body,
        quote!(&self.descriptor),
        true,
    );
    let projected_fields = render_wire_fields(
        quote!(unsafe impl #projected_impl #runtime::__private::WireFields
            for #projected_type #projected_where),
        runtime,
        self_type,
        root_fields_type,
        &range_body,
        quote!(self.descriptor),
        true,
    );
    quote! {
        #[doc(hidden)]
        #vis struct #view_impl #common_impl #common_where {
            #input_field: #backing,
            #represented_length_field: usize,
            descriptor: #state_holder,
            #view_marker: ::core::marker::PhantomData<fn() -> #self_type>,
        }

        #[doc = "Exact-source view API generated for this schema."]
        #vis trait #view_trait #impl_generics:
            #runtime::__private::WireFields<
                Fields = #root_fields_type,
                SelectionRoot = #self_type,
            >
            #where_clause
        {
            /// Returns the exact represented bytes.
            fn as_bytes(&self) -> &[u8];

            #(#trait_methods)*
        }

        impl #common_impl AsRef<[u8]> for #view_impl #common_types #common_where {
            #[inline(always)]
            fn as_ref(&self) -> &[u8] {
                &self.#input_field.as_ref()[..self.#represented_length_field]
            }
        }

        impl #retained_impl #trait_path for #retained_type #retained_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }

            #(#retained_methods)*
        }
        #retained_fields


        impl #projected_impl #trait_path for #projected_type #projected_where {
            #[inline(always)]
            fn as_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }

            #(#projected_methods)*
        }
        #projected_fields



        impl #common_impl #runtime::ExactWire<#self_type>
            for #view_impl #common_types #common_where
        {
            #[inline(always)]
            fn as_wire_bytes(&self) -> &[u8] {
                <Self as AsRef<[u8]>>::as_ref(self)
            }
        }
    }
}

fn render_wire_view_impl(
    schema: &Schema,
    names: &Names,
    targets: &ImplTargets,
    descriptor: &Descriptor,
    error_type: &TokenStream,
    error_names: &error::VariantNames,
    runtime: &TokenStream,
) -> TokenStream {
    let Names {
        error,
        view_impl,
        view_lifetime,
        input_field,
        represented_length_field,
        view_marker,
        frame_input,
        frame_offset,
        ..
    } = names;
    let ImplTargets {
        self_type,
        projected_type,
        ..
    } = targets;
    let (impl_generics, _, where_clause) = targets.bounded.split_for_impl();
    let descriptor_type = &descriptor.ty;
    let descriptor_value = &descriptor.value;

    let fixed_size = if schema.has_explicit_geometry() {
        quote!(None)
    } else {
        view_optional_size(schema, runtime)
    };
    let frame_steps = frame::render_steps(
        schema,
        error,
        error_names,
        frame_input,
        frame_offset,
        runtime,
        false,
    );
    let leading_extent = render_leading_extent(schema, runtime);
    let consumed = render_consumed(schema, runtime);
    let exact_frame = render_exact_frame(
        schema,
        names,
        error_names,
        descriptor_value,
        &consumed,
        runtime,
    );
    let nested_selection_arms = render_nested_selection_arms(schema);
    let flatten_error = error::render_flatten(schema, error, error_names, runtime);
    quote! {
        // SAFETY: generated framing bounds-checks every retained scalar value and child extent;
        // the descriptor owns no input references and reconstruction slices the same exact span.
        unsafe impl #impl_generics #runtime::WireView for #self_type #where_clause {
            type Error = #error_type;
            type State = #descriptor_type;
            type View<#view_lifetime> = #projected_type;

            const FIXED_SIZE: Option<usize> = #fixed_size;
            const LEADING_EXTENT: bool = #leading_extent;

            #[inline]
            fn frame(
                #frame_input: &[u8],
                #frame_offset: usize,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                #(#frame_steps)*
                Ok(#runtime::Frame::new(#descriptor_value, #consumed))
            }
            #exact_frame

            #[inline(always)]
            unsafe fn from_validated_parts<#view_lifetime>(
                input: &#view_lifetime [u8],
                state: &#view_lifetime Self::State,
            ) -> Self::View<#view_lifetime> {
                // SAFETY: the WireView caller guarantees `input` has the exact framed length and
                // `state` came from that frame; every retained child state is paired with its
                // checked generated range below.
                #view_impl {
                    #input_field: input,
                    #represented_length_field: input.len(),
                    descriptor: state,
                    #view_marker: ::core::marker::PhantomData,
                }
            }

            #[inline(always)]
            unsafe fn selection_field_range(
                input: &[u8],
                state: &Self::State,
                index: usize,
            ) -> Option<::core::ops::Range<usize>> {
                // SAFETY: route resolution receives the exact span and state retained by framing.
                let view = unsafe {
                    <Self as #runtime::WireView>::from_validated_parts(input, state)
                };
                #runtime::__private::WireFields::field_range(&view, index)
            }

            #[inline(always)]
            unsafe fn selection_nested_range<Route: #runtime::__private::FieldRoute>(
                input: &[u8],
                state: &Self::State,
                index: usize,
            ) -> Option<::core::ops::Range<usize>> {
                match index {
                    #(#nested_selection_arms)*
                    _ => None,
                }
            }
            #flatten_error
        }
    }
}

/// Renders whether a following schema can start where this one ends.
fn render_leading_extent(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    let terminal_leading_extent = match schema.fields.last().map(|field| &field.kind) {
        Some(FieldKind::RawBytes(super::model::RawBytes {
            extent: super::model::DynamicExtent::Rest,
        })) => quote!(false),
        Some(FieldKind::Array(array)) => {
            let item = &array.item;
            quote!(<#item as #runtime::WireView>::LEADING_EXTENT)
        }
        Some(FieldKind::Nested(super::model::NestedField {
            terminal: true,
            extent: None,
            ty,
        })) => quote!(<#ty as #runtime::WireView>::LEADING_EXTENT),
        _ => quote!(true),
    };
    let leading_children = schema.fields.iter().filter_map(|field| {
        let FieldKind::Nested(nested) = &field.kind else {
            return None;
        };
        if nested.terminal || nested.extent.is_some() {
            return None;
        }
        let ty = &nested.ty;
        Some(quote!(
            (<#ty as #runtime::WireView>::FIXED_SIZE.is_some()
                || <#ty as #runtime::WireView>::LEADING_EXTENT)
        ))
    });
    quote!(#terminal_leading_extent #(&& #leading_children)*)
}

/// Renders the byte count `frame` reports once every step has run.
fn render_consumed(schema: &Schema, runtime: &TokenStream) -> TokenStream {
    if schema.has_sequential_geometry() {
        let cursor = private_ident(schema, "geometry_cursor");
        quote!(#cursor)
    } else if let Some(last) = schema.fields.last() {
        match &last.kind {
            FieldKind::Nested(_) => {
                let total = private_ident(schema, &format!("{}_total", last.name.unraw()));
                quote!(#total)
            }
            FieldKind::Scalar(_)
            | FieldKind::Bytes(_)
            | FieldKind::ScalarArray(_)
            | FieldKind::BitProjection(_) => {
                let total = geometry::view_offset_for_end(schema, last, runtime);
                quote!(#total.expect("framed fixed schema width"))
            }
            FieldKind::RawBytes(_)
            | FieldKind::Array(_)
            | FieldKind::Recursive(_)
            | FieldKind::Flag(_) => {
                unreachable!("dynamic fields use explicit geometry")
            }
        }
    } else {
        quote!(0usize)
    }
}

/// Renders `frame_exact` for schemas whose last field takes the rest of the input.
fn render_exact_frame(
    schema: &Schema,
    names: &Names,
    error_names: &error::VariantNames,
    descriptor_value: &TokenStream,
    consumed: &TokenStream,
    runtime: &TokenStream,
) -> TokenStream {
    let Names {
        error,
        frame_input,
        frame_offset,
        ..
    } = names;
    if matches!(
        schema.fields.last().map(|field| &field.kind),
        Some(FieldKind::Array(_))
    ) || matches!(
        schema.fields.last().map(|field| &field.kind),
        Some(FieldKind::Nested(nested)) if nested.terminal && nested.extent.is_none()
    ) {
        let exact_steps = frame::render_steps(
            schema,
            error,
            error_names,
            frame_input,
            frame_offset,
            runtime,
            true,
        );
        quote! {
            #[inline]
            fn frame_exact(
                #frame_input: &[u8],
                #frame_offset: usize,
            ) -> Result<#runtime::Frame<Self::State>, Self::Error> {
                #(#exact_steps)*
                Ok(#runtime::Frame::new(#descriptor_value, #consumed))
            }
        }
    } else {
        TokenStream::new()
    }
}

fn render_nested_selection_arms(schema: &Schema) -> Vec<TokenStream> {
    schema
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let FieldKind::Nested(nested) = &field.kind else {
                return None;
            };
            let name = &field.name;
            let ty = &nested.ty;
            Some(quote! {
                #index => {
                    // SAFETY: this hook receives the parent schema's retained exact span/state pair.
                    let parent = unsafe { Self::selection_field_range(input, state, #index) }?;
                    let child_input = input.get(parent.clone())?;
                    // SAFETY: child_input and child state were framed together at this field site.
                    let child_range = unsafe { Route::resolve::<#ty>(child_input, &state.#name) }?;
                    let start = parent.start.checked_add(child_range.start)?;
                    let end = parent.start.checked_add(child_range.end)?;
                    (end <= parent.end).then_some(start..end)
                }
            })
        })
        .collect()
}

/// Renders the schema validator calls that run on a framed, retained view.
fn render_validator_calls(
    schema: &Schema,
    names: &Names,
    error_names: &error::VariantNames,
) -> Vec<TokenStream> {
    let Names {
        error,
        retained_value,
        ..
    } = names;
    schema
        .validators
        .iter()
        .zip(&error_names.validators)
        .map(
            |(validator, variant)| quote!(#validator(&#retained_value).map_err(#error::#variant)?;),
        )
        .collect()
}

fn render_select_impl(
    schema: &Schema,
    names: &Names,
    targets: &ImplTargets,
    error_names: &error::VariantNames,
    runtime: &TokenStream,
) -> TokenStream {
    let Names {
        error,
        view_impl,
        backing,
        input_field,
        represented_length_field,
        view_marker,
        current_input,
        view_input,
        input_length,
        frame_result,
        framed_descriptor,
        framed_consumed,
        retained_value,
        ..
    } = names;
    let ImplTargets {
        self_type,
        retained_type,
        ..
    } = targets;
    let (impl_generics, _, where_clause) = targets.bounded.split_for_impl();
    let validator_calls = render_validator_calls(schema, names, error_names);
    let validator_view_calls = schema
        .validators
        .iter()
        .zip(&error_names.validators)
        .map(|(validator, variant)| quote!(#validator(view).map_err(#error::#variant)?;))
        .collect::<Vec<_>>();
    quote! {
        impl #impl_generics #runtime::__private::WireSelect for #self_type #where_clause {
            type Root<#backing> = #retained_type
            where
                #backing: AsRef<[u8]>;

            #[inline]
            fn select_view<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<Self::Root<#backing>, Self::Error> {
                let #current_input = #view_input.as_ref();
                let #input_length = #current_input.len();
                let #frame_result = <Self as #runtime::WireView>::frame_exact(#current_input, 0)?;
                let (#framed_descriptor, #framed_consumed) = #frame_result.into_parts();
                if #framed_consumed > #input_length {
                    return Err(#error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed: #framed_consumed,
                        available: #input_length,
                    }));
                }
                if #framed_consumed < #input_length {
                    return Err(#error::Trailing(#runtime::TrailingBytes {
                        offset: #framed_consumed,
                        trailing: #input_length - #framed_consumed,
                    }));
                }
                Ok(#view_impl {
                    #input_field: #view_input,
                    #represented_length_field: #input_length,
                    descriptor: #framed_descriptor,
                    #view_marker: ::core::marker::PhantomData,
                })
            }

            #[inline]
            fn validated_view<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<Self::Root<#backing>, Self::Error> {
                let #current_input = #view_input.as_ref();
                let #input_length = #current_input.len();
                let #frame_result = <Self as #runtime::WireView>::frame_exact(#current_input, 0)?;
                let (#framed_descriptor, #framed_consumed) = #frame_result.into_parts();
                if #framed_consumed > #input_length {
                    return Err(#error::InvalidFrame(#runtime::InvalidFrameExtent {
                        offset: 0,
                        consumed: #framed_consumed,
                        available: #input_length,
                    }));
                }
                let mut #retained_value = #view_impl {
                    #input_field: #view_input,
                    #represented_length_field: #framed_consumed,
                    descriptor: #framed_descriptor,
                    #view_marker: ::core::marker::PhantomData,
                };
                #(#validator_calls)*
                if #framed_consumed < #input_length {
                    return Err(#error::Trailing(#runtime::TrailingBytes {
                        offset: #framed_consumed,
                        trailing: #input_length - #framed_consumed,
                    }));
                }
                #retained_value.#represented_length_field = #input_length;
                Ok(#retained_value)
            }
            unsafe fn framed_view<#backing: AsRef<[u8]>>(
                #view_input: #backing,
                state: Self::State,
            ) -> Self::Root<#backing> {
                let #input_length = #view_input.as_ref().len();
                #view_impl {
                    #input_field: #view_input,
                    #represented_length_field: #input_length,
                    descriptor: state,
                    #view_marker: ::core::marker::PhantomData,
                }
            }

            fn validate_view<#backing: AsRef<[u8]>>(
                view: &Self::Root<#backing>,
            ) -> Result<(), Self::Error> {
                #(#validator_view_calls)*
                Ok(())
            }
        }
    }
}

fn render_leading_impl(
    schema: &Schema,
    targets: &ImplTargets,
    runtime: &TokenStream,
) -> TokenStream {
    let self_type = &targets.self_type;
    let (impl_generics, _, where_clause) = targets.bounded.split_for_impl();
    if schema.is_syntactically_fixed() {
        quote! {
            // SAFETY: generated fixed framing is deterministic for one immutable span; the
            // sequence constructor rejects a zero width before iteration.
            unsafe impl #impl_generics #runtime::__private::LeadingWire for #self_type #where_clause {}
        }
    } else {
        TokenStream::new()
    }
}

/// Renders the inherent `views`, `cursor`, `next`, `view`, and `view_unchecked` entry points.
fn render_inherent_impl(
    schema: &Schema,
    names: &Names,
    targets: &ImplTargets,
    error_type: &TokenStream,
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    let Names {
        backing,
        view_input,
        ..
    } = names;
    let ImplTargets {
        bounded,
        self_type,
        trait_path,
        ..
    } = targets;
    let (impl_generics, _, where_clause) = bounded.split_for_impl();
    let sequence_lifetime = fresh_schema_lifetime(schema, bounded, "sequence");
    let views_method = render_views_method(schema, &sequence_lifetime, runtime);
    let cursor_methods = render_cursor_methods(schema, &sequence_lifetime, runtime);
    quote! {
        impl #impl_generics #self_type #where_clause {
            #views_method
            #cursor_methods

            /// Validates one exact representation, including schema validators.
            #[inline]
            #vis fn view<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #error_type> {
                <Self as #runtime::__private::WireSelect>::validated_view(#view_input)
            }

            /// Validates exact structural framing while skipping schema validators.
            #[inline]
            #vis fn view_unchecked<#backing: AsRef<[u8]>>(
                #view_input: #backing,
            ) -> Result<impl #trait_path + #runtime::ExactWire<Self>, #error_type> {
                <Self as #runtime::__private::WireSelect>::select_view(#view_input)
            }
        }
    }
}

fn render_views_method(
    schema: &Schema,
    sequence_lifetime: &syn::Lifetime,
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    if schema.is_syntactically_fixed() {
        quote! {
            /// Validates consecutive fixed-width representations and returns an exact iterator.
            #vis fn views<#sequence_lifetime>(
                input: &#sequence_lifetime [u8],
            ) -> Result<
                #runtime::FixedViews<#sequence_lifetime, Self>,
                #runtime::SequenceError<<Self as #runtime::WireView>::Error>,
            >
            where
                Self: #runtime::__private::LeadingWire,
            {
                #runtime::FixedViews::new(input)
            }
        }
    } else if schema.has_leading_extent() {
        quote! {
            /// Returns a lazy facade over consecutive variable-width representations.
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
        }
    } else {
        TokenStream::new()
    }
}

fn render_cursor_methods(
    schema: &Schema,
    sequence_lifetime: &syn::Lifetime,
    runtime: &TokenStream,
) -> TokenStream {
    let vis = &schema.vis;
    if schema.has_leading_extent() {
        quote! {
            /// Frames the first representation and returns a cursor over the suffix.
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

            /// Frames this schema at the cursor without advancing on failure.
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
    }
}

fn bounded_generics(schema: &Schema, runtime: &TokenStream) -> Generics {
    let mut generics = schema.generics.clone();
    for field in schema.nested_fields() {
        let FieldKind::Nested(nested) = &field.kind else {
            unreachable!()
        };
        let ty = &nested.ty;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: #runtime::WireView));
    }
    for field in schema.array_fields() {
        let FieldKind::Array(array) = &field.kind else {
            unreachable!()
        };
        let item = &array.item;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#item: #runtime::WireView));
    }
    for field in &schema.fields {
        let FieldKind::Scalar(scalar) = &field.kind else {
            continue;
        };
        let ValueType::Custom(ty) = &scalar.value_type else {
            continue;
        };
        let wire = scalar_type_tokens(scalar.wire_type);
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: Copy + TryFrom<#wire> + 'static));
    }
    generics
}
