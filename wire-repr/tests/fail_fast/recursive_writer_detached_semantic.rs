//! error: no method named `pair`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Leaf {
    value: u8,
}

#[derive(WireBuilder)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
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

#[derive(WireBuilder)]
struct Envelope<T> {
    value: T,
}

fn main() {
    let mut output = [0u8; 8];
    let _ = Envelope::<Value>::builder(&mut output[..]).value(|value| value.pair(|pair| pair));
}
