//! Validated views decode lazily, so validation and a later getter each read the field.
//! pair: validated = tagged_generated_validated / tagged_handwritten_validated
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Debug)]
struct NonzeroError;

impl core::fmt::Display for NonzeroError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("zero")
    }
}

impl core::error::Error for NonzeroError {}

fn nonzero(value: u8) -> Result<(), NonzeroError> {
    if value == 0 {
        Err(NonzeroError)
    } else {
        Ok(())
    }
}

#[derive(Wire)]
#[wire(error = NonzeroError)]
struct ValidatedBody {
    #[wire(validate = nonzero)]
    value: u8,
}

#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum ValidatedChoice {
    Data(ValidatedBody) = 1,
    Halt = 2,
}

#[inline(never)]
pub fn tagged_generated_validated(bytes: &[u8]) -> u8 {
    match ValidatedChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u8::MAX, |body| body.value()),
        Err(_) => u8::MAX,
    }
}

#[inline(never)]
pub fn tagged_handwritten_validated(bytes: &[u8]) -> u8 {
    let Some((&tag, remaining)) = bytes.split_first() else {
        return u8::MAX;
    };
    match tag {
        1 => {
            let Some((&value, suffix)) = remaining.split_first() else {
                return u8::MAX;
            };
            if value == 0 || !suffix.is_empty() {
                u8::MAX
            } else {
                value
            }
        }
        2 if remaining.is_empty() => 0,
        _ => u8::MAX,
    }
}

#[test]
fn validated_pair_preserves_validation_before_trailing() {
    for bytes in [
        &[][..],
        &[1, 0],
        &[1, 0, 9],
        &[1, 7],
        &[1, 7, 9],
        &[2],
        &[2, 9],
        &[3],
    ] {
        assert_eq!(
            tagged_generated_validated(black_box(bytes)),
            tagged_handwritten_validated(black_box(bytes)),
        );
    }
}
