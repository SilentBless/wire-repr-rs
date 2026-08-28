//! error: custom TryFrom scalar fields do not support stored constants

use wire_repr::WireView;

struct Id;

#[derive(WireView)]
struct Packet {
    #[wire(as = u32, le, constant = Id)]
    value: Id,
}
