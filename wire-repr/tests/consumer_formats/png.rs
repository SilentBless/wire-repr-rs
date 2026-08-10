use wire_repr::{ExactWidthError, wire_repr};

const IHDR: [u8; 25] = [
    0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4,
    0x89,
];
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82];

/// PNG chunk-length domain validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngLengthError {
    /// The length exceeds PNG's 31-bit limit.
    TooLarge,
}

/// PNG chunk-type domain validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkTypeError {
    /// A chunk-type byte is not an ASCII letter.
    NonAsciiLetter {
        /// The invalid byte's zero-based index.
        index: usize,
        /// The invalid byte.
        byte: u8,
    },
}

fn validate_png_length(length: u32) -> Result<(), PngLengthError> {
    if length > 0x7fff_ffff {
        Err(PngLengthError::TooLarge)
    } else {
        Ok(())
    }
}

fn validate_chunk_type(chunk_type: &[u8]) -> Result<(), ChunkTypeError> {
    for (index, byte) in chunk_type.iter().copied().enumerate() {
        if !byte.is_ascii_alphabetic() {
            return Err(ChunkTypeError::NonAsciiLetter { index, byte });
        }
    }
    Ok(())
}

wire_repr! {
    /// One dynamically sized PNG chunk.
    pub layout PngChunk {
        /// The encoded byte count of `data`.
        field data_length: BeU32 { position: 1; }
        /// The opaque four-byte PNG chunk type.
        field chunk_type: bytes(4) { position: 2; }
        /// The opaque chunk payload.
        field data: region(data_length) { position: 3; }
        /// The stored CRC-32/ISO-HDLC value.
        field crc: BeU32 { position: 4; }
    }
}

fn read_be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn crc32_iso_hdlc(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

fn crc_matches(type_and_data: &[u8], stored_crc: u32) -> bool {
    crc32_iso_hdlc(type_and_data) == stored_crc
}

#[test]
fn ihdr_and_iend_parse_exactly_preserving_raw_fields_and_suffix() {
    let suffix = [0xde, 0xad];
    let mut input = IHDR.to_vec();
    input.extend_from_slice(&suffix);

    let (view, parsed_suffix) = PngChunkView::parse_prefix(&input).expect("IHDR parses");
    assert_eq!(view.as_bytes().len(), 25);
    assert_eq!(view.as_bytes(), IHDR);
    assert_eq!(parsed_suffix, suffix);
    assert_eq!(parsed_suffix.as_ptr(), input[25..].as_ptr());
    assert_eq!(view.data_length(), read_be_u32(&IHDR, 0));
    assert_eq!(view.chunk_type(), &IHDR[4..8]);
    assert_eq!(view.data(), &IHDR[8..21]);
    assert_eq!(view.crc(), read_be_u32(&IHDR, 21));
    assert!(matches!(
        PngChunkView::parse_exact(&input),
        Err(PngChunkError::TrailingBytes {
            expected: 25,
            actual: 27
        })
    ));

    let iend = PngChunkView::parse_exact(&IEND).expect("zero-length IEND parses");
    assert_eq!(iend.data_length(), 0);
    assert_eq!(iend.chunk_type(), b"IEND");
    assert_eq!(iend.data(), []);
    assert_eq!(iend.crc(), 0xae42_6082);
}

#[test]
fn malformed_domains_parse_structurally_then_consumer_checks_reject_them() {
    assert_eq!(
        validate_png_length(0x8000_0000),
        Err(PngLengthError::TooLarge),
        "the domain policy does not allocate or parse a huge payload"
    );

    let mut malformed = IEND;
    malformed[4] = b'1';
    let chunk = PngChunkView::parse_exact(&malformed).expect("raw type is structurally opaque");
    assert_eq!(chunk.chunk_type(), b"1END");
    assert_eq!(
        validate_chunk_type(chunk.chunk_type()),
        Err(ChunkTypeError::NonAsciiLetter {
            index: 0,
            byte: b'1'
        })
    );

    let overclaimed_data = [0, 0, 0, 1, b'I', b'E', b'N', b'D'];
    assert!(matches!(
        PngChunkView::parse_prefix(&overclaimed_data),
        Err(PngChunkError::InputTooShort {
            position: 3,
            expected: 1,
            available: 0
        })
    ));
    assert!(matches!(
        PngChunkView::parse_exact(&IHDR[..20]),
        Err(PngChunkError::InputTooShort {
            position: 3,
            expected: 13,
            available: 12
        })
    ));
}

#[test]
fn crc_stays_consumer_validation_not_layout_validation() {
    assert_eq!(crc32_iso_hdlc(&IHDR[4..21]), 0x1f15_c489);
    assert_eq!(crc32_iso_hdlc(&IEND[4..8]), 0xae42_6082);
    let ihdr = PngChunkView::parse_exact(&IHDR).unwrap();
    let iend = PngChunkView::parse_exact(&IEND).unwrap();
    assert!(crc_matches(&ihdr.as_bytes()[4..21], ihdr.crc()));
    assert!(crc_matches(&iend.as_bytes()[4..8], iend.crc()));

    let mut wrong_crc = IHDR;
    wrong_crc[24] ^= 1;
    let view = PngChunkView::parse_exact(&wrong_crc).expect("CRC bytes are structurally opaque");
    assert!(!crc_matches(&view.as_bytes()[4..21], view.crc()));
}

#[test]
fn mutable_chunk_setters_touch_only_their_fields() {
    let suffix = [0xa5, 0x5a];
    let mut bytes = IHDR.to_vec();
    bytes.extend_from_slice(&suffix);
    let before = bytes.clone();
    let (mut view, parsed_suffix) = PngChunkViewMut::parse_prefix_mut(&mut bytes).unwrap();
    view.set_chunk_type(b"tEXt").unwrap();
    view.set_crc(0x1122_3344).unwrap();
    assert_eq!(view.chunk_type(), b"tEXt");
    assert_eq!(view.crc(), 0x1122_3344);
    for (index, (actual, expected)) in view.as_bytes().iter().zip(before.iter()).enumerate() {
        let changed = (4..8).contains(&index) || (21..25).contains(&index);
        if !changed {
            assert_eq!(actual, expected, "unexpected write at byte {index}");
        }
    }
    assert_eq!(&view.as_bytes()[8..21], &before[8..21]);
    assert_eq!(&*parsed_suffix, suffix);
}

#[test]
fn dynamic_builder_derives_length_and_is_atomic() {
    let suffix = [0x5a, 0xa5];
    let mut output = [0xcc; 27];
    output[25..].copy_from_slice(&suffix);
    let (mut view, parsed_suffix) = PngChunkBuilder::new()
        .chunk_type(b"IHDR")
        .data(&IHDR[8..21])
        .crc(0x1f15_c489)
        .build_into(&mut output)
        .expect("IHDR builds");
    assert_eq!(view.as_bytes(), IHDR);
    assert_eq!(&*parsed_suffix, &suffix);
    view.set_crc(0xaabb_ccdd).unwrap();
    assert_eq!(view.crc(), 0xaabb_ccdd);
    assert_eq!(&*parsed_suffix, &suffix);

    let initial = [0x3c; 24];
    let mut short = initial;
    assert!(matches!(
        PngChunkBuilder::new()
            .chunk_type(b"IHDR")
            .data(&IHDR[8..21])
            .crc(0x1f15_c489)
            .build_into(&mut short),
        Err(PngChunkWriteError::OutputTooShort {
            expected: 25,
            actual: 24
        })
    ));
    assert_eq!(short, initial);

    let initial = [0x7e; 25];
    let mut wrong_width = initial;
    assert!(matches!(
        PngChunkBuilder::new()
            .chunk_type(b"BAD")
            .data(&IHDR[8..21])
            .crc(0x1f15_c489)
            .build_into(&mut wrong_width),
        Err(PngChunkWriteError::FieldChunkType(error))
            if error == ExactWidthError::new(4, 3)
    ));
    assert_eq!(wrong_width, initial);
}
