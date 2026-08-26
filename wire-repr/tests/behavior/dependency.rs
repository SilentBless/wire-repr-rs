#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod shared_length {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        length: u8,
        #[wire(bytes = length)]
        first: wire_repr::wire::Bytes,
        #[wire(bytes = length)]
        second: wire_repr::wire::Bytes,
        tail: u8,
    }

    #[test]
    fn one_controller_can_govern_multiple_equal_payloads() -> TestResult {
        let input = [2, 1, 2, 3, 4, 9];
        let view = Foo::view(input)?;
        assert_eq!(view.length(), 2);
        assert_eq!(view.first(), &[1, 2]);
        assert_eq!(view.second(), &[3, 4]);
        assert_eq!(view.tail(), 9);

        let mut output = [0u8; 6];
        Foo::builder(&mut output[..])
            .first(&[1, 2][..])?
            .second(&[3, 4][..])?
            .tail(9)?
            .finish()?;
        assert_eq!(output, input);
        Ok(())
    }

    #[test]
    fn conflicting_payload_intent_is_a_typed_controller_error() {
        let mut output = [0u8; 6];
        let writer = match Foo::builder(&mut output[..]).first(&[1, 2][..]) {
            Ok(writer) => writer,
            Err(error) => panic!("first payload unexpectedly failed: {error}"),
        };
        let error = match writer.second(&[3][..]) {
            Ok(_) => panic!("conflicting controller values unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(FooWriteError::LengthConflict {
                controller: "length",
                expected: 2,
                actual: 1,
            })
        ));
    }

    #[derive(WireView, WireBuilder)]
    struct Bar {
        first: u8,
        second: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Mixed<T> {
        length: u8,
        #[wire(bytes = length)]
        raw: wire_repr::wire::Bytes,
        #[wire(bytes = length)]
        child: T,
        tail: u8,
    }

    #[derive(WireBuilder)]
    struct Outer<T> {
        value: T,
    }

    #[test]
    fn raw_and_nested_payloads_share_one_controller_in_root_and_detached_writers() -> TestResult {
        let expected = [2, 10, 11, 1, 2, 9];
        let mut root = [0u8; 6];
        Mixed::<Bar>::builder(&mut root[..])
            .raw(&[10, 11][..])?
            .child(|bar| bar.first(1).second(2))?
            .tail(9)?
            .finish()?;
        assert_eq!(root, expected);

        let mut detached = [0u8; 6];
        Outer::<Mixed<Bar>>::builder(&mut detached[..])
            .value(|mixed| {
                mixed
                    .raw(&[10, 11][..])
                    .child(|bar| bar.first(1).second(2))
                    .tail(9)
            })?
            .finish()?;
        assert_eq!(detached, expected);
        Ok(())
    }

    #[test]
    fn mixed_payload_length_conflict_is_rejected_at_the_second_payload() {
        let mut output = [0u8; 6];
        let writer = match Mixed::<Bar>::builder(&mut output[..]).raw(&[10][..]) {
            Ok(writer) => writer,
            Err(error) => panic!("raw payload unexpectedly failed: {error}"),
        };
        let error = match writer.child(|bar| bar.first(1).second(2)) {
            Ok(_) => panic!("mixed controller conflict unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(MixedWriteError::LengthConflict {
                controller: "length",
                expected: 1,
                actual: 2,
            })
        ));
    }
}

mod conditional_group {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        #[wire(as = u8)]
        present: bool,
        #[wire(flag = present)]
        details: bool,
        #[wire(depends_on = details)]
        first: u8,
        #[wire(be, depends_on = details)]
        second: u16,
        tail: u8,
    }

    #[derive(WireBuilder)]
    struct Outer<T> {
        value: T,
    }

    #[test]
    fn read_controller_is_authoritative_for_group_presence() -> TestResult {
        let present = Foo::view([1, 7, 0x12, 0x34, 9])?;
        assert!(present.present());
        assert!(present.details());
        assert_eq!(present.first(), Some(7));
        assert_eq!(present.second(), Some(0x1234));
        assert_eq!(present.tail(), 9);

        let absent = Foo::view([0, 9])?;
        assert!(!absent.present());
        assert!(!absent.details());
        assert_eq!(absent.first(), None);
        assert_eq!(absent.second(), None);
        assert_eq!(absent.tail(), 9);
        Ok(())
    }

    #[test]
    fn present_group_truncation_keeps_the_dependent_field_site() {
        let error = match Foo::view([1, 7, 0x12]) {
            Ok(_) => panic!("truncated present group unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::Second(_)));
    }

    #[test]
    fn invalid_physical_presence_controller_is_rejected_before_group_geometry() {
        let error = match Foo::view([2, 9]) {
            Ok(_) => panic!("invalid bool controller unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, FooViewError::PresentValue(_)));
    }

    #[test]
    fn choice_closure_patches_controller_and_writes_one_coherent_group() -> TestResult {
        let mut present_output = [0u8; 5];
        Foo::builder(&mut present_output[..])
            .details(|details| details.present(|details| details.first(7).second(0x1234)))?
            .tail(9)?
            .finish()?;
        assert_eq!(present_output, [1, 7, 0x12, 0x34, 9]);

        let mut absent_output = [0u8; 2];
        Foo::builder(&mut absent_output[..])
            .details(|details| details.absent())?
            .tail(9)?
            .finish()?;
        assert_eq!(absent_output, [0, 9]);
        Ok(())
    }

    #[test]
    fn conditional_group_composes_through_detached_wire_write() -> TestResult {
        let mut output = [0u8; 5];
        Outer::<Foo>::builder(&mut output[..])
            .value(|foo| {
                foo.details(|details| details.present(|details| details.first(7).second(0x1234)))
                    .tail(9)
            })?
            .finish()?;
        assert_eq!(output, [1, 7, 0x12, 0x34, 9]);
        Ok(())
    }
}
