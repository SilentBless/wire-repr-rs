//! error: enum body fields do not accept wire attributes

use wire_repr::WireView;

#[derive(WireView)]
struct FramedLeaf {
    value: u8,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum AttributedChoice {
    #[wire(value = 1)]
    Item(#[wire(vu128)] FramedLeaf),
}
