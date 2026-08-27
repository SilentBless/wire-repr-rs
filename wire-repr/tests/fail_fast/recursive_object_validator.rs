//! error: recursive object bodies currently do not support schema validators

use wire_repr::WireView;

fn validate<T>(_view: &T) -> Result<(), core::convert::Infallible> {
    Ok(())
}

#[derive(WireView)]
#[wire(validate = validate)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    right: wire_repr::wire::Recursive<T>,
}
