//! error: no function or associated item named `views`

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(rest)]
    body: wire_repr::wire::Bytes,
}

fn main() {
    let _ = Foo::views(&[1, 2, 3]);
}
