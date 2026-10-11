use crate::NeedMore;

/// A malformed or incomplete VU128 integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Vu128Error {
    /// The integer does not contain all bytes declared by its prefix.
    #[error(transparent)]
    NeedMore(#[from] NeedMore),
    /// The representation exceeds its declared unsigned width.
    #[error("VU128 integer at offset {offset} exceeds {bits} bits")]
    Overflow {
        /// Absolute input offset of the integer.
        offset: usize,
        /// Declared physical integer width in bits.
        bits: u32,
    },
}

/// Decodes one leading integer, accepting nonminimal encodings whose value fits the width.
#[doc(hidden)]
#[inline]
pub fn decode_vu128<const BITS: u32>(
    input: &[u8],
    offset: usize,
) -> Result<(u128, usize), Vu128Error> {
    let overflow = Vu128Error::Overflow { offset, bits: BITS };
    if BITS == 0 || BITS > 128 {
        return Err(overflow);
    }
    let prefix = *input.first().ok_or(Vu128Error::NeedMore(NeedMore {
        offset,
        additional_at_least: 1,
    }))?;
    let len = match prefix {
        0x00..=0x7f => 1,
        0x80..=0xbf => 2,
        0xc0..=0xdf => 3,
        0xe0..=0xef => 4,
        _ => usize::from(prefix & 0x0f) + 2,
    };
    if input.len() < len {
        return Err(Vu128Error::NeedMore(NeedMore {
            offset: offset.saturating_add(input.len()),
            additional_at_least: len - input.len(),
        }));
    }
    let value = match prefix {
        0x00..=0x7f => u128::from(prefix),
        0x80..=0xbf => u128::from(prefix & 0x3f) | (u128::from(input[1]) << 6),
        0xc0..=0xdf => {
            u128::from(prefix & 0x1f) | (u128::from(input[1]) << 5) | (u128::from(input[2]) << 13)
        }
        0xe0..=0xef => {
            u128::from(prefix & 0x0f)
                | (u128::from(input[1]) << 4)
                | (u128::from(input[2]) << 12)
                | (u128::from(input[3]) << 20)
        }
        _ => {
            let mut value = 0u128;
            for (index, &byte) in input[1..len].iter().enumerate() {
                value |= u128::from(byte) << (index * 8);
            }
            value
        }
    };
    if value > (u128::MAX >> (128 - BITS)) {
        return Err(overflow);
    }
    Ok((value, len))
}

/// A minimally encoded unsigned VU128 integer.
#[doc(hidden)]
pub struct Vu128Bytes {
    bytes: [u8; 17],
    len: usize,
}

impl AsRef<[u8]> for Vu128Bytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Encodes an unsigned integer using its shortest representation.
#[doc(hidden)]
#[inline]
pub fn encode_vu128(value: u128) -> Vu128Bytes {
    let mut bytes = [0; 17];
    let len = vu128::encode_u128(&mut bytes, value);
    Vu128Bytes { bytes, len }
}
