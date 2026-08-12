use core::convert::Infallible;
use core::num::NonZeroUsize;

use wire_repr::{EncodePlan, PrefixCodec, PrefixExtent};

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
