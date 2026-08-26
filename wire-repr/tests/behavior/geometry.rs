#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod raw_bytes {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        head: u8,
        #[wire(rest)]
        body: wire_repr::wire::Bytes,
    }

    #[test]
    fn terminal_rest_borrows_and_writes_the_exact_suffix() -> TestResult {
        let input = [7, 1, 2, 3];
        let view = Foo::view(&input[..])?;
        assert_eq!(view.head(), 7);
        assert_eq!(view.body(), &[1, 2, 3]);
        assert!(core::ptr::eq(view.body().as_ptr(), input[1..].as_ptr()));

        let mut output = [0u8; 4];
        Foo::builder(&mut output[..])
            .body(&[1, 2, 3][..])?
            .head(7)?
            .finish()?;
        assert_eq!(output, input);
        Ok(())
    }

    #[derive(WireView, WireBuilder)]
    struct Bar {
        head: u8,
        length: u8,
        #[wire(bytes = length)]
        body: wire_repr::wire::Bytes,
        tail: u8,
    }

    #[test]
    fn bounded_bytes_use_the_read_controller_and_patch_it_while_writing() -> TestResult {
        let input = [7, 3, 10, 11, 12, 9];
        let view = Bar::view(&input[..])?;
        assert_eq!(view.length(), 3);
        assert_eq!(view.body(), &[10, 11, 12]);
        assert_eq!(view.tail(), 9);
        assert!(core::ptr::eq(view.body().as_ptr(), input[2..5].as_ptr()));

        let mut output = [0u8; 6];
        Bar::builder(&mut output[..])
            .head(7)?
            .body(&[10, 11, 12][..])?
            .tail(9)?
            .finish()?;
        assert_eq!(output, input);
        Ok(())
    }

    #[test]
    fn bounded_bytes_report_the_payload_field_on_truncation() {
        let error = match Bar::view([7, 3, 10, 11]) {
            Ok(_) => panic!("truncated payload unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(error, BarViewError::Body(_)));
    }

    #[test]
    fn bounded_length_overflow_is_typed_and_never_wraps() {
        let body = [0u8; 256];
        let mut output = [0u8; 258];
        let writer = match Bar::builder(&mut output[..]).head(1) {
            Ok(writer) => writer,
            Err(error) => panic!("fixed head unexpectedly failed: {error}"),
        };
        let error = match writer.body(&body[..]) {
            Ok(_) => panic!("unrepresentable byte length unexpectedly wrote"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(BarWriteError::Layout(wire_repr::LayoutError {
                field: "length"
            }))
        ));
    }
}

mod bounded_child {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Bar {
        #[wire(be, constant = 0x1122)]
        tag: u16,
        value: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Foo<T> {
        length: u8,
        #[wire(bytes = length)]
        child: T,
        tail: u8,
    }

    #[derive(WireBuilder)]
    struct Baz<T> {
        value: T,
    }
    #[test]
    fn bounded_nested_child_must_consume_its_declared_extent() -> TestResult {
        let input = [3, 0x11, 0x22, 7, 9];
        let view = Foo::<Bar>::view(input)?;
        assert_eq!(view.length(), 3);
        assert_eq!(view.child().tag(), 0x1122);
        assert_eq!(view.child().value(), 7);
        assert_eq!(view.tail(), 9);

        let mut output = [0u8; 5];
        Foo::<Bar>::builder(&mut output[..])
            .child(|bar| bar.value(7))?
            .tail(9)?
            .finish()?;
        assert_eq!(output, input);
        Ok(())
    }

    #[test]
    fn bounded_nested_controller_is_patched_through_detached_composition() -> TestResult {
        let expected = [3, 0x11, 0x22, 7, 9];
        let mut output = [0u8; 5];
        Baz::<Foo<Bar>>::builder(&mut output[..])
            .value(|foo| foo.child(|bar| bar.value(7)).tail(9))?
            .finish()?;
        assert_eq!(output, expected);
        Ok(())
    }
}

mod gaps {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        head: u8,
        #[wire(be, pad_before = 2, align_before = 4)]
        tail: u16,
    }

    #[test]
    fn padding_is_opaque_on_read_and_zeroed_on_fresh_write() -> TestResult {
        let input = [7, 0xaa, 0xbb, 0xcc, 0x12, 0x34];
        let view = Foo::view(input)?;
        assert_eq!(view.head(), 7);
        assert_eq!(view.tail(), 0x1234);

        let mut output = [0xa5; 6];
        Foo::builder(&mut output[..])
            .tail(0x1234)?
            .head(7)?
            .finish()?;
        assert_eq!(output, [7, 0, 0, 0, 0x12, 0x34]);
        Ok(())
    }

    #[test]
    fn padding_shortage_keeps_the_later_field_site() {
        let error = match Foo::view([7]) {
            Ok(_) => panic!("truncated padded representation unexpectedly framed"),
            Err(error) => error,
        };

        assert!(matches!(error, FooViewError::Tail(_)));
    }
    #[derive(WireView, WireBuilder)]
    struct Aligned {
        head: u8,
        #[wire(align_before = 4)]
        tail: u8,
    }

    #[derive(WireView, WireBuilder)]
    struct Outer<T> {
        prefix: u8,
        child: T,
    }

    #[test]
    fn detached_alignment_is_relative_to_the_child_representation() -> TestResult {
        let expected = [9, 7, 0, 0, 0, 8];
        let mut output = [0xa5; 6];
        Outer::<Aligned>::builder(&mut output[..])
            .prefix(9)?
            .child(|child| child.head(7).tail(8))?
            .finish()?;
        assert_eq!(output, expected);

        let view = Outer::<Aligned>::view(output)?;
        assert_eq!(view.child().head(), 7);
        assert_eq!(view.child().tail(), 8);
        Ok(())
    }
    #[derive(WireView, WireBuilder)]
    struct Dynamic {
        length: u8,
        #[wire(bytes = length)]
        body: wire_repr::wire::Bytes,
        #[wire(align_before = 4)]
        tail: u8,
    }

    #[test]
    fn alignment_uses_the_runtime_end_of_the_previous_field() -> TestResult {
        let input = [2, 1, 2, 0xaa, 8];
        let view = Dynamic::view(input)?;
        assert_eq!(view.body(), &[1, 2]);
        assert_eq!(view.tail(), 8);

        let mut output = [0xa5; 5];
        Dynamic::builder(&mut output[..])
            .body(&[1, 2][..])?
            .tail(8)?
            .finish()?;
        assert_eq!(output, [2, 1, 2, 0, 8]);
        Ok(())
    }
}

mod placement {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        offset: u8,
        length: u8,
        lead: u8,
        #[wire(at = offset, bytes = length)]
        body: wire_repr::wire::Bytes,
        tail: u8,
    }

    #[test]
    fn forward_placement_uses_the_encoded_read_offset_and_zero_fills_writes() -> TestResult {
        let input = [6, 2, 8, 0xaa, 0xbb, 0xcc, 4, 5, 7];
        let view = Foo::view(input)?;
        assert_eq!(view.offset(), 6);
        assert_eq!(view.length(), 2);
        assert_eq!(view.body(), &[4, 5]);
        assert_eq!(view.tail(), 7);

        let mut output = [0xa5; 9];
        Foo::builder(&mut output[..])
            .offset(6)?
            .lead(8)?
            .body(&[4, 5][..])?
            .tail(7)?
            .finish()?;
        assert_eq!(output, [6, 2, 8, 0, 0, 0, 4, 5, 7]);
        Ok(())
    }

    #[test]
    fn backward_dynamic_placement_is_a_typed_geometry_error() {
        let error = match Foo::view([2, 1, 9, 7]) {
            Ok(_) => panic!("backward placement unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            FooViewError::PositionBeforeCursor {
                field: "body",
                position: 2,
                cursor: 3,
            }
        ));
    }

    #[derive(WireView, WireBuilder)]
    struct PositionedRest {
        head: u8,
        #[wire(at = 4, rest)]
        body: wire_repr::wire::Bytes,
    }

    #[test]
    fn positioned_rest_reports_the_exact_shortage_to_its_start() {
        let error = match PositionedRest::view([1]) {
            Ok(_) => panic!("unreached positioned rest unexpectedly framed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            PositionedRestViewError::Body(wire_repr::NeedMore {
                offset: 1,
                additional_at_least: 3,
            })
        ));
    }
    #[derive(WireView, WireBuilder)]
    struct ConstantGeometry {
        head: u8,
        #[wire(at = 4, constant = 0xaa)]
        tag: u8,
        body: u8,
    }

    #[test]
    fn positioned_constant_is_emitted_before_the_following_setter() -> TestResult {
        let expected = [1, 0, 0, 0, 0xaa, 2];
        let mut output = [0xa5; 6];
        ConstantGeometry::builder(&mut output[..])
            .head(1)?
            .body(2)?
            .finish()?;
        assert_eq!(output, expected);
        let view = ConstantGeometry::view(output)?;
        assert_eq!(view.tag(), 0xaa);
        assert_eq!(view.body(), 2);
        Ok(())
    }
    #[derive(WireView, WireBuilder)]
    struct Static {
        lead: u8,
        #[wire(at = 4, be)]
        value: u16,
        tail: u8,
    }

    #[test]
    fn static_forward_placement_has_no_runtime_descriptor_state() -> TestResult {
        let input = [1, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 2];
        let view = Static::view(input)?;
        assert_eq!(view.value(), 0x1234);

        let mut output = [0xa5; 7];
        Static::builder(&mut output[..])
            .lead(1)?
            .value(0x1234)?
            .tail(2)?
            .finish()?;
        assert_eq!(output, [1, 0, 0, 0, 0x12, 0x34, 2]);
        Ok(())
    }
}
