//! error: duplicate `zigzag` attribute

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct RepeatedTransform {
    #[wire(uleb128, zigzag, zigzag)]
    value: i16,
}
