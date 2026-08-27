//! error: recursive object builders are not supported

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    right: wire_repr::wire::Recursive<T>,
}
