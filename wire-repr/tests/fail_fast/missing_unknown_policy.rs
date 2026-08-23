//! error: explicit unknown policy

use wire_repr::Wire;

#[derive(Wire)]
#[wire(tag = U8)]
#[repr(u8)]
enum Operation {
    Ping = 1,
}
