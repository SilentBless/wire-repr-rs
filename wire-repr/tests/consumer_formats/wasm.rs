use core::convert::Infallible;
use core::num::NonZeroUsize;

use wire_repr::{EncodePlan, PrefixCodec, PrefixExtent, wire_repr};

/// Structural failures while framing a `u32` ULEB128 prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum U32Leb128DecodeError {
    /// Input ended before a terminating byte was available.
    Incomplete,
    /// The fifth byte continued past the maximum `u32` ULEB128 width.
    Malformed,
    /// The fifth byte represented bits outside the `u32` range.
    Overflow,
}

/// An allocation-free canonical `u32` ULEB128 encoding plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct U32Leb128Plan {
    bytes: [u8; 5],
    len: usize,
}

impl EncodePlan for U32Leb128Plan {
    fn encoded_len(&self) -> usize {
        self.len
    }

    fn write_into(&self, output: &mut [u8]) {
        output.copy_from_slice(&self.bytes[..self.len]);
    }
}

/// A safe allocation-free `u32` ULEB128 prefix codec.
pub struct U32Leb128;

impl PrefixCodec for U32Leb128 {
    type Value<'wire>
        = u32
    where
        Self: 'wire;
    type DecodeError = U32Leb128DecodeError;
    type EncodeError = Infallible;
    type Plan<'value>
        = U32Leb128Plan
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        for (index, byte) in bytes.iter().copied().take(5).enumerate() {
            if index == 4 {
                if byte & 0x80 != 0 {
                    return Err(U32Leb128DecodeError::Malformed);
                }
                if byte & 0x70 != 0 {
                    return Err(U32Leb128DecodeError::Overflow);
                }
                return Ok(PrefixExtent::new(NonZeroUsize::MIN.saturating_add(4)));
            }
            if byte & 0x80 == 0 {
                let encoded_len =
                    NonZeroUsize::new(index + 1).ok_or(U32Leb128DecodeError::Incomplete)?;
                return Ok(PrefixExtent::new(encoded_len));
            }
        }
        Err(U32Leb128DecodeError::Incomplete)
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes
            .iter()
            .copied()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| {
                value | (u32::from(byte & 0x7f) << (index * 7))
            })
    }

    fn plan<'value>(
        mut value: Self::Value<'value>,
    ) -> Result<Self::Plan<'value>, Self::EncodeError> {
        let mut bytes = [0; 5];
        let mut len = 0;
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes[len] = byte;
            len += 1;
            if value == 0 {
                return Ok(U32Leb128Plan { bytes, len });
            }
        }
    }
}

wire_repr! {
    /// One opaque WebAssembly section framed by its standard section size.
    pub layout WasmSection {
        /// The opaque WebAssembly section identifier.
        field id: U8 { position: 1; }
        /// The ULEB128-encoded byte length of `contents`.
        field size: prefix(U32Leb128) { position: 2; }
        /// The opaque section payload.
        field contents: region(size) { position: 3; }
    }
}

#[test]
fn custom_section_preserves_its_exact_borrowed_bytes_and_suffix() {
    let section = [0, 10, 4, b'n', b'a', b'm', b'e', 1, 2, 3, 4, 5];
    let suffix = [0xde, 0xad];
    let mut input = section.to_vec();
    input.extend_from_slice(&suffix);

    let (view, parsed_suffix) =
        WasmSectionView::parse_prefix(&input).expect("custom section parses");
    assert_eq!(view.as_bytes(), section);
    assert_eq!(view.id(), 0);
    assert_eq!(view.size(), 10);
    assert_eq!(view.contents(), &section[2..]);
    assert_eq!(view.contents().as_ptr(), input[2..].as_ptr());
    assert_eq!(parsed_suffix, suffix);
    assert_eq!(parsed_suffix.as_ptr(), input[section.len()..].as_ptr());
    assert!(matches!(
        WasmSectionView::parse_exact(&input),
        Err(WasmSectionError::TrailingBytes {
            expected: 12,
            actual: 14
        })
    ));
}

#[test]
fn noncanonical_size_is_preserved_across_immutable_and_mutable_views() {
    let mut bytes = [3, 0x85, 0, b'h', b'e', b'l', b'l', b'o'];
    let immutable = WasmSectionView::parse_exact(&bytes).expect("legal noncanonical size parses");
    assert_eq!(immutable.size_encoded(), &[0x85, 0]);
    assert_eq!(immutable.size(), 5);
    assert_eq!(immutable.as_bytes(), bytes);

    let mutable =
        WasmSectionViewMut::parse_exact_mut(&mut bytes).expect("mutable parse preserves bytes");
    assert_eq!(mutable.as_view().size_encoded(), &[0x85, 0]);
    let immutable = mutable.into_view();
    assert_eq!(
        immutable.as_bytes(),
        &[3, 0x85, 0, b'h', b'e', b'l', b'l', b'o']
    );
}

#[test]
fn builder_derives_canonical_size_and_only_id_remains_mutable() {
    let mut output = [0xcc; 10];
    output[8..].copy_from_slice(&[0xa5, 0x5a]);
    let (mut view, suffix) = WasmSectionBuilder::new()
        .id(0)
        .contents(&[4, b'n', b'a', b'm', b'e', 9])
        .build_into(&mut output)
        .expect("builder derives section size from contents");
    assert_eq!(view.as_bytes(), &[0, 6, 4, b'n', b'a', b'm', b'e', 9]);
    assert_eq!(view.size_encoded(), &[6]);
    assert_eq!(view.size(), 6);
    assert_eq!(&*suffix, &[0xa5, 0x5a]);
    view.set_id(7).expect("fixed section id remains mutable");
    assert_eq!(view.as_bytes(), &[7, 6, 4, b'n', b'a', b'm', b'e', 9]);
    assert_eq!(&*suffix, &[0xa5, 0x5a]);
}

#[test]
fn short_builder_output_does_not_modify_any_byte() {
    let initial = [0x3c; 7];
    let mut output = initial;
    assert!(matches!(
        WasmSectionBuilder::new()
            .id(1)
            .contents(b"abcdef")
            .build_into(&mut output),
        Err(WasmSectionWriteError::OutputTooShort {
            expected: 8,
            actual: 7
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn uleb_failures_map_through_the_section_parse_error() {
    for bytes in [
        &[][..],
        &[0x80][..],
        &[0x80, 0x80][..],
        &[0x80, 0x80, 0x80][..],
        &[0x80, 0x80, 0x80, 0x80][..],
    ] {
        assert_eq!(
            U32Leb128::validate_prefix(bytes),
            Err(U32Leb128DecodeError::Incomplete)
        );
    }
    assert_eq!(
        U32Leb128::validate_prefix(&[0x85, 0])
            .expect("complete prefix reports its exact span")
            .encoded_len()
            .get(),
        2
    );
    let plan = U32Leb128::plan(u32::MAX).expect("every u32 has a canonical ULEB128 encoding");
    let mut encoded = [0; 5];
    plan.write_into(&mut encoded);
    assert_eq!(encoded, [0xff, 0xff, 0xff, 0xff, 0x0f]);
    assert_eq!(
        U32Leb128::validate_prefix(&encoded)
            .expect("maximum u32 encoding is structurally valid")
            .encoded_len()
            .get(),
        5
    );
    assert_eq!(U32Leb128::decode(&encoded), u32::MAX);
    for bytes in [
        &[0][..],
        &[0, 0x80][..],
        &[0, 0x80, 0x80, 0x80, 0x80, 0x80][..],
        &[0, 0xff, 0xff, 0xff, 0xff, 0x10][..],
    ] {
        let expected = match bytes.len() {
            1 | 2 => U32Leb128DecodeError::Incomplete,
            6 if bytes[5] == 0x80 => U32Leb128DecodeError::Malformed,
            _ => U32Leb128DecodeError::Overflow,
        };
        assert!(matches!(
            WasmSectionView::parse_prefix(bytes),
            Err(WasmSectionError::FieldSize(error)) if error == expected
        ));
    }
}

#[test]
fn valid_size_with_missing_contents_reports_region_framing() {
    assert!(matches!(
        WasmSectionView::parse_prefix(&[1, 3, 0xaa, 0xbb]),
        Err(WasmSectionError::InputTooShort {
            position: 3,
            expected: 3,
            available: 2
        })
    ));
}

#[test]
fn zero_length_section_is_a_complete_nonstalling_layout() {
    let view = WasmSectionView::parse_exact(&[9, 0]).expect("empty section parses");
    assert_eq!(view.id(), 9);
    assert_eq!(view.size(), 0);
    assert!(view.contents().is_empty());
    assert_eq!(view.as_bytes(), &[9, 0]);
}
