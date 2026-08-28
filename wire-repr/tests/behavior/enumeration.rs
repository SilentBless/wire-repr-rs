#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Bar {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Baz {
    #[wire(be)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u16, be)]
enum Foo {
    #[wire(value = 0x1122)]
    First(Bar),
    #[wire(value = 0x3344)]
    Second(Baz),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

#[derive(WireBuilder)]
struct Envelope<T> {
    length: u8,
    #[wire(bytes = length)]
    value: T,
    tail: u8,
}

#[derive(WireView)]
struct Bounded<T> {
    length: u8,
    #[wire(bytes = length)]
    value: T,
    tail: u8,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Closed {
    #[wire(value = 1)]
    First(Bar),
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Generic<T> {
    #[wire(value = 1)]
    First(T),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

trait Tagged {
    const CODE: u8;
    const ALSO: u8;
}

impl Tagged for Bar {
    const CODE: u8 = 3;
    const ALSO: u8 = 3;
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Associated<T: Tagged> {
    #[wire(value = T::CODE)]
    First(T),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Duplicate<T: Tagged> {
    #[wire(value = T::CODE)]
    First(T),
    #[wire(value = T::ALSO)]
    Second(T),
}

#[allow(non_camel_case_types)]
#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Collision {
    #[wire(value = 1)]
    FooBar(Bar),
    #[wire(value = 2)]
    foo_bar(Bar),
    #[wire(value = 3)]
    unknown(Bar),
    #[wire(unknown)]
    Fallback(wire_repr::wire::Bytes),
}

#[test]
fn static_selector_returns_a_borrowed_exhaustive_variant() -> TestResult {
    let first = Foo::view([0x11, 0x22, 7])?;
    match first.variant() {
        FooVariant::First(value) => assert_eq!(value.value(), 7),
        FooVariant::Second(_) | FooVariant::Unknown { .. } => panic!("wrong variant"),
    }

    let second = Foo::view([0x33, 0x44, 0xaa, 0xbb])?;
    match second.variant() {
        FooVariant::Second(value) => assert_eq!(value.value(), 0xaabb),
        FooVariant::First(_) | FooVariant::Unknown { .. } => panic!("wrong variant"),
    }
    Ok(())
}

#[test]
fn unknown_selector_preserves_selector_and_exact_body() -> TestResult {
    let input = [0x55, 0x66, 1, 2, 3, 4];
    let view = Foo::view(input)?;
    match view.variant() {
        FooVariant::Unknown { selector, body } => {
            assert_eq!(selector, 0x5566);
            assert_eq!(body, &[1, 2, 3, 4]);
        }
        FooVariant::First(_) | FooVariant::Second(_) => panic!("known variant"),
    }
    assert_eq!(view.as_bytes(), &input);
    Ok(())
}

#[test]
fn enum_builder_writes_known_and_unknown_variants() -> TestResult {
    let mut first = [0u8; 3];
    Foo::builder(&mut first[..])
        .first(|bar| bar.value(7))?
        .finish()?;
    assert_eq!(first, [0x11, 0x22, 7]);

    let mut unknown = [0u8; 6];
    Foo::builder(&mut unknown[..])
        .unknown(0x5566, &[1, 2, 3, 4][..])?
        .finish()?;
    assert_eq!(unknown, [0x55, 0x66, 1, 2, 3, 4]);
    Ok(())
}

#[test]
fn enum_builders_compose_as_detached_nested_values() -> TestResult {
    let mut known = [0u8; 5];
    Envelope::<Foo>::builder(&mut known[..])
        .value(|foo| foo.first(|bar| bar.value(7)))?
        .tail(9)?
        .finish()?;
    assert_eq!(known, [3, 0x11, 0x22, 7, 9]);

    let mut unknown = [0u8; 7];
    Envelope::<Foo>::builder(&mut unknown[..])
        .value(|foo| foo.unknown(0x5566, &[1, 2, 3][..]))?
        .tail(9)?
        .finish()?;
    assert_eq!(unknown, [5, 0x55, 0x66, 1, 2, 3, 9]);
    Ok(())
}

#[test]
fn bounded_unknown_enum_preserves_the_following_parent_field() -> TestResult {
    let view = Bounded::<Foo>::view([5, 0x55, 0x66, 1, 2, 3, 9])?;
    match view.value().variant() {
        FooVariant::Unknown { selector, body } => {
            assert_eq!(selector, 0x5566);
            assert_eq!(body, &[1, 2, 3]);
        }
        FooVariant::First(_) | FooVariant::Second(_) => panic!("known variant"),
    }
    assert_eq!(view.tail(), 9);
    Ok(())
}

#[test]
fn closed_enum_rejects_unknown_selector_with_absolute_offset() {
    let error = match Closed::view([7, 0]) {
        Ok(_) => panic!("unknown selector unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClosedViewError::UnknownSelector {
            selector: 7,
            offset: 0,
        }
    ));
    assert_eq!(<Closed as wire_repr::WireView>::FIXED_SIZE, Some(2),);
}

#[test]
fn enum_derives_preserve_generic_variant_bodies() -> TestResult {
    let view = Generic::<Bar>::view([1, 7])?;
    match view.variant() {
        GenericVariant::First(value) => assert_eq!(value.value(), 7),
        GenericVariant::Unknown { .. } => panic!("unknown variant"),
    }

    let mut output = [0u8; 2];
    Generic::<Bar>::builder(&mut output[..])
        .first(|bar| bar.value(7))?
        .finish()?;
    assert_eq!(output, [1, 7]);
    Ok(())
}

#[test]
fn enum_selector_values_resolve_in_generic_impl_context() -> TestResult {
    let view = Associated::<Bar>::view([3, 7])?;
    match view.variant() {
        AssociatedVariant::First(value) => assert_eq!(value.value(), 7),
        AssociatedVariant::Unknown { .. } => panic!("unknown variant"),
    }
    Ok(())
}

#[test]
fn generic_duplicate_selectors_are_rejected_when_framing() {
    let error = match Duplicate::<Bar>::view([3, 7]) {
        Ok(_) => panic!("duplicate selector schema unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(error, DuplicateViewError::DuplicateSelector));
}

#[test]
fn normalized_variant_method_collisions_receive_stable_suffixes() -> TestResult {
    let mut first = [0u8; 2];
    Collision::builder(&mut first[..])
        .foo_bar(|bar| bar.value(7))?
        .finish()?;
    assert_eq!(first, [1, 7]);

    let mut second = [0u8; 2];
    Collision::builder(&mut second[..])
        .foo_bar_2(|bar| bar.value(7))?
        .finish()?;
    assert_eq!(second, [2, 7]);

    let mut reserved = [0u8; 2];
    Collision::builder(&mut reserved[..])
        .unknown_2(|bar| bar.value(7))?
        .finish()?;
    assert_eq!(reserved, [3, 7]);
    Ok(())
}

#[test]
fn exact_unknown_view_forwards_without_reconstruction() -> TestResult {
    #[derive(WireBuilder)]
    struct Envelope<T> {
        count: u8,
        #[wire(counted_by = count)]
        values: wire_repr::wire::Array<T>,
    }

    let source = Foo::view([0x55, 0x66, 1, 2, 3, 4])?;
    let mut output = [0u8; 7];
    Envelope::<Foo>::builder(&mut output[..])
        .values(|values| values.item_view(source))?
        .finish()?;
    assert_eq!(output, [1, 0x55, 0x66, 1, 2, 3, 4]);
    Ok(())
}

mod unit_variants {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Body {
        value: u8,
    }

    #[derive(WireView, WireBuilder)]
    #[wire(selector = u8)]
    enum Command {
        #[wire(value = 0)]
        Ping,
        #[wire(value = 1)]
        Write(Body),
    }

    #[derive(WireView, WireBuilder)]
    #[wire(selector = u8)]
    enum AllUnit {
        #[wire(value = 0)]
        Off,
        #[wire(value = 1)]
        On,
    }

    #[derive(WireView)]
    #[wire(selector = u8)]
    enum AllUnitUnknown {
        #[wire(value = 0)]
        Off,
        #[wire(unknown)]
        Unknown(wire_repr::wire::Bytes),
    }
    #[test]
    fn unit_variants_have_selector_only_views_and_writers() -> TestResult {
        let ping = Command::view([0])?;
        assert!(matches!(ping.variant(), CommandVariant::Ping));

        let write = Command::view([1, 7])?;
        assert!(matches!(
            write.variant(),
            CommandVariant::Write(body) if body.value() == 7
        ));

        let mut output = [0xff; 1];
        Command::builder(&mut output[..]).ping()?.finish()?;
        assert_eq!(output, [0]);

        let mut output = [0xff; 2];
        Command::builder(&mut output[..])
            .write(|body| body.value(7))?
            .finish()?;
        assert_eq!(output, [1, 7]);
        let on = AllUnit::view([1])?;
        assert!(matches!(on.variant(), AllUnitVariant::On));
        let mut output = [0xff; 1];
        AllUnit::builder(&mut output[..]).off()?.finish()?;
        assert_eq!(output, [0]);

        let unknown = AllUnitUnknown::view([7, 1, 2])?;
        assert!(matches!(
            unknown.variant(),
            AllUnitUnknownVariant::Unknown {
                selector: 7,
                body: [1, 2],
            }
        ));
        Ok(())
    }
}
