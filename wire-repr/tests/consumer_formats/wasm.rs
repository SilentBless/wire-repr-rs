use core::convert::Infallible;
use core::num::NonZeroUsize;

use wire_repr::{
    ByteSegment, ByteSink, ByteSource, ByteSourceCursor, PrefixCodec, PrefixExtent, PreparedLayout,
    Wire,
};

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

impl ByteSource for U32Leb128Plan {
    fn byte_len(&self) -> usize {
        self.len
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&self.bytes[..self.len]);
    }
}

impl ByteSourceCursor for U32Leb128Plan {
    type Segments<'source>
        = core::iter::Once<ByteSegment<'source>>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        core::iter::once(ByteSegment::Bytes(&self.bytes[..self.len]))
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

/// A WebAssembly section with a ULEB128 payload length.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct WasmSection<'wire> {
    /// Section identifier.
    pub id: u8,
    /// Encoded payload byte count.
    #[wire(prefix = U32Leb128)]
    pub payload_length: u32,
    /// Opaque section payload.
    #[wire(bytes = payload_length)]
    pub payload: &'wire [u8],
}

#[test]
fn uleb128_preserves_noncanonical_values_and_builds_canonically() {
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

    let noncanonical = [0x85, 0];
    let extent = U32Leb128::validate_prefix(&noncanonical)
        .expect("complete noncanonical prefix reports its exact span");
    assert_eq!(extent.encoded_len().get(), 2);
    assert_eq!(U32Leb128::decode(&noncanonical), 5);

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
    assert_eq!(
        U32Leb128::validate_prefix(&[0x80, 0x80, 0x80, 0x80, 0x80]),
        Err(U32Leb128DecodeError::Malformed)
    );
    assert_eq!(
        U32Leb128::validate_prefix(&[0xff, 0xff, 0xff, 0xff, 0x10]),
        Err(U32Leb128DecodeError::Overflow)
    );
}

#[test]
fn wasm_sections_preserve_accepted_framing_and_encode_canonically() {
    let input = [1, 0x83, 0, 0xaa, 0xbb, 0xcc, 0xdd];
    let (section, suffix) = WasmSection::view(&input).with_remainder().unwrap();
    assert_eq!(section.id(), 1);
    assert_eq!(section.payload_length(), 3);
    assert_eq!(section.payload(), &[0xaa, 0xbb, 0xcc]);
    assert_eq!(section.as_bytes(), &input[..6]);
    assert_eq!(suffix, &[0xdd]);

    let plan = WasmSection {
        id: section.id(),
        payload_length: section.payload_length(),
        payload: section.payload(),
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 5);
    let mut output = [0xa5; 6];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[1, 3, 0xaa, 0xbb, 0xcc]);
    assert_eq!(suffix, &mut [0xa5]);
}
