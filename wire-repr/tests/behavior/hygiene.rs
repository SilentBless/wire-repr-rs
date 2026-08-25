#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Foo {
    __wire_repr_schema: u8,
    foo_1bar: u8,
    foo1bar: u8,
    r#type: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bar")]
struct BarError;

#[wire_repr::validator]
fn r#type(view: &impl BarView) -> Result<(), BarError> {
    (view.foo() != 0).then_some(()).ok_or(BarError)
}

#[derive(WireView)]
#[wire(validate = r#type)]
struct Bar {
    foo: u8,
}

#[test]
fn generated_names_do_not_restrict_valid_rust_identifiers() -> TestResult {
    let foo = Foo::view([1, 2, 3, 4])?;
    assert_eq!(foo.__wire_repr_schema(), 1);
    assert_eq!(foo.foo_1bar(), 2);
    assert_eq!(foo.foo1bar(), 3);
    assert_eq!(foo.r#type(), 4);

    let mut output = [0u8; 4];
    Foo::builder(&mut output[..])
        .__wire_repr_schema(1)?
        .foo_1bar(2)?
        .foo1bar(3)?
        .r#type(4)?
        .finish()?;
    assert_eq!(output, [1, 2, 3, 4]);

    assert!(matches!(Bar::view([0]), Err(BarViewError::Type(BarError))));
    Ok(())
}

mod generics {
    use core::marker::PhantomData;

    use wire_repr::{WireBuilder, WireView};

    use super::TestResult;

    struct Bar<'foo, T, const N: usize>(PhantomData<&'foo [T; N]>);

    #[allow(unsafe_code)]
    unsafe impl<'foo, T: 'foo, const N: usize> wire_repr::WireView for Bar<'foo, T, N> {
        type Error = core::convert::Infallible;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = Some(0);

        fn frame(
            _input: &[u8],
            _absolute_offset: usize,
        ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
            Ok(wire_repr::Frame::new((), 0))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view Self::State,
        ) -> Self::View<'view> {
            input
        }
    }

    impl<'foo, T: 'foo, const N: usize> wire_repr::WireBuilder for Bar<'foo, T, N> {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl<'foo, T: 'foo, const N: usize> wire_repr::WireWrite<()> for Bar<'foo, T, N> {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            Ok(())
        }
    }

    #[derive(WireView, WireBuilder)]
    struct Foo<'foo, T, const N: usize>
    where
        T: 'foo,
    {
        foo: u8,
        bar: Bar<'foo, T, N>,
    }

    #[test]
    fn derives_preserve_lifetime_type_const_and_where_generics() -> TestResult {
        type FooBar<'foo> = Foo<'foo, u16, 2>;

        let foo = FooBar::view([7])?;
        assert_eq!(foo.foo(), 7);
        assert!(foo.bar().is_empty());

        let mut output = [0u8; 1];
        FooBar::builder(&mut output[..])
            .foo(7)?
            .bar(|()| ())?
            .finish()?;
        assert_eq!(output, [7]);
        Ok(())
    }
}

mod defaults {
    use core::marker::PhantomData;

    use wire_repr::WireBuilder;

    use super::TestResult;

    struct Child<T, const N: usize>(PhantomData<(T, [(); N])>);

    impl<T, const N: usize> wire_repr::WireBuilder for Child<T, N> {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl<T, const N: usize> wire_repr::WireWrite<()> for Child<T, N> {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            Ok(())
        }
    }

    #[derive(WireBuilder)]
    struct Foo<T = u16, const N: usize = 2> {
        foo: u8,
        child: Child<T, N>,
    }

    #[test]
    fn generated_writer_preserves_type_and_const_defaults() -> TestResult {
        type DefaultFoo = Foo;

        let mut output = [0u8; 1];
        DefaultFoo::builder(&mut output[..])
            .foo(7)?
            .child(|()| ())?
            .finish()?;
        assert_eq!(output, [7]);
        Ok(())
    }
}

mod method_generic_collision {
    use core::marker::PhantomData;

    use wire_repr::WireBuilder;

    use super::TestResult;

    struct Child<T>(PhantomData<T>);

    impl<T> wire_repr::WireBuilder for Child<T> {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl<T> wire_repr::WireWrite<()> for Child<T> {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            _value: (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            Ok(())
        }
    }

    #[derive(WireBuilder)]
    struct Foo<__WireReprOutput> {
        foo: u8,
        child: Child<__WireReprOutput>,
    }

    #[test]
    fn manual_method_output_generic_cannot_collide_with_schema_generic() -> TestResult {
        let mut output = [0u8; 1];
        Foo::<u16>::builder(&mut output[..])
            .foo(9)?
            .child(|()| ())?
            .finish()?;
        assert_eq!(output, [9]);
        Ok(())
    }
}
