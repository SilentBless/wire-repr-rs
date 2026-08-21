use wire_repr::{PreparedLayout, Wire};

const SQLITE_MAGIC: [u8; 16] = *b"SQLite format 3\0";

/// The fixed 100-byte SQLite database header.
#[derive(Clone, Debug, Eq, PartialEq, Wire)]
pub struct Header {
    /// The SQLite format marker.
    pub magic: [u8; 16],
    /// The raw database page size.
    #[wire(be)]
    pub page_size: u16,
    /// The file format write version.
    pub write_version: u8,
    /// The file format read version.
    pub read_version: u8,
    /// Reserved bytes at the end of each page.
    pub reserved_space: u8,
    /// The maximum embedded payload fraction.
    pub max_payload_fraction: u8,
    /// The minimum embedded payload fraction.
    pub min_payload_fraction: u8,
    /// The leaf payload fraction.
    pub leaf_payload_fraction: u8,
    /// The file change counter.
    #[wire(be)]
    pub file_change_counter: u32,
    /// The database size in pages.
    #[wire(be)]
    pub database_size_pages: u32,
    /// The first freelist trunk page.
    #[wire(be)]
    pub first_freelist_trunk_page: u32,
    /// The total freelist page count.
    #[wire(be)]
    pub freelist_page_count: u32,
    /// The schema cookie.
    #[wire(be)]
    pub schema_cookie: u32,
    /// The schema format number.
    #[wire(be)]
    pub schema_format_number: u32,
    /// The raw suggested cache size.
    #[wire(be)]
    pub suggested_cache_size: u32,
    /// The largest root b-tree page number.
    #[wire(be)]
    pub largest_root_btree_page: u32,
    /// The database text encoding.
    #[wire(be)]
    pub text_encoding: u32,
    /// The user version.
    #[wire(be)]
    pub user_version: u32,
    /// The incremental-vacuum mode.
    #[wire(be)]
    pub incremental_vacuum_mode: u32,
    /// The application identifier.
    #[wire(be)]
    pub application_id: u32,
    /// The space reserved for future SQLite expansion.
    pub reserved_expansion: [u8; 20],
    /// The change counter valid for the SQLite version.
    #[wire(be)]
    pub version_valid_for: u32,
    /// The SQLite library version number.
    #[wire(be)]
    pub sqlite_version_number: u32,
}

/// SQLite-specific header semantics layered on top of the fixed wire layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderSemanticError {
    WrongMagic,
    NonZeroReservedExpansion,
}

fn validate_sqlite_header(header: &HeaderView<'_>) -> Result<(), HeaderSemanticError> {
    if header.magic() != &SQLITE_MAGIC {
        Err(HeaderSemanticError::WrongMagic)
    } else if header.reserved_expansion().iter().any(|byte| *byte != 0) {
        Err(HeaderSemanticError::NonZeroReservedExpansion)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageSizeError {
    InvalidRaw(u16),
}

// SQLite's page-size encoding is semantic consumer validation, not layout validation.
fn page_size_from_raw(raw: u16) -> Result<u32, PageSizeError> {
    if raw == 1 {
        Ok(65_536)
    } else if (512..=32_768).contains(&raw) && raw.is_power_of_two() {
        Ok(u32::from(raw))
    } else {
        Err(PageSizeError::InvalidRaw(raw))
    }
}

fn write_be_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn write_be_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn sqlite_header() -> Vec<u8> {
    let mut bytes = vec![0xa5; 100];
    bytes[0..16].copy_from_slice(&SQLITE_MAGIC);
    write_be_u16(&mut bytes, 16, 4_096);
    bytes[18] = 1;
    bytes[19] = 1;
    bytes[20] = 32;
    bytes[21] = 64;
    bytes[22] = 32;
    bytes[23] = 32;
    write_be_u32(&mut bytes, 24, 0x0102_0304);
    write_be_u32(&mut bytes, 28, 0x0000_0102);
    write_be_u32(&mut bytes, 32, 0x0000_0203);
    write_be_u32(&mut bytes, 36, 0x0000_0304);
    write_be_u32(&mut bytes, 40, 0x1122_3344);
    write_be_u32(&mut bytes, 44, 4);
    write_be_u32(&mut bytes, 48, 0xffff_ff80);
    write_be_u32(&mut bytes, 52, 0x0000_0506);
    write_be_u32(&mut bytes, 56, 1);
    write_be_u32(&mut bytes, 60, 0x5566_7788);
    write_be_u32(&mut bytes, 64, 1);
    write_be_u32(&mut bytes, 68, 0x0a0b_0c0d);
    bytes[72..92].fill(0);
    write_be_u32(&mut bytes, 92, 0x99aa_bbcc);
    write_be_u32(&mut bytes, 96, 3_045_000);
    bytes
}

fn semantic_header() -> Header {
    Header {
        magic: SQLITE_MAGIC,
        page_size: 4_096,
        write_version: 1,
        read_version: 1,
        reserved_space: 32,
        max_payload_fraction: 64,
        min_payload_fraction: 32,
        leaf_payload_fraction: 32,
        file_change_counter: 0x0102_0304,
        database_size_pages: 0x0000_0102,
        first_freelist_trunk_page: 0x0000_0203,
        freelist_page_count: 0x0000_0304,
        schema_cookie: 0x1122_3344,
        schema_format_number: 4,
        suggested_cache_size: 0xffff_ff80,
        largest_root_btree_page: 0x0000_0506,
        text_encoding: 1,
        user_version: 0x5566_7788,
        incremental_vacuum_mode: 1,
        application_id: 0x0a0b_0c0d,
        reserved_expansion: [0; 20],
        version_valid_for: 0x99aa_bbcc,
        sqlite_version_number: 3_045_000,
    }
}

#[test]
fn real_header_parses_as_an_exact_semantic_value_with_consumer_checks_and_a_disjoint_suffix() {
    let header = sqlite_header();
    let suffix = [0xde, 0xad, 0xbe, 0xef];
    let mut input = header.clone();
    input.extend_from_slice(&suffix);

    let (parsed, parsed_suffix) = Header::view(&input)
        .with_remainder()
        .expect("real header parses");
    assert_eq!(parsed.as_bytes(), header.as_slice());
    assert_eq!(parsed_suffix, suffix);
    assert_eq!(parsed.as_bytes().as_ptr(), input.as_ptr());
    assert_eq!(parsed_suffix.as_ptr(), input[100..].as_ptr());
    assert_eq!(parsed.magic(), &SQLITE_MAGIC);
    assert_eq!(parsed.reserved_expansion(), &[0; 20]);
    assert_eq!(parsed.page_size(), 4_096);
    assert_eq!(parsed.write_version(), 1);
    assert_eq!(parsed.max_payload_fraction(), 64);
    assert_eq!(parsed.file_change_counter(), 0x0102_0304);
    assert_eq!(parsed.schema_cookie(), 0x1122_3344);
    assert_eq!(parsed.suggested_cache_size(), 0xffff_ff80);
    assert_eq!(parsed.application_id(), 0x0a0b_0c0d);
    assert_eq!(parsed.version_valid_for(), 0x99aa_bbcc);
    assert_eq!(parsed.sqlite_version_number(), 3_045_000);
    assert_eq!(validate_sqlite_header(&parsed), Ok(()));

    assert!(matches!(
        Header::view(&input).without_trailing(),
        Err(HeaderDecodeError::TrailingBytes {
            expected: 100,
            actual: 104
        })
    ));
    assert!(matches!(
        Header::view(&header[..99]).without_trailing(),
        Err(HeaderDecodeError::InputTooShort {
            field: "sqlite_version_number",
            required: 4,
            available: 3
        })
    ));
}

#[test]
fn consumer_semantics_reject_wrong_magic_after_structural_parse() {
    let mut bytes = sqlite_header();
    bytes[0] = b'X';

    let parsed = Header::view(&bytes)
        .without_trailing()
        .expect("all fixed-layout bytes parse");
    assert_eq!(parsed.magic(), b"XQLite format 3\0");
    assert_eq!(
        validate_sqlite_header(&parsed),
        Err(HeaderSemanticError::WrongMagic)
    );
}

#[test]
fn consumer_semantics_reject_nonzero_reserved_expansion_after_structural_parse() {
    let mut bytes = sqlite_header();
    bytes[80] = 1;

    let parsed = Header::view(&bytes)
        .without_trailing()
        .expect("all fixed-layout bytes parse");
    assert_eq!(parsed.reserved_expansion().as_slice(), &bytes[72..92]);
    assert_eq!(
        validate_sqlite_header(&parsed),
        Err(HeaderSemanticError::NonZeroReservedExpansion)
    );
}

#[test]
fn sqlite_page_size_semantics_remain_handwritten_consumer_logic() {
    assert_eq!(page_size_from_raw(1), Ok(65_536));
    assert_eq!(page_size_from_raw(4_096), Ok(4_096));
    for raw in [0, 511, 513, 32_769] {
        assert_eq!(page_size_from_raw(raw), Err(PageSizeError::InvalidRaw(raw)));
    }

    let mut structurally_valid = sqlite_header();
    write_be_u16(&mut structurally_valid, 16, 513);
    let parsed = Header::view(&structurally_valid)
        .without_trailing()
        .expect("layout parsing does not impose SQLite page-size semantics");
    assert_eq!(parsed.page_size(), 513);
    assert_eq!(
        page_size_from_raw(parsed.page_size()),
        Err(PageSizeError::InvalidRaw(513))
    );
}

#[test]
fn prepared_encoding_writes_the_canonical_header_without_touching_the_suffix() {
    let expected = sqlite_header();
    let plan = semantic_header().prepare().expect("header prepares");
    assert_eq!(plan.encoded_len(), 100);

    let mut output = vec![0x5a; 104];
    let suffix_before = output[100..].to_vec();
    let (written, suffix) = plan.commit_into(&mut output).expect("header commits");
    assert_eq!(written.as_bytes(), expected.as_slice());
    assert_eq!(&*suffix, suffix_before.as_slice());
    assert_eq!(written.as_bytes()[72..92], [0; 20]);
    assert_eq!(output[100..], suffix_before);
}

#[test]
fn short_prepared_output_is_unchanged() {
    let plan = semantic_header().prepare().expect("header prepares");
    let initial = [0x3c; 99];
    let mut output = initial;
    assert!(plan.commit_into(&mut output).is_err());
    assert_eq!(output, initial);
}
