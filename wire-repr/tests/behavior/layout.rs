#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod fixed_bytes {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo<const N: usize> {
        foo: [u8; N],
        #[wire(constant = *b"wire")]
        bar: [u8; 4],
        baz: u8,
    }

    #[test]
    fn const_generic_byte_arrays_round_trip_without_endian_attributes() -> TestResult {
        let input = [1, 2, 3, b'w', b'i', b'r', b'e', 9];
        let view = Foo::<3>::view(input)?;
        assert_eq!(view.foo(), [1, 2, 3]);
        assert_eq!(view.bar(), *b"wire");
        assert_eq!(view.baz(), 9);

        let mut output = [0u8; 8];
        Foo::<3>::builder(&mut output[..])
            .baz(9)?
            .foo([1, 2, 3])?
            .finish()?;
        assert_eq!(output, input);
        Ok(())
    }

    #[test]
    fn array_shortage_and_constant_mismatch_keep_field_context() {
        let error = match Foo::<3>::view([1, 2]) {
            Ok(_) => panic!("truncated byte array unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::Foo(_)));

        let error = match Foo::<3>::view([1, 2, 3, b'w', b'X', b'r', b'e', 9]) {
            Ok(_) => panic!("invalid byte-array constant unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::BarConstant(_)));
    }
    #[test]
    fn absolute_offset_overflow_is_a_typed_layout_error() {
        let error = match <Foo<3> as wire_repr::WireView>::frame(
            &[1, 2, 3, b'w', b'i', b'r', b'e', 9],
            usize::MAX,
        ) {
            Ok(_) => panic!("overflowing absolute offset unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            FooViewError::LayoutUnavailable { field: "foo" }
        ));
    }
}

mod multiple_children {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Bar {
        #[wire(le, constant = 0x1122)]
        tag: u16,
        value: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Baz {
        #[wire(be, constant = 0x3344)]
        tag: u16,
        value: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Foo<T, U> {
        header: u8,
        first: T,
        middle: [u8; 2],
        second: U,
        trailer: u8,
    }

    type FooBarBaz = Foo<Bar, Baz>;

    #[test]
    fn multiple_fixed_children_keep_declaration_order_across_setter_order() -> TestResult {
        let expected = [0xaa, 0x22, 0x11, 1, 2, 3, 0x33, 0x44, 4, 0xee];
        let mut output = [0u8; 10];
        FooBarBaz::builder(&mut output[..])
            .second(|baz| baz.value(4))?
            .trailer(0xee)?
            .middle([2, 3])?
            .header(0xaa)?
            .first(|bar| bar.value(1))?
            .finish()?;
        assert_eq!(output, expected);

        let view = FooBarBaz::view(output)?;
        assert_eq!(view.header(), 0xaa);
        assert_eq!(view.first().tag(), 0x1122);
        assert_eq!(view.first().value(), 1);
        assert_eq!(view.middle(), [2, 3]);
        assert_eq!(view.second().tag(), 0x3344);
        assert_eq!(view.second().value(), 4);
        assert_eq!(view.trailer(), 0xee);
        Ok(())
    }

    #[test]
    fn each_nested_child_keeps_its_own_parent_error_variant() {
        let mut first_invalid = [0xaa, 0x22, 0x10, 1, 2, 3, 0x33, 0x44, 4, 0xee];
        let error = match FooBarBaz::view(first_invalid) {
            Ok(_) => panic!("invalid first child unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::First(_)));

        first_invalid[2] = 0x11;
        first_invalid[7] = 0x45;
        let error = match FooBarBaz::view(first_invalid) {
            Ok(_) => panic!("invalid second child unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::Second(_)));
    }
}

mod fixed_child_region {
    use super::*;

    struct Bar;

    impl wire_repr::WireBuilder for Bar {
        const FIXED_SIZE: Option<usize> = Some(1);
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<()> for Bar {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            writer.write(&[1, 2])?;
            Ok(())
        }
    }

    struct Short;

    impl wire_repr::WireBuilder for Short {
        const FIXED_SIZE: Option<usize> = Some(1);
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<()> for Short {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            Ok(())
        }
    }

    #[derive(WireBuilder)]
    struct ShortFoo {
        bar: Short,
        foo: u8,
    }
    #[derive(WireBuilder)]
    struct Foo {
        bar: Bar,
        foo: u8,
    }

    #[derive(WireBuilder)]
    struct TerminalFoo {
        bar: Bar,
    }

    #[derive(WireBuilder)]
    struct TerminalShortFoo {
        bar: Short,
    }

    #[test]
    fn fixed_child_cannot_overwrite_the_following_field_region() {
        let mut output = [0u8; 2];
        let error = match Foo::builder(&mut output[..]).bar(|()| ()) {
            Ok(_) => panic!("oversized fixed child unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Output(wire_repr::OutputError::ChildOverflow {
                end: 2,
                limit: 1,
            })
        ));
    }

    #[test]
    fn terminal_fixed_children_use_the_same_exact_region_contract() {
        let mut output = [0u8; 2];
        let overflow = match TerminalFoo::builder(&mut output[..]).bar(|()| ()) {
            Ok(_) => panic!("oversized terminal fixed child unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            overflow,
            wire_repr::WriteError::Output(wire_repr::OutputError::ChildOverflow {
                end: 2,
                limit: 1,
            })
        ));

        let incomplete = match TerminalShortFoo::builder(&mut output[..]).bar(|()| ()) {
            Ok(_) => panic!("undersized terminal fixed child unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            incomplete,
            wire_repr::WriteError::Output(wire_repr::OutputError::ChildIncomplete {
                end: 0,
                limit: 1,
            })
        ));
    }
    #[test]
    fn fixed_child_must_fill_its_complete_region() {
        let mut output = [0u8; 2];
        let error = match ShortFoo::builder(&mut output[..]).bar(|()| ()) {
            Ok(_) => panic!("undersized fixed child unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Output(wire_repr::OutputError::ChildIncomplete {
                end: 0,
                limit: 1,
            })
        ));
    }
}

mod layout_error_name_collision {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Bar {
        value: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Foo<T> {
        layout: T,
        value: u8,
    }

    #[test]
    fn layout_field_does_not_collide_with_generated_layout_error() -> TestResult {
        let mut output = [0u8; 2];
        Foo::<Bar>::builder(&mut output[..])
            .layout(|bar| bar.value(1))?
            .value(2)?
            .finish()?;
        let view = Foo::<Bar>::view(output)?;
        assert_eq!(view.layout().value(), 1);
        assert_eq!(view.value(), 2);
        Ok(())
    }
}

mod variable_nonterminal {
    use super::*;

    struct Bar;

    #[allow(unsafe_code)]
    unsafe impl wire_repr::WireView for Bar {
        type Error = core::convert::Infallible;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = None;

        fn frame(
            input: &[u8],
            _absolute_offset: usize,
        ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
            Ok(wire_repr::Frame::new((), input.len().min(1)))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view Self::State,
        ) -> Self::View<'view> {
            input
        }
    }

    impl wire_repr::WireBuilder for Bar {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<()> for Bar {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            writer.write(&[1])?;
            Ok(())
        }
    }

    #[derive(WireView, WireBuilder)]
    struct Foo {
        bar: Bar,
        foo: u8,
    }

    #[derive(WireBuilder)]
    struct Outer<T> {
        inner: T,
    }
    #[test]
    fn variable_nonterminal_geometry_is_an_error_not_a_panic() {
        assert!(matches!(
            Foo::view([1, 2]),
            Err(FooViewError::LayoutUnavailable { field: "bar" })
        ));

        let mut output = [0u8; 2];
        let error = match Foo::builder(&mut output[..]).bar(|()| ()) {
            Ok(_) => panic!("variable nonterminal child unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(FooWriteError::Layout(wire_repr::LayoutError {
                field: "bar",
            }))
        ));

        let mut nested_output = [0u8; 2];
        let nested_error = match Outer::<Foo>::builder(&mut nested_output[..])
            .inner(|foo| foo.bar(|()| ()).foo(2))
        {
            Ok(_) => panic!("detached builder bypassed nonterminal layout capability"),
            Err(error) => error,
        };
        assert!(matches!(
            nested_error,
            wire_repr::WriteError::Schema(OuterWriteError::Inner(FooWriteError::Layout(
                wire_repr::LayoutError { field: "bar" }
            )))
        ));
    }
}
