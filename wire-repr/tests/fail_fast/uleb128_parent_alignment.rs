//! error: `at` and `align_before` cannot follow an outstanding `uleb128` controller prefix

use wire_repr::{WireBuilder, wire};

#[derive(WireBuilder)]
struct AlignedSuffix {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    bytes: wire::Bytes,
    #[wire(align_before = 4)]
    tail: u8,
}
