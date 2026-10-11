//! error: `at` and `align_before` cannot follow an outstanding `uleb128` controller prefix

use wire_repr::{WireView, wire};

#[derive(WireView)]
struct PlacedSuffix {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    bytes: wire::Bytes,
    #[wire(at = 16)]
    tail: u8,
}
