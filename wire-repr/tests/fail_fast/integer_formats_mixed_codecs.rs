//! error: only one integer encoding attribute is allowed

use wire_repr::WireView;

#[derive(WireView)]
struct ConflictingEncoding {
    #[wire(uleb128, vu128)]
    value: u32,
}
