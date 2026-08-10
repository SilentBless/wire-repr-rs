#![deny(missing_docs, unsafe_code)]

//! Public immutable bit-projection coverage.

use wire_repr::wire_repr;

wire_repr! {
    /// Sequential projection layout.
    pub layout SequentialBits {
        /// Packed feature flags.
        field flags: U8 { position: 1; projections {
            /// Enabled flag.
            bit enabled: 0;
            /// Mode value.
            bits mode: 1..=3;
            /// Raw Rust getter name.
            bit r#type: 4;
        } }
    }
    /// Absolute projection layout.
    pub absolute layout AbsoluteBits {
        /// Big-endian flags after a preserved gap.
        field flags: BeU16 { offset: 2; projections {
            /// The top semantic bit.
            bit high: 15;
            /// Low semantic nibble.
            bits low: 0..=3;
        } }
    }
    /// Little-endian semantic numbering layout.
    pub layout LittleBits {
        /// Little-endian value.
        field flags: LeU16 { position: 1; projections {
            /// Semantic least-significant bit.
            bit low: 0;
            /// Semantic top bit.
            bit high: 15;
        } }
    }
    /// Twenty-four-bit range layout.
    pub layout Narrow24 {
        /// A three-byte value used for its encoded top bit.
        field top_flags: BeU24 { position: 1; projections {
            /// The highest encoded bit.
            bit top: 23;
        } }
        /// A separate three-byte value used for its complete range.
        field all_flags: BeU24 { position: 2; projections {
            /// Every encoded bit.
            bits all: 0..=23;
        } }
    }
    /// Full-width wide storage layout.
    pub absolute layout Wide {
        /// A complete wide unsigned value.
        field value: LeU128 { offset: 0; projections {
            /// All bits normalized without a width shift.
            bits all: 0..=127;
        } }
    }
}

#[test]
fn sequential_storage_and_projections_are_both_available() {
    let parsed = SequentialBitsView::parse_exact(&[0b0001_1011]);
    let view = match parsed {
        Ok(value) => value,
        Err(error) => panic!("valid sequential layout was rejected: {error}"),
    };
    assert_eq!(view.flags(), 0b0001_1011);
    assert!(view.enabled());
    assert_eq!(view.mode(), 5);
    assert!(view.r#type());
}

#[test]
fn absolute_and_little_endian_use_semantic_lsb_numbering() {
    let absolute = AbsoluteBitsView::parse_exact(&[0xaa, 0xbb, 0x80, 0x09]);
    let absolute = match absolute {
        Ok(value) => value,
        Err(error) => panic!("valid absolute layout was rejected: {error}"),
    };
    assert_eq!(absolute.as_bytes(), &[0xaa, 0xbb, 0x80, 0x09]);
    assert_eq!(absolute.flags(), 0x8009);
    assert!(absolute.high());
    assert_eq!(absolute.low(), 9);

    let little = LittleBitsView::parse_exact(&[0x01, 0x80]);
    let little = match little {
        Ok(value) => value,
        Err(error) => panic!("valid little-endian layout was rejected: {error}"),
    };
    assert_eq!(little.flags(), 0x8001);
    assert!(little.low());
    assert!(little.high());
}

#[test]
fn u24_and_u128_ranges_normalize_without_high_bits() {
    let narrow = Narrow24View::parse_exact(&[0x80, 0x00, 0x00, 0x80, 0x00, 0x01]);
    let narrow = match narrow {
        Ok(value) => value,
        Err(error) => panic!("valid U24 layout was rejected: {error}"),
    };
    assert!(narrow.top());
    assert_eq!(narrow.all(), 0x0080_0001);

    let bytes = [0xff; 16];
    let wide = WideView::parse_exact(&bytes);
    let wide = match wide {
        Ok(value) => value,
        Err(error) => panic!("valid U128 layout was rejected: {error}"),
    };
    assert_eq!(wide.all(), u128::MAX);
}

#[test]
fn parsing_and_errors_remain_owned_by_storage() {
    assert!(matches!(
        SequentialBitsView::parse_exact(&[]),
        Err(SequentialBitsError::InputTooShort {
            expected: 1,
            actual: 0
        })
    ));
    assert!(matches!(
        SequentialBitsView::parse_exact(&[1, 2]),
        Err(SequentialBitsError::TrailingBytes {
            expected: 1,
            actual: 2
        })
    ));
    assert!(SequentialBitsView::parse_exact(&[0]).is_ok());
}
