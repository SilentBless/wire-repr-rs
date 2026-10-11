//! error: recursive object bodies cannot use `uleb128` without variable scalar geometry

use wire_repr::WireView;

#[derive(WireView)]
struct RecursivePair<T> {
    left: wire_repr::wire::Recursive<T>,
    #[wire(uleb128)]
    value: u16,
    right: wire_repr::wire::Recursive<T>,
}
