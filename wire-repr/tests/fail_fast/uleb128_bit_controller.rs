//! error: `uleb128` cannot be a bit projection controller

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct ProjectedRegister {
    #[wire(uleb128)]
    register: u16,
    #[wire(bits_of = register, bit = 0)]
    enabled: bool,
}
