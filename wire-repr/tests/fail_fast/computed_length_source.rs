//! error: computed field cannot be a byte length source

use wire_repr::Wire;

fn count() -> usize {
    1
}

#[derive(Wire)]
struct Packet<'wire> {
    #[wire(computed = count())]
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
}
