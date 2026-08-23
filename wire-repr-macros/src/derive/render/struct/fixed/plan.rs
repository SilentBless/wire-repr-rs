//! Fixed-struct prepared encoding rendering.

use crate::derive::model::{Field, FieldPosition};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) struct Input<'a> {
    pub(super) schema: Schema<'a>,
    pub(super) layout: Layout<'a>,
    pub(super) types: Types<'a>,
    pub(super) runtime: &'a TokenStream,
}

pub(super) struct Schema<'a> {
    pub(super) fields: &'a [Field],
    pub(super) labels: &'a [String],
    pub(super) variants: &'a [Ident],
}

pub(super) struct Layout<'a> {
    pub(super) plans: &'a [Ident],
    pub(super) gaps: &'a [Option<Ident>],
    pub(super) gap_names: &'a [&'a Ident],
}

pub(super) struct Types<'a> {
    pub(super) vis: &'a syn::Visibility,
    pub(super) plan: &'a Ident,
    pub(super) encode_error: &'a Ident,
    pub(super) field_proxy: &'a Ident,
    pub(super) self_type: &'a TokenStream,
    pub(super) impl_generics: &'a TokenStream,
}

pub(super) struct Output {
    pub(super) declaration: TokenStream,
    pub(super) wire_encode: TokenStream,
}

pub(super) fn render(input: Input<'_>) -> Output {
    let Input {
        schema: Schema {
            fields,
            labels,
            variants,
        },
        layout: Layout {
            plans,
            gaps,
            gap_names,
        },
        types:
            Types {
                vis,
                plan,
                encode_error,
                field_proxy,
                self_type,
                impl_generics,
            },
        runtime,
    } = input;
    let plan_fields = fields.iter().zip(plans).map(|(field, field_plan)| {
        let codec = fixed_codec(field, runtime);
        quote!(#field_plan: <#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>)
    });
    let prepare_steps =
        fields
            .iter()
            .zip(plans)
            .zip(variants)
            .map(|((field, field_plan), variant)| {
                let field_name = &field.name;
                let codec = fixed_codec(field, runtime);
                quote! {
                    let #field_plan = <#codec as #runtime::FixedCodec>::plan(self.#field_name)
                        .map_err(#encode_error::#variant)?;
                }
            });
    let geometry_steps = fields.iter().zip(plans).zip(gaps).zip(labels).map(
        |(((field, field_plan), gap), label)| {
            let length = quote!(#runtime::ByteSource::byte_len(&#field_plan));
            if let (Some(gap), Some(position)) = (gap, &field.position) {
                let field_start = match position {
                    FieldPosition::Static(position) => quote!(#position),
                    FieldPosition::Source(source) => quote! {
                        usize::try_from(self.#source).map_err(|_| {
                            #encode_error::PositionNotRepresentable {
                                field: #label,
                                value: self.#source as u128,
                            }
                        })?
                    },
                };
                quote! {
                    let field_start = #field_start;
                    if field_start < encoded_len {
                        return Err(#encode_error::PositionBeforeCursor {
                            field: #label,
                            position: field_start,
                            cursor: encoded_len,
                        });
                    }
                    let #gap = field_start - encoded_len;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else if let Some(gap) = gap {
                let padding = field.padding_before;
                let alignment = match field.alignment_before {
                    Some(boundary) => quote!(Some(#boundary)),
                    None => quote!(None::<usize>),
                };
                quote! {
                    let before_gap = encoded_len;
                    let padded = encoded_len
                        .checked_add(#padding)
                        .ok_or(#encode_error::LengthOverflow)?;
                    let alignment_padding = match #alignment {
                        Some(boundary) => {
                            let remainder = padded % boundary;
                            if remainder == 0 { 0 } else { boundary - remainder }
                        }
                        None => 0,
                    };
                    let field_start = padded
                        .checked_add(alignment_padding)
                        .ok_or(#encode_error::LengthOverflow)?;
                    let #gap = field_start - before_gap;
                    encoded_len = field_start
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            } else {
                quote! {
                    encoded_len = encoded_len
                        .checked_add(#length)
                        .ok_or(#encode_error::LengthOverflow)?;
                }
            }
        },
    );
    let emit_steps = plans.iter().zip(gaps).map(|(field_plan, gap)| {
        let padding = gap
            .as_ref()
            .map(|gap| quote!(#runtime::ByteSink::fill(sink, 0, self.#gap);));
        quote! {
            #padding
            #runtime::ByteSource::emit_to(&self.#field_plan, sink);
        }
    });
    let cursor_bounds = fields.iter().map(|field| {
        let codec = fixed_codec(field, runtime);
        quote!(<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value>: #runtime::ByteSourceCursor)
    });
    let mut segment_types = Vec::new();
    let mut segment_values = Vec::new();
    for ((field, field_plan), gap) in fields.iter().zip(plans).zip(gaps) {
        if let Some(gap) = gap {
            segment_types
                .push(quote!(::core::iter::Once<#runtime::ByteSegment<'__wire_repr_source>>));
            segment_values.push(quote!(::core::iter::once(#runtime::ByteSegment::Rest {
                byte: 0,
                len: self.#gap,
            })));
        }
        let codec = fixed_codec(field, runtime);
        segment_types.push(quote!(
            <<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Segments<'__wire_repr_source>
        ));
        segment_values.push(quote!(#runtime::ByteSourceCursor::segments(&self.#field_plan)));
    }
    let segments_type = segment_types
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Chain<#left, #right>))
        .expect("fixed structs have fields");
    let segments_value = segment_values
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Iterator::chain(#left, #right)))
        .expect("fixed structs have fields");
    let mut bytes_types = Vec::new();
    let mut bytes_values = Vec::new();
    for ((field, field_plan), gap) in fields.iter().zip(plans).zip(gaps) {
        if let Some(gap) = gap {
            bytes_types.push(quote!(::core::iter::Take<::core::iter::Repeat<u8>>));
            bytes_values.push(quote!(::core::iter::repeat(0).take(self.#gap)));
        }
        let codec = fixed_codec(field, runtime);
        bytes_types.push(quote!(
            <<#codec as #runtime::FixedCodec>::Plan<'__wire_repr_value> as #runtime::ByteSourceCursor>::Bytes<'__wire_repr_source>
        ));
        bytes_values.push(quote!(#runtime::ByteSourceCursor::bytes(&self.#field_plan)));
    }
    let bytes_type = bytes_types
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Chain<#left, #right>))
        .expect("fixed structs have fields");
    let bytes_value = bytes_values
        .into_iter()
        .reduce(|left, right| quote!(::core::iter::Iterator::chain(#left, #right)))
        .expect("fixed structs have fields");

    let declaration = quote! {
        /// A prepared encoding for this wire representation.
        #vis struct #plan<'__wire_repr_value> {
            #(#plan_fields,)*
            #(#gap_names: usize,)*
            encoded_len: usize,
        }

        #[allow(missing_docs)]
        impl #plan<'_> {
            #[must_use]
            #vis const fn encoded_len(&self) -> usize {
                self.encoded_len
            }

            #[doc = "Returns a byte-selection root for this prepared representation."]
            #[must_use]
            #vis fn bytes(&self) -> #runtime::ByteSelection<'_, Self, #field_proxy<#runtime::RootScope>> {
                #runtime::ByteSelection::new(self, #field_proxy::__wire_repr_new())
            }
        }

        impl<'__wire_repr_value> #runtime::ByteSource for #plan<'__wire_repr_value> {
            #[inline(always)]
            fn byte_len(&self) -> usize {
                self.encoded_len
            }

            #[inline(always)]
            fn emit_to<S: #runtime::ByteSink>(&self, sink: &mut S) {
                #(#emit_steps)*
            }
        }

        impl<'__wire_repr_value> #runtime::ByteSourceCursor for #plan<'__wire_repr_value>
        where
            #(#cursor_bounds,)*
        {
            type Segments<'__wire_repr_source> = #segments_type
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn segments(&self) -> Self::Segments<'_> {
                #segments_value
            }

            type Bytes<'__wire_repr_source> = #bytes_type
            where
                Self: '__wire_repr_source;

            #[inline(always)]
            fn bytes(&self) -> Self::Bytes<'_> {
                #bytes_value
            }
        }

        impl<'__wire_repr_value> #runtime::PreparedLayout for #plan<'__wire_repr_value> {
            type Written<'__wire_repr_output> = #runtime::Written<'__wire_repr_output>;

            fn commit_into<'__wire_repr_output>(
                self,
                output: &'__wire_repr_output mut [u8],
            ) -> Result<
                (Self::Written<'__wire_repr_output>, &'__wire_repr_output mut [u8]),
                #runtime::OutputTooShortError,
            > {
                let required = self.encoded_len;
                if output.len() < required {
                    return Err(#runtime::OutputTooShortError {
                        required,
                        available: output.len(),
                    });
                }
                let (bytes, suffix) = output.split_at_mut(required);
                #runtime::ByteSource::write_into(&self, bytes);
                Ok((#runtime::Written::new(bytes), suffix))
            }
        }
    };

    let wire_encode = quote! {
        impl #impl_generics #runtime::WireEncode for #self_type {
            type EncodeError = #encode_error;
            type Plan<'__wire_repr_value> = #plan<'__wire_repr_value>
            where
                Self: '__wire_repr_value;

            fn prepare<'__wire_repr_value>(
                self,
            ) -> Result<Self::Plan<'__wire_repr_value>, Self::EncodeError>
            where
                Self: '__wire_repr_value,
            {
                #(#prepare_steps)*
                let mut encoded_len = 0usize;
                #(#geometry_steps)*
                Ok(#plan {
                    #(#plans,)*
                    #(#gap_names,)*
                    encoded_len,
                })
            }
        }
    };

    Output {
        declaration,
        wire_encode,
    }
}

fn fixed_codec(field: &Field, runtime: &TokenStream) -> TokenStream {
    match &field.kind {
        crate::derive::model::FieldKind::Fixed(codec) => super::super::codec_tokens(codec, runtime),
        _ => unreachable!(),
    }
}
