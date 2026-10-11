/// Maps a signed integer to its unsigned ZigZag representation.
#[doc(hidden)]
#[inline]
pub const fn zigzag_encode(value: i128) -> u128 {
    ((value as u128) << 1) ^ ((value >> 127) as u128)
}

/// Recovers a signed integer from its unsigned ZigZag representation.
#[doc(hidden)]
#[inline]
pub const fn zigzag_decode(value: u128) -> i128 {
    ((value >> 1) as i128) ^ -((value & 1) as i128)
}
