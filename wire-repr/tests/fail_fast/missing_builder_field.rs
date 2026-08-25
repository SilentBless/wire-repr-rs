//! error: no method named `finish`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    foo: u8,
    bar: u8,
}

fn bar(output: &mut [u8]) {
    let Ok(writer) = Foo::builder(output).foo(1) else {
        return;
    };
    let _ = writer.finish();
}
