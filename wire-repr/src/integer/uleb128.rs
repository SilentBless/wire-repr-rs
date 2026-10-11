use crate::NeedMore;

/// A malformed or incomplete unsigned LEB128 integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Uleb128Error {
    /// The integer has not reached its terminal byte.
    #[error(transparent)]
    NeedMore(#[from] NeedMore),
    /// The representation exceeds its declared unsigned width.
    #[error("ULEB128 integer at offset {offset} exceeds {bits} bits")]
    Overflow {
        /// Absolute input offset of the integer.
        offset: usize,
        /// Declared physical integer width in bits.
        bits: u32,
    },
}

/// Decodes one leading integer, accepting nonminimal encodings within the width bound.
#[doc(hidden)]
#[inline]
pub fn decode_uleb128<const BITS: u32>(
    input: &[u8],
    offset: usize,
) -> Result<(u128, usize), Uleb128Error> {
    let overflow = Uleb128Error::Overflow { offset, bits: BITS };
    if !matches!(BITS, 8 | 16 | 32 | 64 | 128) {
        return Err(overflow);
    }
    let input = &input[..input.len().min(BITS.div_ceil(7) as usize)];
    let mut consumed = 0;
    match leb128fmt::decode_uint_slice::<u128, BITS>(input, &mut consumed) {
        Ok(value) => Ok((value, consumed)),
        Err(error) if error.is_more_bytes_needed() => Err(Uleb128Error::NeedMore(NeedMore {
            offset: offset.saturating_add(input.len()),
            additional_at_least: 1,
        })),
        Err(_) => Err(overflow),
    }
}

/// A minimally encoded unsigned integer.
#[doc(hidden)]
pub struct Uleb128Bytes {
    bytes: [u8; 19],
    len: usize,
}

impl AsRef<[u8]> for Uleb128Bytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

leb128fmt::encode_uint_arr!(encode_u128, u128, 128);

/// Encodes an unsigned integer using its shortest representation.
#[doc(hidden)]
#[inline]
pub fn encode_uleb128(value: u128) -> Uleb128Bytes {
    let (bytes, len) = encode_u128(value).expect("every u128 fits its physical width");
    Uleb128Bytes { bytes, len }
}
