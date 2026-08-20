use wire_repr::{ExactWidthError, wire_repr};

const SQLITE_MAGIC: [u8; 16] = *b"SQLite format 3\0";

wire_repr! {
    /// The fixed 100-byte SQLite database header.
    pub absolute layout Header {
        /// The SQLite format marker.
        field magic: bytes(16) { offset: 0; }
        /// The raw database page size.
        field page_size: BeU16 { offset: 16; }
        /// The file format write version.
        field write_version: U8 { offset: 18; }
        /// The file format read version.
        field read_version: U8 { offset: 19; }
        /// Reserved bytes at the end of each page.
        field reserved_space: U8 { offset: 20; }
        /// The maximum embedded payload fraction.
        field max_payload_fraction: U8 { offset: 21; }
        /// The minimum embedded payload fraction.
        field min_payload_fraction: U8 { offset: 22; }
        /// The leaf payload fraction.
        field leaf_payload_fraction: U8 { offset: 23; }
        /// The file change counter.
        field file_change_counter: BeU32 { offset: 24; }
        /// The database size in pages.
        field database_size_pages: BeU32 { offset: 28; }
        /// The first freelist trunk page.
        field first_freelist_trunk_page: BeU32 { offset: 32; }
        /// The total freelist page count.
        field freelist_page_count: BeU32 { offset: 36; }
        /// The schema cookie.
        field schema_cookie: BeU32 { offset: 40; }
        /// The schema format number.
        field schema_format_number: BeU32 { offset: 44; }
        /// The raw suggested cache size.
        field suggested_cache_size: BeU32 { offset: 48; }
        /// The largest root b-tree page number.
        field largest_root_btree_page: BeU32 { offset: 52; }
        /// The database text encoding.
        field text_encoding: BeU32 { offset: 56; }
        /// The user version.
        field user_version: BeU32 { offset: 60; }
        /// The incremental-vacuum mode.
        field incremental_vacuum_mode: BeU32 { offset: 64; }
        /// The application identifier.
        field application_id: BeU32 { offset: 68; }
        /// The space reserved for future SQLite expansion.
        field reserved_expansion: bytes(20) { offset: 72; }
        /// The change counter valid for the SQLite version.
        field version_valid_for: BeU32 { offset: 92; }
        /// The SQLite library version number.
        field sqlite_version_number: BeU32 { offset: 96; }
    }
}

/// SQLite-specific header semantics layered on top of the fixed wire layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderSemanticError {
    WrongMagic,
    NonZeroReservedExpansion,
}

fn validate_sqlite_header(view: &Header<'_>) -> Result<(), HeaderSemanticError> {
    if view.magic() != SQLITE_MAGIC {
        Err(HeaderSemanticError::WrongMagic)
    } else if view.reserved_expansion().iter().any(|byte| *byte != 0) {
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

fn read_be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
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

#[test]
fn real_header_parses_as_an_exact_absolute_view_with_consumer_semantics_and_a_disjoint_suffix() {
    let header = sqlite_header();
    let suffix = [0xde, 0xad, 0xbe, 0xef];
    let mut input = header.clone();
    input.extend_from_slice(&suffix);

    let (view, parsed_suffix) = Header::view(&input)
        .with_remainder()
        .expect("real header parses");
    assert_eq!(Header::WIDTH, 100);
    assert_eq!(view.as_bytes(), header.as_slice());
    assert_eq!(parsed_suffix, suffix);
    assert_eq!(view.as_bytes().as_ptr(), input.as_ptr());
    assert_eq!(parsed_suffix.as_ptr(), input[100..].as_ptr());
    assert_eq!(view.magic(), SQLITE_MAGIC.as_slice());
    assert_eq!(view.reserved_expansion(), [0; 20]);
    assert_eq!(validate_sqlite_header(&view), Ok(()));
    assert_eq!(view.page_size(), read_be_u16(&header, 16));
    assert_eq!(view.write_version(), header[18]);
    assert_eq!(view.max_payload_fraction(), header[21]);
    assert_eq!(view.file_change_counter(), read_be_u32(&header, 24));
    assert_eq!(view.schema_cookie(), read_be_u32(&header, 40));
    assert_eq!(view.suggested_cache_size(), read_be_u32(&header, 48));
    assert_eq!(view.application_id(), read_be_u32(&header, 68));
    assert_eq!(view.version_valid_for(), read_be_u32(&header, 92));
    assert_eq!(view.sqlite_version_number(), read_be_u32(&header, 96));

    assert!(matches!(
        Header::view(&input).without_trailing(),
        Err(HeaderError::TrailingBytes {
            expected: 100,
            actual: 104
        })
    ));
    assert!(matches!(
        Header::view(&header[..99]).without_trailing(),
        Err(HeaderError::InputTooShort {
            expected: 100,
            actual: 99
        })
    ));
}

#[test]
fn consumer_semantics_reject_wrong_magic_after_structural_parse() {
    let mut bytes = sqlite_header();
    bytes[0] = b'X';

    let view = Header::view(&bytes)
        .without_trailing()
        .expect("all exact-layout bytes parse");
    assert_eq!(view.magic(), &bytes[0..16]);
    assert_eq!(
        validate_sqlite_header(&view),
        Err(HeaderSemanticError::WrongMagic)
    );
}

#[test]
fn consumer_semantics_reject_nonzero_reserved_expansion_after_structural_parse() {
    let mut bytes = sqlite_header();
    bytes[80] = 1;

    let view = Header::view(&bytes)
        .without_trailing()
        .expect("all exact-layout bytes parse");
    assert_eq!(view.reserved_expansion(), &bytes[72..92]);
    assert_eq!(
        validate_sqlite_header(&view),
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
    let view = Header::view(&structurally_valid)
        .without_trailing()
        .expect("layout parsing does not impose SQLite page-size semantics");
    assert_eq!(view.page_size(), 513);
    assert_eq!(
        page_size_from_raw(view.page_size()),
        Err(PageSizeError::InvalidRaw(513))
    );
}

#[test]
fn mutable_views_change_only_declared_field_spans() {
    let mut bytes = sqlite_header();
    let before = bytes.clone();
    let mut view = HeaderViewMut::parse_exact_mut(&mut bytes).expect("real mutable header parses");
    view.set_page_size(8_192).expect("built-in plan succeeds");
    view.set_schema_cookie(0xaabb_ccdd)
        .expect("built-in plan succeeds");
    view.set_sqlite_version_number(3_046_000)
        .expect("built-in plan succeeds");
    assert_eq!(view.page_size(), 8_192);
    assert_eq!(view.schema_cookie(), 0xaabb_ccdd);
    assert_eq!(view.sqlite_version_number(), 3_046_000);

    for index in 0..bytes.len() {
        let changed =
            (16..18).contains(&index) || (40..44).contains(&index) || (96..100).contains(&index);
        if !changed {
            assert_eq!(
                bytes[index], before[index],
                "unexpected write at byte {index}"
            );
        }
    }
    assert_eq!(bytes[72..92], before[72..92]);
}

#[test]
fn builder_writes_a_complete_header_without_touching_the_suffix() {
    let mut output = vec![0x5a; 104];
    let suffix_before = output[100..].to_vec();
    let mut expected = sqlite_header();
    let (mut view, suffix) = HeaderBuilder::new()
        .magic(&SQLITE_MAGIC)
        .page_size(4_096)
        .write_version(1)
        .read_version(1)
        .reserved_space(32)
        .max_payload_fraction(64)
        .min_payload_fraction(32)
        .leaf_payload_fraction(32)
        .file_change_counter(0x0102_0304)
        .database_size_pages(0x0000_0102)
        .first_freelist_trunk_page(0x0000_0203)
        .freelist_page_count(0x0000_0304)
        .schema_cookie(0x1122_3344)
        .schema_format_number(4)
        .suggested_cache_size(0xffff_ff80)
        .largest_root_btree_page(0x0000_0506)
        .text_encoding(1)
        .user_version(0x5566_7788)
        .incremental_vacuum_mode(1)
        .application_id(0x0a0b_0c0d)
        .reserved_expansion(&[0; 20])
        .version_valid_for(0x99aa_bbcc)
        .sqlite_version_number(3_045_000)
        .build_into(&mut output)
        .expect("complete SQLite header builder");
    assert_eq!(view.as_bytes().len(), 100);
    assert_eq!(view.as_bytes(), expected.as_slice());
    assert_eq!(&*suffix, suffix_before.as_slice());
    assert_eq!(view.page_size(), 4_096);
    assert_eq!(view.application_id(), 0x0a0b_0c0d);
    assert_eq!(view.sqlite_version_number(), 3_045_000);
    view.set_user_version(7)
        .expect("built view remains mutable");
    expected[60..64].copy_from_slice(&7_u32.to_be_bytes());
    assert_eq!(view.as_bytes(), expected.as_slice());
    assert_eq!(output[72..92], [0; 20]);
    assert_eq!(output[100..], suffix_before);
    assert_eq!(read_be_u32(&output, 60), 7);
}

#[test]
fn wrong_width_builder_bytes_leave_output_unchanged() {
    let mut output = [0x3c; 100];
    let before = output;
    assert!(matches!(
        HeaderBuilder::new()
            .magic(&SQLITE_MAGIC[..15])
            .page_size(4_096)
            .write_version(1)
            .read_version(1)
            .reserved_space(32)
            .max_payload_fraction(64)
            .min_payload_fraction(32)
            .leaf_payload_fraction(32)
            .file_change_counter(1)
            .database_size_pages(2)
            .first_freelist_trunk_page(3)
            .freelist_page_count(4)
            .schema_cookie(5)
            .schema_format_number(4)
            .suggested_cache_size(6)
            .largest_root_btree_page(7)
            .text_encoding(1)
            .user_version(8)
            .incremental_vacuum_mode(0)
            .application_id(9)
            .reserved_expansion(&[0; 20])
            .version_valid_for(10)
            .sqlite_version_number(3_045_000)
            .build_into(&mut output),
        Err(HeaderWriteError::FieldMagic(error))
            if error == ExactWidthError::new(16, 15)
    ));
    assert_eq!(output, before);
}

#[test]
fn short_builder_output_is_unchanged() {
    let mut output = [0x3c; 99];
    assert!(matches!(
        HeaderBuilder::new()
            .magic(&SQLITE_MAGIC)
            .page_size(4_096)
            .write_version(1)
            .read_version(1)
            .reserved_space(32)
            .max_payload_fraction(64)
            .min_payload_fraction(32)
            .leaf_payload_fraction(32)
            .file_change_counter(1)
            .database_size_pages(2)
            .first_freelist_trunk_page(3)
            .freelist_page_count(4)
            .schema_cookie(5)
            .schema_format_number(4)
            .suggested_cache_size(6)
            .largest_root_btree_page(7)
            .text_encoding(1)
            .user_version(8)
            .incremental_vacuum_mode(0)
            .application_id(9)
            .reserved_expansion(&[0; 20])
            .version_valid_for(10)
            .sqlite_version_number(3_045_000)
            .build_into(&mut output),
        Err(HeaderWriteError::OutputTooShort {
            needed: 100,
            available: 99
        })
    ));
    assert_eq!(output, [0x3c; 99]);
}
