#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "support/packet.rs"]
mod packet;

fuzz_target!(|input: &[u8]| {
    packet::inspect_packet(input);
});
