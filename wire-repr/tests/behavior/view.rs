#![allow(dead_code)]

use wire_repr::{ConstantMismatch, NeedMore, TrailingBytes, WireView};

mod exact {
    use super::*;

    #[derive(WireView)]
    struct Bar {
        #[wire(le, constant = 0x4433_2211)]
        foo: u32,
    }

    #[derive(WireView)]
    struct Foo<T> {
        foo: u8,
        #[wire(be)]
        bar: u16,
        baz: T,
    }

    type FooBar = Foo<Bar>;

    const BYTES: [u8; 7] = [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44];

    #[test]
    fn exact_view_decodes_fixed_and_nested_fields() {
        let foo = FooBar::view(BYTES).unwrap();

        assert_eq!(foo.as_bytes(), BYTES);
        assert_eq!(foo.foo(), 0xaa);
        assert_eq!(foo.bar(), 0x1234);
        assert_eq!(foo.baz().foo(), 0x4433_2211);
        assert_eq!(foo.baz().as_bytes(), &BYTES[3..]);
    }

    #[test]
    fn exact_view_reports_field_site_and_absolute_offset() {
        let error = FooBar::view(&BYTES[..2]).err().unwrap();
        match error {
            FooViewError::Bar(NeedMore {
                offset,
                additional_at_least,
            }) => {
                assert_eq!(offset, 2);
                assert_eq!(additional_at_least, 1);
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let mut wrong = BYTES;
        wrong[3] ^= 1;
        let error = FooBar::view(wrong).err().unwrap();
        match error {
            FooViewError::Baz(BarViewError::FooConstant(ConstantMismatch {
                offset,
                expected,
                actual,
            })) => {
                assert_eq!(offset, 3);
                assert_eq!(expected, 0x4433_2211);
                assert_ne!(actual, expected);
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let mut trailing = BYTES.to_vec();
        trailing.push(0xff);
        let error = FooBar::view(trailing).err().unwrap();
        match error {
            FooViewError::Trailing(TrailingBytes { offset, trailing }) => {
                assert_eq!(offset, BYTES.len());
                assert_eq!(trailing, 1);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        for end in 0..BYTES.len() {
            assert!(FooBar::view(&BYTES[..end]).is_err(), "accepted {end} bytes");
        }
    }
}

mod validation {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
    #[error("foo")]
    struct FooError;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
    #[error("bar")]
    struct BarError;

    #[wire_repr::validator]
    fn foo(view: &impl FooView) -> Result<(), FooError> {
        (view.foo() != 0).then_some(()).ok_or(FooError)
    }

    #[wire_repr::validator]
    fn bar(view: &impl FooView) -> Result<(), BarError> {
        (view.foo() != 7).then_some(()).ok_or(BarError)
    }

    #[derive(WireView)]
    #[wire(validate = foo, validate = bar)]
    struct Foo {
        foo: u8,
    }

    #[test]
    fn validators_run_in_declaration_order_before_trailing_rejection() {
        assert!(matches!(Foo::view([0]), Err(FooViewError::Foo2(FooError))));
        assert!(matches!(Foo::view([7]), Err(FooViewError::Bar(BarError))));
        assert!(matches!(
            Foo::view([0, 1]),
            Err(FooViewError::Foo2(FooError))
        ));
        assert!(matches!(
            Foo::view([1, 2]),
            Err(FooViewError::Trailing(TrailingBytes {
                offset: 1,
                trailing: 1,
            }))
        ));
    }
}

mod manual {
    use super::*;

    struct Bar;

    #[allow(unsafe_code)]
    unsafe impl wire_repr::WireView for Bar {
        type Error = NeedMore;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = Some(1);

        fn frame(
            input: &[u8],
            absolute_offset: usize,
        ) -> Result<wire_repr::Frame<()>, Self::Error> {
            if input.is_empty() {
                Err(NeedMore {
                    offset: absolute_offset,
                    additional_at_least: 1,
                })
            } else {
                Ok(wire_repr::Frame::new((), input.len() + 1))
            }
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view (),
        ) -> Self::View<'view> {
            input
        }
    }

    #[derive(WireView)]
    struct Foo {
        bar: Bar,
    }

    #[test]
    fn generated_parent_rejects_invalid_manual_extent() {
        assert!(matches!(
            Foo::view([1]),
            Err(FooViewError::InvalidFrame(error))
                if error.offset == 0 && error.consumed == 2 && error.available == 1
        ));
    }
}
