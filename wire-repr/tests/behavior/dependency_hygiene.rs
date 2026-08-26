#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Bar {
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct Foo<Build> {
    #[wire(as = u8)]
    has_first: bool,
    #[wire(as = u8)]
    has_second: bool,
    #[wire(flag = has_first)]
    foo_1bar: bool,
    #[wire(depends_on = foo_1bar)]
    present: u8,
    #[wire(flag = has_second)]
    foo1bar: bool,
    #[wire(depends_on = foo1bar)]
    other: u8,
    tail: Build,
}

#[test]
fn conditional_internal_names_do_not_restrict_valid_schema_names() -> TestResult {
    let mut output = [0u8; 4];
    Foo::<Bar>::builder(&mut output[..])
        .foo_1bar(|choice| choice.present(|group| group.present(7)))?
        .foo1bar(|choice| choice.absent())?
        .tail(|bar| bar.tail(9))?
        .finish()?;
    assert_eq!(output, [1, 0, 7, 9]);

    let view = Foo::<Bar>::view(output)?;
    assert_eq!(view.present(), Some(7));
    assert_eq!(view.other(), None);
    assert_eq!(view.tail().tail(), 9);
    Ok(())
}
