//! error: `zigzag` requires a signed physical integer

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct UnsignedTransform {
    #[wire(vu128, zigzag)]
    value: u32,
}
