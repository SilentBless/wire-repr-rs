//! pair: scalar = tagged_generated_scalar / tagged_handwritten_scalar
//! pair: fixed_bytes = tagged_generated_fixed_bytes / tagged_handwritten_fixed_bytes
//! pair: mapped = tagged_generated_mapped / tagged_handwritten_mapped
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct Body {
    #[wire(be)]
    value: u16,
}

#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum Choice {
    Halt = 1,
    Data(Body) = 2,
}

#[derive(Wire)]
#[wire(tag = [u8; 4], unknown = reject)]
enum ByteChoice {
    #[wire(tag = b"HALT")]
    Halt,
    #[wire(tag = b"DATA")]
    Data(Body),
}


#[derive(Clone, Copy, PartialEq)]
enum Code {
    Data,
    Halt,
}
#[derive(Debug)]
struct TableError;
impl core::fmt::Display for TableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("table")
    }
}
impl core::error::Error for TableError {}
struct Table;
impl Table {
    fn decode(&self, raw: u8) -> Result<Option<Code>, TableError> {
        Ok(match raw {
            0x41 => Some(Code::Data),
            0x7f => Some(Code::Halt),
            _ => None,
        })
    }
    fn encode(&self, code: Code) -> Result<Option<u8>, TableError> {
        Ok(Some(match code {
            Code::Data => 0x41,
            Code::Halt => 0x7f,
        }))
    }
}

#[derive(Wire)]
#[wire(tag = U8, table = Table, table_error = TableError, unknown = reject)]
enum MappedChoice {
    #[wire(table = Code::Data)]
    Data(Body),
    #[wire(table = Code::Halt)]
    Halt,
}

macro_rules! generated_decode {
    ($name:ident, $ty:ty, $request:expr) => {
        #[inline(never)]
        pub fn $name(bytes: &[u8]) -> u16 {
            match $request(bytes).without_trailing() {
                Ok(choice) if choice.is_halt() => 0,
                Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
                Err(_) => u16::MAX,
            }
        }
    };
}

generated_decode!(tagged_generated_scalar, Choice, Choice::view);
generated_decode!(tagged_generated_fixed_bytes, ByteChoice, ByteChoice::view);

#[inline(never)]
pub fn tagged_handwritten_scalar(bytes: &[u8]) -> u16 {
    match bytes {
        [1] => 0,
        [2, high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}
#[inline(never)]
pub fn tagged_handwritten_fixed_bytes(bytes: &[u8]) -> u16 {
    match bytes {
        b"HALT" => 0,
        [b'D', b'A', b'T', b'A', high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}

#[inline(never)]
pub fn tagged_generated_mapped(bytes: &[u8]) -> u16 {
    match MappedChoice::view(bytes).table(&Table).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}
#[inline(never)]
pub fn tagged_handwritten_mapped(bytes: &[u8]) -> u16 {
    let Some((&raw, remaining)) = bytes.split_first() else {
        return u16::MAX;
    };
    let Ok(Some(code)) = Table.decode(raw) else {
        return u16::MAX;
    };
    match code {
        Code::Data => {
            let [high, low] = remaining else {
                return u16::MAX;
            };
            u16::from_be_bytes([*high, *low])
        }
        Code::Halt if remaining.is_empty() => 0,
        Code::Halt => u16::MAX,
    }
}

#[test]
fn tagged_pairs_are_semantically_equivalent() {
    for bytes in [&[][..], &[1], &[2, 0x12, 0x34], &[3]] {
        assert_eq!(
            tagged_generated_scalar(black_box(bytes)),
            tagged_handwritten_scalar(black_box(bytes))
        );
    }
    for bytes in [&[][..], b"HALT", b"DATA\x12\x34", b"NOPE"] {
        assert_eq!(
            tagged_generated_fixed_bytes(black_box(bytes)),
            tagged_handwritten_fixed_bytes(black_box(bytes))
        );
    }

    for bytes in [&[][..], &[0x41, 0x12, 0x34], &[0x7f], &[0x55]] {
        assert_eq!(
            tagged_generated_mapped(black_box(bytes)),
            tagged_handwritten_mapped(black_box(bytes))
        );
    }
}
