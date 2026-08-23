//! error: overlaps

use wire_repr::Wire;

#[derive(Wire)]
#[wire(bitfield = u8, reserved = zero)]
struct Flags {
    #[wire(bits = 0..=4)]
    mode: u8,
    #[wire(bit = 4)]
    enabled: bool,
}
