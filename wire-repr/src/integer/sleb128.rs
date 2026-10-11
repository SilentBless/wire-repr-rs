use crate::NeedMore;

/// A malformed or incomplete signed LEB128 integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Sleb128Error {
    /// The integer has not reached its terminal byte.
    #[error(transparent)]
    NeedMore(#[from] NeedMore),
    /// The representation exceeds its declared signed width.
    #[error("SLEB128 integer at offset {offset} exceeds {bits} bits")]
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
pub fn decode_sleb128<const BITS: u32>(
    input: &[u8],
    offset: usize,
) -> Result<(i128, usize), Sleb128Error> {
    let overflow = Sleb128Error::Overflow { offset, bits: BITS };
    if BITS == 0 || BITS > 128 {
        return Err(overflow);
    }
    let maximum = BITS.div_ceil(7) as usize;
    let input = &input[..input.len().min(maximum)];
    let mut consumed = 0;
    match leb128fmt::decode_sint_slice::<i128, BITS>(input, &mut consumed) {
        Ok(value) => Ok((value, consumed)),
        Err(error) if error.is_more_bytes_needed() && input.len() < maximum => {
            Err(Sleb128Error::NeedMore(NeedMore {
                offset: offset.saturating_add(input.len()),
                additional_at_least: 1,
            }))
        }
        Err(_) => Err(overflow),
    }
}

/// A minimally encoded signed integer.
#[doc(hidden)]
pub struct Sleb128Bytes {
    bytes: [u8; 19],
    len: usize,
}

impl AsRef<[u8]> for Sleb128Bytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

leb128fmt::encode_sint_arr!(encode_i128, i128, 128);

/// Encodes a signed integer using its shortest representation.
#[doc(hidden)]
#[inline]
pub fn encode_sleb128(value: i128) -> Sleb128Bytes {
    let (bytes, len) = encode_i128(value).expect("every i128 fits its physical width");
    Sleb128Bytes { bytes, len }
}
