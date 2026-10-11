//! error: `zigzag` requires `uleb128` or `vu128`

use wire_repr::WireView;

#[derive(WireView)]
struct ConflictingTransform {
    #[wire(sleb128, zigzag)]
    value: i32,
}
