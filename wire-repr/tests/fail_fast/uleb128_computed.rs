//! error: computed destinations cannot use `uleb128`

use wire_repr::WireBuilder;

fn calculate() -> u16 {
    300
}

#[derive(WireBuilder)]
struct ComputedValue {
    #[wire(uleb128, computed = calculate())]
    value: u16,
}
