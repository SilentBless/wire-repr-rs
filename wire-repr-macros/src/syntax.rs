//! Exact token grammar for canonical `wire_repr!` input.

use syn::{
    Attribute, Error, Ident, LitInt, Meta, Path, Result, Token, Visibility, braced,
    parse::{Parse, ParseStream},
};

mod keyword {
    syn::custom_keyword!(absolute);
    syn::custom_keyword!(align);
    syn::custom_keyword!(bytes);
    syn::custom_keyword!(padding);
    syn::custom_keyword!(prefix);
    syn::custom_keyword!(region);
    syn::custom_keyword!(projections);
    syn::custom_keyword!(bit);
    syn::custom_keyword!(bits);
}

/// Parsed invocation before semantic normalization.
pub(crate) struct Invocation {
    pub(crate) layouts: Vec<Layout>,
}

/// Parsed layout declaration.
pub(crate) struct Layout {
    pub(crate) docs: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) kind: LayoutKind,
    pub(crate) name: Ident,
    pub(crate) fields: Vec<Field>,
    pub(crate) physical: Vec<PhysicalEntry>,
}

/// A parsed physical sequential-layout entry.
pub(crate) enum PhysicalEntry {
    Field(usize),
    Padding { position: LitInt, length: LitInt },
    Alignment { position: LitInt, boundary: LitInt },
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
    pub(crate) placement: LitInt,
    pub(crate) projections: Vec<Projection>,
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
    /// A custom prefix codec path.
    Prefix(Path),
    /// An opaque byte region bounded by a named field value.
    Region(Ident),
}

impl Parse for Invocation {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut layouts = Vec::new();
        while !input.is_empty() {
            layouts.push(parse_layout(input)?);
        }
        Ok(Self { layouts })
    }
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
    let mut fields = Vec::new();
    let mut physical = Vec::new();
    while !content.is_empty() {
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

    Ok(Layout {
        docs,
        visibility,
        kind,
        name,
        fields,
        physical,
    })
}

fn parse_padding(input: ParseStream<'_>) -> Result<PhysicalEntry> {
    input.parse::<keyword::padding>()?;
    let content;
    braced!(content in input);
    let position = parse_entry_property(&content, "position")?;
    let length = parse_entry_property(&content, "length")?;
    if !content.is_empty() {
        return Err(Error::new(
            content.span(),
            "unexpected tokens after `length`",
        ));
    }
    Ok(PhysicalEntry::Padding { position, length })
}

fn parse_alignment(input: ParseStream<'_>) -> Result<PhysicalEntry> {
    input.parse::<keyword::align>()?;
    let content;
    braced!(content in input);
    let position = parse_entry_property(&content, "position")?;
    let boundary = parse_entry_property(&content, "boundary")?;
    if !content.is_empty() {
        return Err(Error::new(
            content.span(),
            "unexpected tokens after `boundary`",
        ));
    }
    Ok(PhysicalEntry::Alignment { position, boundary })
}

fn parse_entry_property(input: ParseStream<'_>, expected: &str) -> Result<LitInt> {
    let name: Ident = input
        .parse()
        .map_err(|_| Error::new(input.span(), format!("expected `{expected}`")))?;
    if name != expected {
        return Err(Error::new(name.span(), format!("expected `{expected}`")));
    }
    input.parse::<Token![:]>()?;
    let value = input.parse()?;
    input.parse::<Token![;]>()?;
    Ok(value)
}

fn parse_field(input: ParseStream<'_>, kind: LayoutKind) -> Result<Field> {
    let docs = parse_docs(input)?;
    let keyword: Ident = input
        .parse()
        .map_err(|_| Error::new(input.span(), "expected `field`"))?;
    if keyword != "field" {
        return Err(Error::new(keyword.span(), "expected `field`"));
    }

    let name = input.parse()?;
    input.parse::<Token![:]>()?;
    let codec = parse_codec(input)?;
    let content;
    braced!(content in input);
    let placement_name: Ident = content.parse().map_err(|_| {
        Error::new(
            content.span(),
            match kind {
                LayoutKind::Sequential => "expected `position`",
                LayoutKind::Absolute => "expected `offset`",
            },
        )
    })?;
    let expected = match kind {
        LayoutKind::Sequential => "position",
        LayoutKind::Absolute => "offset",
    };
    if placement_name != expected {
        let message = match (kind, placement_name.to_string().as_str()) {
            (LayoutKind::Sequential, "offset") => {
                "`offset` is unsupported in fixed sequential layouts; use `position`"
            }
            (LayoutKind::Absolute, "position") => {
                "`position` is unsupported in fixed absolute layouts; use `offset`"
            }
            (LayoutKind::Sequential, "bits" | "region" | "align" | "padding") => {
                "this is unsupported in fixed sequential layouts; use `position`"
            }
            (LayoutKind::Absolute, "bits" | "region" | "align" | "padding") => {
                "this is unsupported in fixed absolute layouts; use `offset`"
            }
            _ => match kind {
                LayoutKind::Sequential => "expected `position`",
                LayoutKind::Absolute => "expected `offset`",
            },
        };
        return Err(Error::new(placement_name.span(), message));
    }

    content.parse::<Token![:]>()?;
    let placement = content.parse::<LitInt>()?;
    content.parse::<Token![;]>()?;
    let mut projections = Vec::new();
    if content.peek(keyword::projections) {
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
    }
    if !content.is_empty() {
        return Err(Error::new(
            content.span(),
            format!("unexpected tokens after `{expected}` or `projections`"),
        ));
    }
    Ok(Field {
        docs,
        name,
        codec,
        placement,
        projections,
    })
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
        return Err(Error::new(
            input.span(),
            "bare codec paths are not supported; use `codec(path::ToCodec)`",
        ));
    }
    if input.peek(keyword::bytes) {
        input.parse::<keyword::bytes>()?;
        let content;
        syn::parenthesized!(content in input);
        let literal = content
            .parse::<LitInt>()
            .map_err(|_| Error::new(content.span(), "expected an unsuffixed base-10 byte width"))?;
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
    if input.peek(keyword::prefix) {
        input.parse::<keyword::prefix>()?;
        let content;
        syn::parenthesized!(content in input);
        let path = content.parse::<Path>()?;
        if !content.is_empty() {
            return Err(Error::new(content.span(), "expected one prefix codec path"));
        }
        return Ok(Codec::Prefix(path));
    }
    if input.peek(keyword::region) {
        input.parse::<keyword::region>()?;
        let content;
        syn::parenthesized!(content in input);
        let source = content.parse::<Ident>()?;
        if !content.is_empty() {
            return Err(Error::new(
                content.span(),
                "expected one region length field identifier",
            ));
        }
        return Ok(Codec::Region(source));
    }
    let first: Ident = input.parse().map_err(|_| {
        Error::new(
            input.span(),
            "expected a builtin codec, `codec(path)`, `bytes(N)`, `prefix(path)`, or `region(field)`",
        )
    })?;
    if first == "codec" {
        let content;
        syn::parenthesized!(content in input);
        let path = content.parse::<Path>()?;
        if !content.is_empty() {
            return Err(Error::new(content.span(), "expected one codec path"));
        }
        return Ok(Codec::Custom(path));
    }
    if input.peek(Token![::]) {
        return Err(Error::new(
            first.span(),
            "bare codec paths are not supported; use `codec(path::ToCodec)`",
        ));
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
    fn accepts_absolute_after_visibility_and_with_sequential_layouts() {
        for source in [
            "/// packet\nabsolute layout Private { #[doc = \"kind\"] field kind: U8 { offset: 0; } field code: codec(crate::Code) { offset: 2; } }",
            "pub absolute layout Public { field value: U8 { offset: 0; } }",
            "pub(crate) absolute layout Restricted { field value: U8 { offset: 0; } }",
            "pub layout Tail { field end: LeU16 { position: 1; } }",
        ] {
            if let Err(error) = parse_str::<Invocation>(source) {
                panic!("failed to parse `{source}`: {error}");
            }
        }

        let parsed: Invocation = parse_str("/// packet\nabsolute layout Private { #[doc = \"kind\"] field kind: U8 { offset: 0; } field code: codec(crate::Code) { offset: 2; } } pub absolute layout Public { field value: U8 { offset: 0; } } pub(crate) absolute layout Restricted { field value: U8 { offset: 0; } } layout Tail { field end: LeU16 { position: 1; } }").unwrap();
        assert_eq!(parsed.layouts.len(), 4);
        assert_eq!(parsed.layouts[0].kind, LayoutKind::Absolute);
        assert!(matches!(
            &parsed.layouts[0].visibility,
            Visibility::Inherited
        ));
        assert!(matches!(
            &parsed.layouts[1].visibility,
            Visibility::Public(_)
        ));
        assert!(matches!(
            &parsed.layouts[2].visibility,
            Visibility::Restricted(_)
        ));
        assert_eq!(parsed.layouts[0].fields[0].placement.base10_digits(), "0");
        assert_eq!(parsed.layouts[3].kind, LayoutKind::Sequential);
    }

    #[test]
    fn placement_diagnostics_are_mode_specific() {
        for (source, needle) in [
            (
                "layout H { field f: U8 { offset: 1; } }",
                "unsupported in fixed sequential",
            ),
            (
                "absolute layout H { field f: U8 { position: 1; } }",
                "unsupported in fixed absolute",
            ),
        ] {
            assert!(parse_error(source).to_string().contains(needle));
        }
    }

    #[test]
    fn accepts_docs_visibility_custom_paths_and_multiple_sequential_layouts() {
        let parsed: Invocation = parse_str(
            "/// header\npub(crate) layout Header { #[doc = \"kind\"] field kind: codec(crate::Code) { position: 2; } field first: U8 { position: 1; } } layout Tail { field end: LeU16 { position: 1; } }",
        )
        .unwrap();

        assert_eq!(parsed.layouts.len(), 2);
        assert_eq!(parsed.layouts[0].docs.len(), 1);
        assert!(matches!(
            &parsed.layouts[0].visibility,
            Visibility::Restricted(_)
        ));
        assert_eq!(parsed.layouts[0].fields[0].docs.len(), 1);
        assert!(matches!(
            &parsed.layouts[0].fields[0].codec,
            Codec::Custom(path) if path.segments.len() == 2
        ));
    }

    #[test]
    fn local_syntax_diagnostics_remain_targeted() {
        for (source, needle) in [
            ("struct Header {}", "`struct`"),
            ("layout H { field f U8 { position: 1; } }", "expected `:`"),
            (
                "layout H { field f: { position: 1; } }",
                "expected a builtin codec",
            ),
            (
                "layout H { field f: crate::Code { position: 1; } }",
                "use `codec(path::ToCodec)`",
            ),
            (
                "layout H { field f: Code::Nested { position: 1; } }",
                "use `codec(path::ToCodec)`",
            ),
            ("layout H { field f: U8 { position: 1 } }", "expected `;`"),
            (
                "#[derive(Clone)] layout H { field f: U8 { position: 1; } }",
                "only outer documentation",
            ),
            (
                "layout H { #[cfg(test)] field f: U8 { position: 1; } }",
                "only outer documentation",
            ),
            (
                "layout H { field f: U8 position: 1; }",
                "expected curly braces",
            ),
            ("layout H { field f: U8 { : 1; } }", "expected `position`"),
        ] {
            let error = parse_error(source);
            assert!(error.to_string().contains(needle), "{source}: {error}");
        }
    }

    #[test]
    fn accepts_nested_projection_grammar_and_rejects_empty_or_unknown_forms() {
        let parsed: Invocation = parse_str("layout H { field flags: U8 { position: 1; projections { #[doc = \"enabled\"] bit r#type: 0; bits mode: 1..=3; } } } absolute layout A { field flags: LeU16 { offset: 0; projections { bit low: 0; } } }").unwrap();
        assert_eq!(parsed.layouts[0].fields[0].projections.len(), 2);
        assert_eq!(
            parsed.layouts[0].fields[0].projections[0].name.to_string(),
            "r#type"
        );
        for (source, needle) in [
            (
                "layout H { field f: U8 { position: 1; projections { } } }",
                "must contain",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { byte x: 0; } } }",
                "expected `bit` or `bits`",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { bits x: 0..1; } } }",
                "inclusive",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { bit x 0; } } }",
                "expected `:`",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { bit x: 0 } } }",
                "expected `;`",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { #[cfg(test)] bit x: 0; } } }",
                "only outer documentation",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { bit x: 0; } projections { bit y: 1; } } }",
                "unexpected tokens",
            ),
            (
                "layout H { field f: U8 { position: 1; projections { bit x: 0; } bit y: 1; } }",
                "unexpected tokens",
            ),
        ] {
            assert!(parse_error(source).to_string().contains(needle));
        }
    }

    #[test]
    fn reserved_field_forms_are_rejected_in_each_fixed_mode() {
        for (source, needle) in [
            (
                "layout H { field f: U8 { bits: 1; } }",
                "unsupported in fixed sequential",
            ),
            (
                "layout H { field f: U8 { region: 1; } }",
                "unsupported in fixed sequential",
            ),
            (
                "absolute layout H { field f: U8 { align: 1; } }",
                "unsupported in fixed absolute",
            ),
            (
                "absolute layout H { field f: U8 { padding: 1; } }",
                "unsupported in fixed absolute",
            ),
        ] {
            let error = parse_error(source);
            assert!(error.to_string().contains(needle), "{source}: {error}");
        }
    }

    #[test]
    fn parses_custom_prefix_codecs_and_rejects_malformed_forms() {
        let parsed: Invocation =
            parse_str("layout H { field value: prefix(crate::Terminated) { position: 1; } }")
                .unwrap();
        assert!(matches!(
            &parsed.layouts[0].fields[0].codec,
            Codec::Prefix(path) if path.segments.len() == 2
        ));

        for source in [
            "layout H { field value: prefix() { position: 1; } }",
            "layout H { field value: prefix(crate::A crate::B) { position: 1; } }",
        ] {
            assert!(parse_str::<Invocation>(source).is_err(), "{source}");
        }
    }
    #[test]
    fn parses_byte_widths_and_rejects_invalid_forms() {
        let parsed: Invocation = parse_str(
            "layout Sequential { field value: bytes(16) { position: 1; } } absolute layout Absolute { field value: bytes(16) { offset: 0; } }",
        ).unwrap();
        assert!(matches!(
            parsed.layouts[0].fields[0].codec,
            Codec::Bytes(16)
        ));
        assert!(matches!(
            parsed.layouts[1].fields[0].codec,
            Codec::Bytes(16)
        ));

        for source in [
            "layout H { field value: bytes() { position: 1; } }",
            "layout H { field value: bytes(0) { position: 1; } }",
            "layout H { field value: bytes(16u8) { position: 1; } }",
            "layout H { field value: bytes(0x10) { position: 1; } }",
            "layout H { field value: bytes(16 17) { position: 1; } }",
        ] {
            assert!(parse_str::<Invocation>(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_region_sources_and_rejects_malformed_forms() {
        let parsed: Invocation = parse_str(
            "layout H { field payload: region(length) { position: 2; } field length: U8 { position: 1; } }",
        )
        .unwrap();
        assert!(matches!(
            &parsed.layouts[0].fields[0].codec,
            Codec::Region(source) if source == "length"
        ));

        for source in [
            "layout H { field payload: region() { position: 1; } }",
            "layout H { field payload: region(crate::length) { position: 1; } }",
            "layout H { field payload: region(length extra) { position: 1; } }",
        ] {
            assert!(parse_str::<Invocation>(source).is_err(), "{source}");
        }
    }

    #[test]
    fn parses_padding_and_alignment_entries_with_targeted_grammar_errors() {
        let parsed: Invocation = parse_str(
            "layout H { align { position: 3; boundary: 8; } field tail: U8 { position: 4; } padding { position: 2; length: 3; } field head: U8 { position: 1; } }",
        )
        .unwrap();
        let physical = &parsed.layouts[0].physical;
        assert_eq!(physical.len(), 4);
        assert!(matches!(
            &physical[0],
            PhysicalEntry::Alignment { position, boundary }
                if position.base10_digits() == "3" && boundary.base10_digits() == "8"
        ));
        assert!(matches!(&physical[1], PhysicalEntry::Field(0)));
        assert!(matches!(
            &physical[2],
            PhysicalEntry::Padding { position, length }
                if position.base10_digits() == "2" && length.base10_digits() == "3"
        ));
        assert!(matches!(&physical[3], PhysicalEntry::Field(1)));

        for (source, needle) in [
            (
                "layout H { field a: U8 { position: 1; } padding { length: 3; position: 2; } }",
                "expected `position`",
            ),
            (
                "layout H { field a: U8 { position: 1; } padding { position: 2; bytes: 3; } }",
                "expected `length`",
            ),
            (
                "layout H { field a: U8 { position: 1; } padding { position: 2; length: 3; extra: 1; } }",
                "unexpected tokens",
            ),
            (
                "layout H { field a: U8 { position: 1; } align { position: 2; size: 4; } }",
                "expected `boundary`",
            ),
            (
                "layout H { field a: U8 { position: 1; } align { position: 2; boundary: 4 } }",
                "expected `;`",
            ),
        ] {
            let error = parse_error(source);
            assert!(error.to_string().contains(needle), "{source}: {error}");
        }
    }
}
