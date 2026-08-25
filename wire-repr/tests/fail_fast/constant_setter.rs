//! error: no method named `foo`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(constant = 1)]
    foo: u8,
}

fn bar(output: &mut [u8]) {
    let _ = Foo::builder(output).foo(1);
}
