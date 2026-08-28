//! error: `Missing: TryFrom<u32>` is not satisfied
//! error: `u32: TryFrom<Missing>` is not satisfied

use wire_repr::{WireBuilder, WireView};

struct Missing;

#[derive(WireView, WireBuilder)]
struct Packet {
    #[wire(as = u32, le)]
    value: Missing,
}
