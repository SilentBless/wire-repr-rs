//! error: recursive object bodies currently support recursive markers, plain fixed scalars, and fixed byte arrays

use wire_repr::WireView;

#[derive(WireView)]
struct Bad<T> {
    left: wire_repr::wire::Recursive<T>,
    #[wire(rest)]
    bytes: wire_repr::wire::Bytes,
}
