//! error: condition must name a physically earlier logical flag field

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    controller: u8,
    #[wire(depends_on = controller)]
    value: u8,
}
