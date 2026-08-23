//! error: byte length source must name an earlier field

use wire_repr::Wire;

#[derive(Wire)]
struct Packet<'wire> {
    #[wire(bytes = length)]
    payload: &'wire [u8],
    length: u8,
}
