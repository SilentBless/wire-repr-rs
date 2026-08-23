use super::schema::{
    CodegenByteChoice, CodegenChoice, CodegenFlags, DynamicPacket, FixedPacket, TableChoice,
    TinyTable, ValidatedChoice,
};

/// Decodes one fixed representation through generated derive code.
#[inline(never)]
pub(super) fn generated_fixed_decode(bytes: &[u8]) -> u16 {
    FixedPacket::view(bytes)
        .with_remainder()
        .map_or(u16::MAX, |(packet, _)| packet.word())
}

/// Decodes the fixed representation directly.
#[inline(never)]
pub(super) fn handwritten_fixed_decode(bytes: &[u8]) -> u16 {
    let Some(bytes) = bytes.get(..2) else {
        return u16::MAX;
    };
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// Decodes bounded borrowed bytes through generated derive code.
#[inline(never)]
pub(super) fn generated_bounded_decode(bytes: &[u8]) -> u8 {
    DynamicPacket::view(bytes)
        .without_trailing()
        .ok()
        .and_then(|packet| {
            packet
                .payload()
                .first()
                .copied()
                .map(|first| first ^ packet.tail() ^ packet.length())
        })
        .unwrap_or(u8::MAX)
}

/// Decodes the same bounded bytes directly.
#[inline(never)]
pub(super) fn handwritten_bounded_decode(bytes: &[u8]) -> u8 {
    let Some((&length, remaining)) = bytes.split_first() else {
        return u8::MAX;
    };
    let length = usize::from(length);
    if remaining.len() != length + 1 || length == 0 {
        return u8::MAX;
    }
    remaining[0] ^ remaining[length] ^ length as u8
}

/// Dispatches a tagged enum through generated derive code.
#[inline(never)]
pub(super) fn generated_enum_decode(bytes: &[u8]) -> u16 {
    match CodegenChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}

/// Dispatches the same tagged representation directly.
#[inline(never)]
pub(super) fn handwritten_enum_decode(bytes: &[u8]) -> u16 {
    match bytes {
        [1] => 0,
        [2, high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}

/// Dispatches a fixed byte-tagged enum through generated derive code.
#[inline(never)]
pub(super) fn generated_byte_enum_decode(bytes: &[u8]) -> u16 {
    match CodegenByteChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}

/// Dispatches the same fixed byte-tagged representation directly.
#[inline(never)]
pub(super) fn handwritten_byte_enum_decode(bytes: &[u8]) -> u16 {
    match bytes {
        b"HALT" => 0,
        [b'D', b'A', b'T', b'A', high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}

/// Decodes nominal bit projections through generated derive code.
#[inline(never)]
pub(super) fn generated_bitfield_decode(bytes: &[u8]) -> u8 {
    CodegenFlags::view(bytes)
        .without_trailing()
        .map_or(u8::MAX, |flags| {
            u8::from(flags.enabled()) | (flags.mode() << 1)
        })
}

/// Decodes the same bit projections directly.
#[inline(never)]
pub(super) fn handwritten_bitfield_decode(bytes: &[u8]) -> u8 {
    let [high, low] = bytes else {
        return u8::MAX;
    };
    let raw = u16::from_be_bytes([*high, *low]);
    u8::from(raw & 1 != 0) | (((raw >> 1) as u8 & 0x07) << 1)
}

/// Sums a fixed sequence through the generated infallible view iterator.
#[inline(never)]
pub(super) fn generated_fixed_sequence(bytes: &[u8]) -> u16 {
    let Ok(packets) = FixedPacket::views(bytes) else {
        return u16::MAX;
    };
    packets.fold(0, |sum, packet| sum.wrapping_add(packet.word()))
}

/// Sums the same fixed-width sequence directly.
#[inline(never)]
pub(super) fn handwritten_fixed_sequence(bytes: &[u8]) -> u16 {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return u16::MAX;
    }
    chunks.fold(0, |sum, bytes| {
        sum.wrapping_add(u16::from_be_bytes([bytes[0], bytes[1]]))
    })
}

/// Walks variable-width records through the generated fail-closed cursor.
#[inline(never)]
pub(super) fn generated_variable_cursor(bytes: &[u8]) -> u8 {
    let mut records = DynamicPacket::cursor(bytes);
    let mut sum = 0u8;
    loop {
        match records.next() {
            Ok(Some(record)) => {
                sum = sum
                    .wrapping_add(record.length())
                    .wrapping_add(record.tail());
            }
            Ok(None) => return sum,
            Err(_) => return u8::MAX,
        }
    }
}

/// Walks the same variable-width records directly.
#[inline(never)]
pub(super) fn handwritten_variable_cursor(mut bytes: &[u8]) -> u8 {
    let mut sum = 0u8;
    while let Some((&length, remaining)) = bytes.split_first() {
        let length = usize::from(length);
        let tail_index = length;
        let Some((&tail, suffix)) = remaining
            .get(tail_index)
            .zip(remaining.get(tail_index + 1..))
        else {
            return u8::MAX;
        };
        sum = sum.wrapping_add(length as u8).wrapping_add(tail);
        bytes = suffix;
    }
    sum
}
/// Decodes a validated nested enum body through generated derive code.
#[inline(never)]
pub(super) fn generated_validated_enum_decode(bytes: &[u8]) -> u8 {
    match ValidatedChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u8::MAX, |body| body.value()),
        Err(_) => u8::MAX,
    }
}

/// Decodes the same validated nested enum body directly.
#[inline(never)]
pub(super) fn handwritten_validated_enum_decode(bytes: &[u8]) -> u8 {
    match bytes {
        [1, value] if *value != 0 => *value,
        [2] => 0,
        _ => u8::MAX,
    }
}

/// Decodes one table-selected operation through generated derive code.
#[inline(never)]
pub(super) fn generated_table_decode(bytes: &[u8]) -> u16 {
    let table = TinyTable {
        data: 0x41,
        halt: 0x7f,
    };
    match TableChoice::view(bytes).table(&table).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}

/// Decodes the same table-selected operation directly.
#[inline(never)]
pub(super) fn handwritten_table_decode(bytes: &[u8]) -> u16 {
    match bytes {
        [0x41, high, low] => u16::from_be_bytes([*high, *low]),
        [0x7f] => 0,
        _ => u16::MAX,
    }
}
