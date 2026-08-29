#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "support/recursive.rs"]
mod schema;

fuzz_target!(|input: &[u8]| {
    schema::inspect_recursive(input);
});
