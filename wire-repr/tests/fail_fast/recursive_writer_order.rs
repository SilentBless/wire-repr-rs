//! error: no method named `opcode`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Leaf {
    value: u8,
}

#[derive(WireBuilder)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    opcode: u8,
    right: wire_repr::wire::Recursive<T>,
}

#[derive(WireBuilder)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 1)]
    Leaf(Leaf),
    #[wire(value = 2)]
    Pair(Pair<Value>),
}

fn main() {
    let mut output = [0u8; 8];
    let _ = Value::builder(&mut output[..]).pair(|pair| pair.opcode(7));
}
