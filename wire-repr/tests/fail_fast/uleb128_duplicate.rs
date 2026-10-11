//! error: duplicate `uleb128` attribute

use wire_repr::WireView;

#[derive(WireView)]
struct RepeatedEncoding {
    #[wire(uleb128)]
    #[wire(uleb128)]
    value: u16,
}
