use wire_repr::{PreparedLayout, Wire};

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

/// Nominal PNG chunk type with lossless preservation of unknown names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Wire)]
#[wire(tag = [u8; 4], unknown = preserve)]
pub enum ChunkType {
    /// Image header.
    #[wire(tag = b"IHDR")]
    Ihdr,
    /// Image data.
    #[wire(tag = b"IDAT")]
    Idat,
    /// Image trailer.
    #[wire(tag = b"IEND")]
    Iend,
    /// Any structurally representable chunk type not known here.
    #[wire(unknown)]
    Other([u8; 4]),
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

/// One dynamically sized PNG chunk.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct PngChunk<'wire> {
    /// The encoded byte count of `data`.
    #[wire(be)]
    pub data_length: u32,
    /// The nominal four-byte PNG chunk type.
    pub chunk_type: ChunkType,
    /// The opaque chunk payload.
    #[wire(bytes = data_length)]
    pub data: &'wire [u8],
    /// The stored CRC-32/ISO-HDLC value.
    #[wire(be)]
    pub crc: u32,
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
fn ihdr_and_iend_preserve_exact_fields_and_disjoint_suffix() {
    let suffix = [0xde, 0xad];
    let mut input = IHDR.to_vec();
    input.extend_from_slice(&suffix);

    let (view, parsed_suffix) = PngChunk::view(&input)
        .with_remainder()
        .expect("IHDR parses");
    assert_eq!(view.as_bytes().len(), 25);
    assert_eq!(view.as_bytes(), IHDR);
    assert_eq!(parsed_suffix, suffix);
    assert_eq!(parsed_suffix.as_ptr(), input[25..].as_ptr());
    assert_eq!(view.data_length(), read_be_u32(&IHDR, 0));
    assert!(view.chunk_type().is_ihdr());
    assert_eq!(view.chunk_type().as_bytes(), &IHDR[4..8]);
    assert_eq!(view.data(), &IHDR[8..21]);
    assert_eq!(view.crc(), read_be_u32(&IHDR, 21));
    assert!(matches!(
        PngChunk::view(&input).without_trailing(),
        Err(PngChunkDecodeError::TrailingBytes {
            expected: 25,
            actual: 27
        })
    ));

    let iend = PngChunk::view(&IEND)
        .without_trailing()
        .expect("zero-length IEND parses");
    assert_eq!(iend.data_length(), 0);
    assert!(iend.chunk_type().is_iend());
    assert_eq!(iend.chunk_type().as_bytes(), b"IEND");
    assert_eq!(iend.data(), []);
    assert_eq!(iend.crc(), 0xae42_6082);

    for (chunk_type, expected) in [(ChunkType::Idat, b"IDAT"), (ChunkType::Iend, b"IEND")] {
        let mut output = [0; 4];
        let (written, suffix) = chunk_type
            .build_into(&mut output)
            .expect("known chunk type commits");
        assert_eq!(written.as_bytes(), expected);
        assert!(suffix.is_empty());
    }
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
    let chunk = PngChunk::view(&malformed)
        .without_trailing()
        .expect("raw type is structurally opaque");
    assert_eq!(chunk.chunk_type().other(), Some(b"1END"));
    assert_eq!(
        validate_chunk_type(chunk.chunk_type().as_bytes()),
        Err(ChunkTypeError::NonAsciiLetter {
            index: 0,
            byte: b'1'
        })
    );

    let overclaimed_data = [0, 0, 0, 1, b'I', b'E', b'N', b'D'];
    assert!(matches!(
        PngChunk::view(&overclaimed_data).with_remainder(),
        Err(PngChunkDecodeError::InputTooShort {
            field: "data",
            required: 1,
            available: 0
        })
    ));
    assert!(matches!(
        PngChunk::view(&IHDR[..20]).without_trailing(),
        Err(PngChunkDecodeError::InputTooShort {
            field: "data",
            required: 13,
            available: 12
        })
    ));
}

#[test]
fn crc_stays_consumer_validation_not_layout_validation() {
    assert_eq!(crc32_iso_hdlc(&IHDR[4..21]), 0x1f15_c489);
    assert_eq!(crc32_iso_hdlc(&IEND[4..8]), 0xae42_6082);
    let ihdr = PngChunk::view(&IHDR).without_trailing().unwrap();
    let iend = PngChunk::view(&IEND).without_trailing().unwrap();
    assert!(crc_matches(&ihdr.as_bytes()[4..21], ihdr.crc()));
    assert!(crc_matches(&iend.as_bytes()[4..8], iend.crc()));

    let mut wrong_crc = IHDR;
    wrong_crc[24] ^= 1;
    let view = PngChunk::view(&wrong_crc)
        .without_trailing()
        .expect("CRC bytes are structurally opaque");
    assert!(!crc_matches(&view.as_bytes()[4..21], view.crc()));
}

#[test]
fn prepared_encoding_derives_length_and_is_atomic() {
    let suffix = [0x5a, 0xa5];
    let plan = PngChunk {
        data_length: 99,
        chunk_type: ChunkType::Ihdr,
        data: &IHDR[8..21],
        crc: 0x1f15_c489,
    }
    .prepare()
    .expect("IHDR prepares");
    assert_eq!(plan.encoded_len(), 25);

    let mut output = [0xcc; 27];
    output[25..].copy_from_slice(&suffix);
    let (written, parsed_suffix) = plan.commit_into(&mut output).expect("IHDR commits");
    assert_eq!(written.as_bytes(), IHDR);
    assert_eq!(&*parsed_suffix, &suffix);

    let plan = PngChunk {
        data_length: 0,
        chunk_type: ChunkType::Ihdr,
        data: &IHDR[8..21],
        crc: 0x1f15_c489,
    }
    .prepare()
    .unwrap();
    let initial = [0x3c; 24];
    let mut short = initial;
    assert!(plan.commit_into(&mut short).is_err());
    assert_eq!(short, initial);
}

#[test]
fn unknown_chunk_types_round_trip_losslessly() {
    let mut private = IEND;
    private[4..8].copy_from_slice(b"vpAg");
    let view = PngChunk::view(&private)
        .without_trailing()
        .expect("unknown chunk type remains structurally representable");
    assert_eq!(view.chunk_type().other(), Some(b"vpAg"));

    let plan = PngChunk {
        data_length: 0,
        chunk_type: ChunkType::Other(*b"vpAg"),
        data: &[],
        crc: 0xae42_6082,
    }
    .prepare()
    .expect("unknown chunk type prepares");
    let mut output = [0; 12];
    let (written, suffix) = plan
        .commit_into(&mut output)
        .expect("unknown chunk commits");
    assert_eq!(&written.as_bytes()[4..8], b"vpAg");
    assert!(suffix.is_empty());
}

#[test]
fn consecutive_png_chunks_use_a_fail_closed_typed_cursor() {
    let mut datastream = Vec::from(IHDR);
    datastream.extend_from_slice(&IEND);

    let mut chunks = PngChunk::cursor(&datastream);
    let ihdr = chunks.next().unwrap().unwrap();
    assert!(ihdr.chunk_type().is_ihdr());
    assert_eq!(ihdr.data(), &IHDR[8..21]);
    let iend = chunks.next().unwrap().unwrap();
    assert!(iend.chunk_type().is_iend());
    assert_eq!(iend.chunk_type().as_bytes(), b"IEND");
    assert!(iend.data().is_empty());
    assert!(chunks.next().unwrap().is_none());
    assert!(chunks.remaining().is_empty());

    let truncated = &datastream[..datastream.len() - 1];
    let mut chunks = PngChunk::cursor(truncated);
    assert!(chunks.next().unwrap().is_some());
    let failing = chunks.remaining();
    assert!(matches!(
        chunks.next(),
        Err(wire_repr::ViewCursorError::Item(
            PngChunkDecodeError::InputTooShort { field: "crc", .. }
        ))
    ));
    assert_eq!(chunks.remaining().as_ptr(), failing.as_ptr());
    assert_eq!(chunks.remaining().len(), failing.len());
}
