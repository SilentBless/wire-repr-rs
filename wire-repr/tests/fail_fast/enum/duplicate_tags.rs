//! error: duplicate

use wire_repr::Wire;

#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum Operation {
    First = 1,
    Second = 1,
}
