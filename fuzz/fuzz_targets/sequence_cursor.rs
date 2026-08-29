#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "support/sequence.rs"]
mod schema;

fuzz_target!(|input: &[u8]| {
    schema::inspect_sequences(input);
});
