//! error: wire::Recursive<T> requires one direct generic root type parameter

use wire_repr::WireView;

#[derive(WireView)]
struct Bad {
    child: wire_repr::wire::Recursive<u8>,
}
