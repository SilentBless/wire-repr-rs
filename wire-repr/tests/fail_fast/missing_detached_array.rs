//! error: mismatched types

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Item {
    value: u8,
}

#[derive(WireBuilder)]
struct Group {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Item>,
    tail: u8,
}

#[derive(WireBuilder)]
struct Packet {
    group: Group,
}

fn main() {
    let mut output = [0u8; 2];
    let _ = Packet::builder(&mut output[..]).group(|group| group.tail(7));
}
