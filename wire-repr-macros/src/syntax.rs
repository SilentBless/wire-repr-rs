//! Exact token grammar for canonical `wire_repr!` input.

use proc_macro2::Span;
use syn::{
    Attribute, Error, Ident, LitInt, Meta, Path, Result, Token, Type, Visibility, braced,
    parse::{Parse, ParseStream},
};

mod keyword {
    syn::custom_keyword!(absolute);
    syn::custom_keyword!(align);
    syn::custom_keyword!(bytes);
    syn::custom_keyword!(padding);
    syn::custom_keyword!(bytes_to);
    syn::custom_keyword!(remaining_bytes);
    syn::custom_keyword!(variable);
    syn::custom_keyword!(projections);
    syn::custom_keyword!(bit);
    syn::custom_keyword!(bits);
    syn::custom_keyword!(derive);
    syn::custom_keyword!(derive_error);
    syn::custom_keyword!(finalize);
    syn::custom_keyword!(context);
    syn::custom_keyword!(value);
    syn::custom_keyword!(len);
}

/// Parsed invocation before semantic normalization.
pub(crate) struct Invocation {
    pub(crate) items: Vec<Item>,
}

/// A top-level declaration in source order.
pub(crate) enum Item {
    /// A byte-backed layout declaration.
    Layout(Layout),
    /// A transparent nominal fixed integer scalar.
    Scalar(Scalar),
}

/// Parsed scalar declaration before semantic normalization.
pub(crate) struct Scalar {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) name: Ident,
    pub(crate) storage: Codec,
}

/// Parsed layout declaration.
pub(crate) struct Layout {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) kind: LayoutKind,
    pub(crate) name: Ident,
    pub(crate) contexts: Vec<Context>,
    pub(crate) fields: Vec<Field>,
    pub(crate) physical: Vec<PhysicalEntry>,
}

/// A generated-builder-only borrowed input declaration.
pub(crate) struct Context {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) name: Ident,
    pub(crate) referent: Type,
}

/// A field or spacing placement before semantic normalization.
pub(crate) enum Placement {
    Explicit(LitInt),
    Implicit(Span),
}

impl Placement {
    pub(crate) fn span(&self) -> Span {
        match self {
            Self::Explicit(value) => value.span(),
            Self::Implicit(span) => *span,
        }
    }

    pub(crate) const fn is_explicit(&self) -> bool {
        matches!(self, Self::Explicit(_))
    }
}

/// A parsed physical sequential-layout entry.
pub(crate) enum PhysicalEntry {
    Field(usize),
    Padding {
        placement: Placement,
        length: LitInt,
    },
    Alignment {
        placement: Placement,
        boundary: LitInt,
    },
}

/// Parsed placement mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LayoutKind {
    /// Fields use one-based contiguous positions.
    Sequential,
    /// Fields use zero-based absolute byte offsets.
    Absolute,
}

/// Parsed field declaration.
pub(crate) struct Field {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) name: Ident,
    pub(crate) codec: Codec,
    pub(crate) mapping: Option<Path>,
    pub(crate) placement: Placement,
    pub(crate) projections: Vec<Projection>,
    pub(crate) derivation: Option<Derivation>,
    pub(crate) finalization: Option<Finalization>,
}

/// Builder-only fixed-field derivation declaration.
pub(crate) struct Derivation {
    pub(crate) function: Path,
    pub(crate) operands: Vec<DeriveOperand>,
    pub(crate) error: Path,
}

/// One explicit derived-field dependency.
pub(crate) enum DeriveOperand {
    Value { source: Ident, span: Span },
    Len { source: Ident, span: Span },
}

/// Builder-only infallible post-write finalization declaration.
pub(crate) struct Finalization {
    pub(crate) function: Path,
    pub(crate) operands: Vec<FinalizeOperand>,
}

/// One explicit finalizer input.
pub(crate) enum FinalizeOperand {
    Bytes {
        start: FinalizeBoundary,
        end: FinalizeBoundary,
    },
    Context {
        source: Ident,
        span: Span,
    },
    Value {
        source: Ident,
        span: Span,
    },
}

/// A restricted finalizer byte boundary.
pub(crate) enum FinalizeBoundary {
    BufStart(Span),
    BufEnd(Span),
    FieldStart { source: Ident, span: Span },
    FieldEnd { source: Ident, span: Span },
}

/// Parsed storage-owned bit projection.
pub(crate) struct Projection {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) kind: ProjectionKind,
    pub(crate) name: Ident,
    pub(crate) start: LitInt,
    pub(crate) end: LitInt,
}

/// Projection shape before numeric normalization.
#[derive(Clone, Copy)]
pub(crate) enum ProjectionKind {
    Bit,
    Bits,
}

/// Codec spelling before builtin resolution.
pub(crate) enum Codec {
    /// A bare identifier, which may be a builtin or a semantic error later.
    Bare(Ident),
    /// A custom structural fixed codec path.
    Custom(Path),
    /// A structural borrowed byte span with a validated fixed width.
    Bytes(usize),
    /// A custom self-delimiting codec path.
    Prefix(Path),
    /// An opaque byte span with a restricted wire-relative range expression.
    Range(ByteRangeSyntax),
}

/// Exact syntax for a dynamic byte range before semantic normalization.
pub(crate) struct ByteRangeSyntax {
    pub(crate) end: ByteRangeEnd,
}

/// A restricted byte-range end expression.
pub(crate) enum ByteRangeEnd {
    BufEnd,
    Relative { source: Ident, span: Span },
    Absolute { source: Ident, span: Span },
}

impl Parse for Invocation {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(parse_item(input)?);
        }
        Ok(Self { items })
    }
}

fn parse_item(input: ParseStream<'_>) -> Result<Item> {
    let fork = input.fork();
    parse_docs(&fork)?;
    fork.parse::<Visibility>()?;
    if fork.peek(Token![struct]) {
        let keyword: Token![struct] = fork.parse()?;
        return Err(Error::new(
            keyword.span,
            "expected `layout` or `scalar`; `struct` is not wire_repr syntax",
        ));
    }
    if !fork.peek(keyword::absolute) {
        let keyword: Ident = fork
            .parse()
            .map_err(|_| Error::new(fork.span(), "expected `layout` or `scalar`"))?;
        if keyword == "scalar" {
            return parse_scalar(input).map(Item::Scalar);
        }
    }
    parse_layout(input).map(Item::Layout)
}

fn parse_scalar(input: ParseStream<'_>) -> Result<Scalar> {
    let docs = parse_docs(input)?;
    let visibility = input.parse::<Visibility>()?;
    let keyword: Ident = input
        .parse()
        .map_err(|_| Error::new(input.span(), "expected `scalar`"))?;
    if keyword != "scalar" {
        return Err(Error::new(keyword.span(), "expected `scalar`"));
    }
    let name = input.parse()?;
    input.parse::<Token![:]>()?;
    let storage = parse_codec(input)?;
    input.parse::<Token![;]>()?;
    Ok(Scalar {
        docs,
        visibility,
        name,
        storage,
    })
}

fn parse_layout(input: ParseStream<'_>) -> Result<Layout> {
    let docs = parse_docs(input)?;
    let visibility = input.parse::<Visibility>()?;

    if input.peek(Token![struct]) {
        let keyword: Token![struct] = input.parse()?;
        return Err(Error::new(
            keyword.span,
            "expected `layout`; `struct` is not wire_repr syntax",
        ));
    }

    let kind = if input.peek(keyword::absolute) {
        input.parse::<keyword::absolute>()?;
        LayoutKind::Absolute
    } else {
        LayoutKind::Sequential
    };

    let keyword: Ident = input
        .parse()
        .map_err(|_| Error::new(input.span(), "expected `layout`"))?;
    if keyword != "layout" {
        return Err(Error::new(keyword.span(), "expected `layout`"));
    }

    let name = input.parse()?;
    let content;
    braced!(content in input);
    let mut contexts = Vec::new();
    let mut fields = Vec::new();
    let mut physical = Vec::new();
    while !content.is_empty() {
        let declaration = content.fork();
        parse_docs(&declaration)?;
        if declaration.peek(keyword::context) {
            if !physical.is_empty() {
                return Err(Error::new(
                    declaration.span(),
                    "`context` declarations must appear before physical layout entries",
                ));
            }
            contexts.push(parse_context(&content)?);
        } else {
            let visibility = declaration.parse::<Visibility>()?;
            if !matches!(visibility, Visibility::Inherited) && declaration.peek(keyword::context) {
                return Err(Error::new(
                    declaration.span(),
                    "`context` declarations do not support visibility",
                ));
            }
            if content.peek(keyword::padding) {
                physical.push(parse_padding(&content)?);
            } else if content.peek(keyword::align) {
                physical.push(parse_alignment(&content)?);
            } else {
                let declaration_index = fields.len();
                fields.push(parse_field(&content, kind)?);
                physical.push(PhysicalEntry::Field(declaration_index));
            }
        }
    }

    Ok(Layout {
        docs,
        visibility,
        kind,
        name,
        contexts,
        fields,
        physical,
    })
}

fn parse_context(input: ParseStream<'_>) -> Result<Context> {
    let docs = parse_docs(input)?;
    input.parse::<keyword::context>()?;
    let name = input.parse()?;
    input.parse::<Token![:]>()?;
    let referent = input.parse()?;
    input.parse::<Token![;]>()?;
    Ok(Context {
        docs,
        name,
        referent,
    })
}

fn parse_padding(input: ParseStream<'_>) -> Result<PhysicalEntry> {
    input.parse::<keyword::padding>()?;
    let length = parse_spacing_value(input, "padding")?;
    let placement = parse_spacing_placement(input)?;
    input.parse::<Token![;]>()?;
    Ok(PhysicalEntry::Padding { placement, length })
}

fn parse_alignment(input: ParseStream<'_>) -> Result<PhysicalEntry> {
    input.parse::<keyword::align>()?;
    let boundary = parse_spacing_value(input, "align")?;
    let placement = parse_spacing_placement(input)?;
    input.parse::<Token![;]>()?;
    Ok(PhysicalEntry::Alignment {
        placement,
        boundary,
    })
}

fn parse_spacing_value(input: ParseStream<'_>, name: &str) -> Result<LitInt> {
    let content;
    syn::parenthesized!(content in input);
    let value = content.parse::<LitInt>()?;
    if !content.is_empty() {
        return Err(Error::new(
            content.span(),
            format!("expected one `{name}` value"),
        ));
    }
    Ok(value)
}

fn parse_spacing_placement(input: ParseStream<'_>) -> Result<Placement> {
    if input.peek(Token![@]) {
        input.parse::<Token![@]>()?;
        return input.parse().map(Placement::Explicit);
    }
    Ok(Placement::Implicit(input.span()))
}

fn parse_field(input: ParseStream<'_>, kind: LayoutKind) -> Result<Field> {
    let docs = parse_docs(input)?;
    let name: Ident = input.parse()?;
    let placement = if input.peek(Token![@]) {
        input.parse::<Token![@]>()?;
        Placement::Explicit(input.parse()?)
    } else {
        Placement::Implicit(name.span())
    };
    input.parse::<Token![:]>()?;
    let codec = parse_codec(input)?;
    let mapping = if input.peek(Token![as]) {
        input.parse::<Token![as]>()?;
        Some(input.parse::<Path>()?)
    } else {
        None
    };
    if kind == LayoutKind::Absolute && !placement.is_explicit() {
        return Err(Error::new(
            name.span(),
            "absolute fields require `name @ offset: Codec;`",
        ));
    }
    if input.peek(Token![;]) {
        input.parse::<Token![;]>()?;
        return Ok(Field {
            docs,
            name,
            codec,
            mapping,
            placement,
            projections: Vec::new(),
            derivation: None,
            finalization: None,
        });
    }
    let content;
    braced!(content in input);
    let mut projections = Vec::new();
    let mut derivation = None;
    let mut derive_error = None;
    let mut finalization = None;
    while !content.is_empty() {
        if content.peek(keyword::projections) {
            if !projections.is_empty() {
                return Err(Error::new(content.span(), "duplicate `projections` clause"));
            }
            content.parse::<keyword::projections>()?;
            let projection_content;
            braced!(projection_content in content);
            if projection_content.is_empty() {
                return Err(Error::new(
                    projection_content.span(),
                    "`projections` must contain at least one projection",
                ));
            }
            while !projection_content.is_empty() {
                projections.push(parse_projection(&projection_content)?);
            }
        } else if content.peek(keyword::derive) {
            if finalization.is_some() {
                return Err(Error::new(
                    content.span(),
                    "a field cannot have both `derive` and `finalize`",
                ));
            }
            if derivation.is_some() {
                return Err(Error::new(content.span(), "duplicate `derive` clause"));
            }
            content.parse::<keyword::derive>()?;
            content.parse::<Token![:]>()?;
            let function = content.parse::<Path>()?;
            let operands_content;
            syn::parenthesized!(operands_content in content);
            let mut operands = Vec::new();
            while !operands_content.is_empty() {
                let operand_name: Ident = operands_content.parse()?;
                let span = operand_name.span();
                let operand_content;
                syn::parenthesized!(operand_content in operands_content);
                let source = operand_content.parse::<Ident>()?;
                if !operand_content.is_empty() {
                    return Err(Error::new(
                        operand_content.span(),
                        "expected one field identifier",
                    ));
                }
                let operand = if operand_name == "value" {
                    DeriveOperand::Value { source, span }
                } else if operand_name == "len" {
                    DeriveOperand::Len { source, span }
                } else {
                    return Err(Error::new(span, "expected `value(field)` or `len(range)`"));
                };
                operands.push(operand);
                if operands_content.is_empty() {
                    break;
                }
                operands_content.parse::<Token![,]>()?;
            }
            derivation = Some((function, operands));
            content.parse::<Token![;]>()?;
        } else if content.peek(keyword::finalize) {
            if finalization.is_some() {
                return Err(Error::new(content.span(), "duplicate `finalize` clause"));
            }
            if derivation.is_some() {
                return Err(Error::new(
                    content.span(),
                    "a field cannot have both `derive` and `finalize`",
                ));
            }
            finalization = Some(parse_finalization(&content)?);
        } else if content.peek(keyword::derive_error) {
            if finalization.is_some() {
                return Err(Error::new(
                    content.span(),
                    "`derive_error` is only valid with `derive`, not `finalize`",
                ));
            }
            if derive_error.is_some() {
                return Err(Error::new(
                    content.span(),
                    "duplicate `derive_error` clause",
                ));
            }
            content.parse::<keyword::derive_error>()?;
            content.parse::<Token![:]>()?;
            derive_error = Some(content.parse::<Path>()?);
            content.parse::<Token![;]>()?;
        } else if !projections.is_empty() {
            return Err(Error::new(content.span(), "expected `projections` clause"));
        } else {
            return Err(Error::new(
                content.span(),
                "expected `projections`, `derive`, `derive_error`, or `finalize`",
            ));
        }
    }
    let derivation = match (derivation, derive_error) {
        (Some((function, operands)), Some(error)) => Some(Derivation {
            function,
            operands,
            error,
        }),
        (Some(_), None) => return Err(Error::new(name.span(), "`derive` requires `derive_error`")),
        (None, Some(_)) => return Err(Error::new(name.span(), "`derive_error` requires `derive`")),
        (None, None) => None,
    };
    input.parse::<Token![;]>()?;
    Ok(Field {
        docs,
        name,
        codec,
        mapping,
        placement,
        projections,
        derivation,
        finalization,
    })
}

fn parse_finalization(input: ParseStream<'_>) -> Result<Finalization> {
    input.parse::<keyword::finalize>()?;
    input.parse::<Token![:]>()?;
    let function = input.parse()?;
    let operands_content;
    syn::parenthesized!(operands_content in input);
    if operands_content.is_empty() {
        return Err(Error::new(
            operands_content.span(),
            "`finalize` requires at least one operand",
        ));
    }
    let mut operands = Vec::new();
    while !operands_content.is_empty() {
        operands.push(parse_finalize_operand(&operands_content)?);
        if operands_content.is_empty() {
            break;
        }
        operands_content.parse::<Token![,]>()?;
    }
    input.parse::<Token![;]>()?;
    Ok(Finalization { function, operands })
}

fn parse_finalize_operand(input: ParseStream<'_>) -> Result<FinalizeOperand> {
    let operand_name: Ident = input.parse().map_err(|_| {
        Error::new(
            input.span(),
            "expected `bytes(...)`, `context(name)`, or `value(field)`",
        )
    })?;
    let span = operand_name.span();
    let operand_content;
    syn::parenthesized!(operand_content in input);
    if operand_name == "bytes" {
        let start = parse_finalize_boundary(&operand_content)?;
        operand_content.parse::<Token![..]>().map_err(|_| {
            Error::new(
                operand_content.span(),
                "expected `..` in finalizer bytes range",
            )
        })?;
        let end = parse_finalize_boundary(&operand_content)?;
        if !operand_content.is_empty() {
            return Err(Error::new(
                operand_content.span(),
                "expected a restricted finalizer bytes range",
            ));
        }
        return Ok(FinalizeOperand::Bytes { start, end });
    }
    let source: Ident = operand_content.parse().map_err(|_| {
        Error::new(
            operand_content.span(),
            if operand_name == "context" {
                "expected one context identifier"
            } else if operand_name == "value" {
                "expected one field identifier"
            } else {
                "expected `bytes(...)`, `context(name)`, or `value(field)`"
            },
        )
    })?;
    if !operand_content.is_empty() {
        return Err(Error::new(
            operand_content.span(),
            if operand_name == "context" {
                "expected one context identifier"
            } else if operand_name == "value" {
                "expected one field identifier"
            } else {
                "expected `bytes(...)`, `context(name)`, or `value(field)`"
            },
        ));
    }
    if operand_name == "context" {
        Ok(FinalizeOperand::Context { source, span })
    } else if operand_name == "value" {
        Ok(FinalizeOperand::Value { source, span })
    } else {
        Err(Error::new(
            span,
            "expected `bytes(...)`, `context(name)`, or `value(field)`",
        ))
    }
}

fn parse_finalize_boundary(input: ParseStream<'_>) -> Result<FinalizeBoundary> {
    let first: Ident = input.parse().map_err(|_| {
        Error::new(
            input.span(),
            "expected `buf_start`, `buf_end`, `field.start`, or `field.end`",
        )
    })?;
    let span = first.span();
    if first == "buf_start" {
        return Ok(FinalizeBoundary::BufStart(span));
    }
    if first == "buf_end" {
        return Ok(FinalizeBoundary::BufEnd(span));
    }
    if !input.peek(Token![..]) && input.peek(Token![.]) {
        input.parse::<Token![.]>()?;
        let boundary: Ident = input
            .parse()
            .map_err(|_| Error::new(input.span(), "expected `start` or `end` after field name"))?;
        return match boundary.to_string().as_str() {
            "start" => Ok(FinalizeBoundary::FieldStart {
                source: first,
                span,
            }),
            "end" => Ok(FinalizeBoundary::FieldEnd {
                source: first,
                span,
            }),
            _ => Err(Error::new(
                boundary.span(),
                "expected `start` or `end` after field name",
            )),
        };
    }
    Err(Error::new(
        span,
        "expected `buf_start`, `buf_end`, `field.start`, or `field.end`",
    ))
}

fn parse_projection(input: ParseStream<'_>) -> Result<Projection> {
    let docs = parse_docs(input)?;
    let kind = if input.peek(keyword::bit) {
        input.parse::<keyword::bit>()?;
        ProjectionKind::Bit
    } else if input.peek(keyword::bits) {
        input.parse::<keyword::bits>()?;
        ProjectionKind::Bits
    } else {
        let actual: Ident = input
            .parse()
            .map_err(|_| Error::new(input.span(), "expected `bit` or `bits`"))?;
        return Err(Error::new(actual.span(), "expected `bit` or `bits`"));
    };
    let name = input.parse()?;
    input.parse::<Token![:]>()?;
    let start = input.parse::<LitInt>()?;
    let end = match kind {
        ProjectionKind::Bit => LitInt::new(start.base10_digits(), start.span()),
        ProjectionKind::Bits => {
            input
                .parse::<Token![..=]>()
                .map_err(|_| Error::new(input.span(), "expected inclusive `..=` range"))?;
            input.parse::<LitInt>()?
        }
    };
    input.parse::<Token![;]>()?;
    Ok(Projection {
        docs,
        kind,
        name,
        start,
        end,
    })
}

fn parse_codec(input: ParseStream<'_>) -> Result<Codec> {
    if input.peek(Token![::])
        || input.peek(Token![crate])
        || input.peek(Token![self])
        || input.peek(Token![super])
        || input.peek(Token![Self])
    {
        return input.parse::<Path>().map(Codec::Custom);
    }
    if input.peek(keyword::bytes) {
        input.parse::<keyword::bytes>()?;
        let content;
        syn::parenthesized!(content in input);
        if content.peek(LitInt) {
            let literal = content.parse::<LitInt>()?;
            if !content.is_empty() {
                return Err(Error::new(content.span(), "expected one byte width"));
            }
            if !literal.suffix().is_empty() {
                return Err(Error::new(literal.span(), "byte width must be unsuffixed"));
            }
            if !literal
                .to_string()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            {
                return Err(Error::new(
                    literal.span(),
                    "byte width must be a base-10 literal",
                ));
            }
            let width = literal
                .base10_parse::<usize>()
                .map_err(|_| Error::new(literal.span(), "byte width must fit in usize"))?;
            if width == 0 {
                return Err(Error::new(literal.span(), "byte width must be nonzero"));
            }
            return Ok(Codec::Bytes(width));
        }
        let source = content.parse::<Ident>().map_err(|_| {
            Error::new(
                content.span(),
                "expected one relative range source in `bytes(source)`",
            )
        })?;
        if !content.is_empty() {
            return Err(Error::new(
                content.span(),
                "expected one relative range source in `bytes(source)`",
            ));
        }
        return Ok(Codec::Range(ByteRangeSyntax {
            end: ByteRangeEnd::Relative {
                span: source.span(),
                source,
            },
        }));
    }
    if input.peek(keyword::bytes_to) {
        input.parse::<keyword::bytes_to>()?;
        let content;
        syn::parenthesized!(content in input);
        let source = content.parse::<Ident>().map_err(|_| {
            Error::new(
                content.span(),
                "expected one absolute endpoint source in `bytes_to(source)`",
            )
        })?;
        if !content.is_empty() {
            return Err(Error::new(
                content.span(),
                "expected one absolute endpoint source in `bytes_to(source)`",
            ));
        }
        return Ok(Codec::Range(ByteRangeSyntax {
            end: ByteRangeEnd::Absolute {
                span: source.span(),
                source,
            },
        }));
    }
    if input.peek(keyword::remaining_bytes) {
        input.parse::<keyword::remaining_bytes>()?;
        return Ok(Codec::Range(ByteRangeSyntax {
            end: ByteRangeEnd::BufEnd,
        }));
    }
    if input.peek(keyword::variable) {
        input.parse::<keyword::variable>()?;
        let content;
        syn::parenthesized!(content in input);
        let path = content.parse::<Path>()?;
        if !content.is_empty() {
            return Err(Error::new(
                content.span(),
                "expected one variable codec path",
            ));
        }
        return Ok(Codec::Prefix(path));
    }
    let first: Ident = input.parse().map_err(|_| {
        Error::new(input.span(), "expected a builtin codec, a direct codec path, `bytes(N)`, `bytes(source)`, `bytes_to(source)`, `remaining_bytes`, or `variable(path)`")
    })?;
    if first == "codec" && input.peek(syn::token::Paren) {
        return Err(Error::new(
            first.span(),
            "`codec(path)` is removed; use the codec path directly",
        ));
    }
    if first == "prefix" {
        return Err(Error::new(
            first.span(),
            "`prefix(path)` is removed; use `variable(path)`",
        ));
    }
    if input.peek(Token![::]) {
        let mut path: Path = syn::parse_quote!(#first);
        while input.peek(Token![::]) {
            input.parse::<Token![::]>()?;
            path.segments.push(input.parse()?);
        }
        return Ok(Codec::Custom(path));
    }
    Ok(Codec::Bare(first))
}

fn parse_docs(input: ParseStream<'_>) -> Result<Vec<Attribute>> {
    let attrs = input.call(Attribute::parse_outer)?;
    for attr in &attrs {
        if !attr.path().is_ident("doc") || !matches!(&attr.meta, Meta::NameValue(_)) {
            return Err(Error::new_spanned(
                attr,
                "only outer documentation attributes are supported here",
            ));
        }
    }
    Ok(attrs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    fn parse_error(source: &str) -> Error {
        match parse_str::<Invocation>(source) {
            Ok(_) => panic!("expected syntax error"),
            Err(error) => error,
        }
    }

    #[test]
    fn parses_canonical_field_placement_codecs_and_properties() {
        let parsed: Invocation = parse_str(
            "/// scalar\npub scalar Hardware: BeU16; layout Packet { #[doc = \"length\"] length @ 1: U8; payload @ 2: bytes(length); end @ 3: BeU16; absolute @ 4: bytes_to(end); tail @ 5: remaining_bytes; variable @ 6: variable(crate::Delimited); flags @ 7: U8 as crate::Flags { projections { bit r#type: 0; } }; padding(3) @ 8; align(4) @ 9; } absolute layout Absolute { tag @ 0: U8; payload @ 1: bytes(2); }",
        ).unwrap();
        let Item::Layout(layout) = &parsed.items[1] else {
            panic!("expected layout")
        };
        assert_eq!(layout.fields.len(), 7);
        assert!(matches!(
            layout.fields[1].codec,
            Codec::Range(ByteRangeSyntax {
                end: ByteRangeEnd::Relative { .. },
                ..
            })
        ));
        assert!(matches!(
            layout.fields[3].codec,
            Codec::Range(ByteRangeSyntax {
                end: ByteRangeEnd::Absolute { .. },
                ..
            })
        ));
        assert!(matches!(
            layout.fields[4].codec,
            Codec::Range(ByteRangeSyntax {
                end: ByteRangeEnd::BufEnd,
                ..
            })
        ));
        assert!(matches!(layout.fields[5].codec, Codec::Prefix(_)));
        assert_eq!(layout.fields[6].projections.len(), 1);
        assert!(matches!(
            layout.physical[7],
            PhysicalEntry::Padding {
                placement: Placement::Explicit(_),
                ..
            }
        ));
        assert!(matches!(
            layout.physical[8],
            PhysicalEntry::Alignment {
                placement: Placement::Explicit(_),
                ..
            }
        ));
        let Item::Layout(absolute) = &parsed.items[2] else {
            panic!("expected absolute layout")
        };
        assert_eq!(absolute.kind, LayoutKind::Absolute);
    }

    #[test]
    fn parses_implicit_sequential_entries_and_builder_clauses() {
        let parsed: Invocation = parse_str(
            "layout H { source: U8; derived: U8 { derive: crate::derive(value(source)); derive_error: crate::Error; }; flags: U8 { projections { bit enabled: 0; } }; padding(3); align(8); }",
        ).unwrap();
        let Item::Layout(layout) = &parsed.items[0] else {
            panic!("expected layout")
        };
        assert!(
            layout
                .fields
                .iter()
                .all(|field| matches!(field.placement, Placement::Implicit(_)))
        );
        assert_eq!(layout.fields[2].projections.len(), 1);
        assert!(matches!(
            layout.physical[3],
            PhysicalEntry::Padding {
                placement: Placement::Implicit(_),
                ..
            }
        ));
    }

    #[test]
    fn accepts_direct_fixed_codec_paths_and_keeps_bare_names_bare() {
        let parsed: Invocation = parse_str(
            "scalar Declared: U8; layout H { declared: Declared; crate_path: crate::Codec; self_path: self::Codec; super_path: super::Codec; absolute_path: ::external::Codec; module_path: module::Codec; }",
        )
        .unwrap();
        let Item::Layout(layout) = &parsed.items[1] else {
            panic!("expected layout")
        };
        assert!(matches!(layout.fields[0].codec, Codec::Bare(ref name) if name == "Declared"));
        for field in &layout.fields[1..] {
            assert!(matches!(field.codec, Codec::Custom(_)));
        }
    }

    #[test]
    fn rejects_legacy_codec_wrapper_with_migration_diagnostic() {
        let error = parse_error("layout H { value: codec(crate::Codec); }");
        assert!(
            error
                .to_string()
                .contains("`codec(path)` is removed; use the codec path directly")
        );
    }

    #[test]
    fn retains_parser_local_coverage_for_contexts_finalizers_and_diagnostics() {
        let parsed: Invocation = parse_str(
            "layout H { #[doc = \"context\"] context state: crate::State; checksum: BeU16 { finalize: crate::finish(bytes(buf_start..buf_end), context(state), value(checksum)); }; payload: remaining_bytes; }",
        )
        .unwrap();
        let Item::Layout(layout) = &parsed.items[0] else {
            panic!("expected layout")
        };
        assert_eq!(layout.contexts.len(), 1);
        assert_eq!(layout.contexts[0].docs.len(), 1);
        assert!(matches!(
            layout.fields[0]
                .finalization
                .as_ref()
                .unwrap()
                .operands
                .as_slice(),
            [
                FinalizeOperand::Bytes { .. },
                FinalizeOperand::Context { .. },
                FinalizeOperand::Value { .. }
            ]
        ));

        for (source, needle) in [
            ("layout H { field value: U8; }", "expected `:`"),
            (
                "layout H { value: U8 { projections { bit x: 0; } } }",
                "expected `;`",
            ),
            (
                "layout H { value: U8 { projections { } }; }",
                "must contain",
            ),
            (
                "layout H { value: U8; context state: crate::State; }",
                "must appear before physical",
            ),
            (
                "layout H { value: U8 { finalize: crate::finish(); }; }",
                "at least one operand",
            ),
            ("layout H { value: bytes(0); }", "must be nonzero"),
            ("layout H { value: bytes(4u8); }", "must be unsuffixed"),
        ] {
            let error = parse_error(source);
            assert!(error.to_string().contains(needle), "{source}: {error}");
        }
    }

    #[test]
    fn rejects_removed_grammar_and_requires_canonical_terms() {
        for source in [
            "layout H { field value: U8; }",
            "layout H { value: U8 { position: 1; }; }",
            "absolute layout H { value: U8; }",
            "layout H { padding { length: 1; } }",
            "layout H { align { boundary: 4; } }",
            "layout H { value: prefix(crate::P); }",
            "layout H { value: bytes(current_pos..current_pos + length); }",
            "layout H { value: bytes(current_pos..buf_end); }",
            "layout H { value: bytes(buf_start..end); }",
        ] {
            assert!(parse_str::<Invocation>(source).is_err(), "{source}");
        }
        assert!(
            parse_error("layout H { value: prefix(crate::P); }")
                .to_string()
                .contains("variable(path)")
        );
        assert!(
            parse_error("absolute layout H { value: U8; }")
                .to_string()
                .contains("name @ offset")
        );
    }

    #[test]
    fn retains_docs_raw_identifiers_and_direct_paths() {
        let parsed: Invocation =
            parse_str("/// layout\npub layout H { #[doc = \"value\"] r#type @ 1: crate::Codec; }")
                .unwrap();
        let Item::Layout(layout) = &parsed.items[0] else {
            panic!("expected layout")
        };
        assert_eq!(layout.docs.len(), 1);
        assert_eq!(layout.fields[0].docs.len(), 1);
        assert_eq!(layout.fields[0].name, "r#type");
        assert!(matches!(layout.fields[0].codec, Codec::Custom(_)));
    }
}
