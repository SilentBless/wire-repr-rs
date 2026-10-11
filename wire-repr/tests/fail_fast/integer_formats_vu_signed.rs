//! error: `vu128` requires an unsigned physical integer unless `zigzag` is present

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct MissingTransform {
    #[wire(vu128)]
    value: i64,
}
