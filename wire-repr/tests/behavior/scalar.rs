#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod little {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        foo: u8,
        bar: i8,
        #[wire(le)]
        baz: u16,
        #[wire(le)]
        qux: i16,
        #[wire(le)]
        quux: u32,
        #[wire(le)]
        corge: i32,
        #[wire(le)]
        grault: u64,
        #[wire(le)]
        garply: i64,
        #[wire(le)]
        waldo: u128,
        #[wire(le)]
        fred: i128,
        #[wire(le)]
        plugh: f32,
        #[wire(le)]
        xyzzy: f64,
    }

    const BYTES: [u8; 74] = [
        0x12, 0xfe, 0x56, 0x34, 0xcc, 0xed, 0xde, 0xbc, 0x9a, 0x78, 0x88, 0xa9, 0xcb, 0xed, 0xef,
        0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11,
        0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0,
    ];

    #[test]
    fn every_primitive_round_trips_little_endian() -> TestResult {
        let foo = Foo::view(BYTES)?;
        assert_eq!(foo.foo(), 0x12);
        assert_eq!(foo.bar(), -2);
        assert_eq!(foo.baz(), 0x3456);
        assert_eq!(foo.qux(), -0x1234);
        assert_eq!(foo.quux(), 0x789a_bcde);
        assert_eq!(foo.corge(), -0x1234_5678);
        assert_eq!(foo.grault(), 0x0123_4567_89ab_cdef);
        assert_eq!(foo.garply(), -2);
        assert_eq!(foo.waldo(), 0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        assert_eq!(foo.fred(), -1);
        assert_eq!(foo.plugh(), 1.0);
        assert_eq!(foo.xyzzy(), -2.0);

        let mut output = [0u8; 74];
        Foo::builder(&mut output[..])
            .foo(0x12)?
            .bar(-2)?
            .baz(0x3456)?
            .qux(-0x1234)?
            .quux(0x789a_bcde)?
            .corge(-0x1234_5678)?
            .grault(0x0123_4567_89ab_cdef)?
            .garply(-2)?
            .waldo(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff)?
            .fred(-1)?
            .plugh(1.0)?
            .xyzzy(-2.0)?
            .finish()?;
        assert_eq!(output, BYTES);
        Ok(())
    }
}

mod big {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Bar {
        #[wire(be)]
        foo: u16,
        #[wire(be)]
        bar: i16,
        #[wire(be)]
        baz: u32,
        #[wire(be)]
        qux: i32,
        #[wire(be)]
        quux: u64,
        #[wire(be)]
        corge: i64,
        #[wire(be)]
        grault: u128,
        #[wire(be)]
        garply: i128,
        #[wire(be)]
        waldo: f32,
        #[wire(be)]
        fred: f64,
    }

    const BYTES: [u8; 72] = [
        0x34, 0x56, 0xed, 0xcc, 0x78, 0x9a, 0xbc, 0xde, 0xed, 0xcb, 0xa9, 0x88, 0x01, 0x23, 0x45,
        0x67, 0x89, 0xab, 0xcd, 0xef, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x00, 0x11,
        0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0x3f, 0x80, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn every_multibyte_primitive_round_trips_big_endian() -> TestResult {
        let bar = Bar::view(BYTES)?;
        assert_eq!(bar.foo(), 0x3456);
        assert_eq!(bar.bar(), -0x1234);
        assert_eq!(bar.baz(), 0x789a_bcde);
        assert_eq!(bar.qux(), -0x1234_5678);
        assert_eq!(bar.quux(), 0x0123_4567_89ab_cdef);
        assert_eq!(bar.corge(), -2);
        assert_eq!(bar.grault(), 0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        assert_eq!(bar.garply(), -1);
        assert_eq!(bar.waldo(), 1.0);
        assert_eq!(bar.fred(), -2.0);

        let mut output = [0u8; 72];
        Bar::builder(&mut output[..])
            .foo(0x3456)?
            .bar(-0x1234)?
            .baz(0x789a_bcde)?
            .qux(-0x1234_5678)?
            .quux(0x0123_4567_89ab_cdef)?
            .corge(-2)?
            .grault(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff)?
            .garply(-1)?
            .waldo(1.0)?
            .fred(-2.0)?
            .finish()?;
        assert_eq!(output, BYTES);
        Ok(())
    }
}

mod conversion {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Foo {
        #[wire(as = u16, le)]
        foo: usize,
        #[wire(as = i32, be)]
        bar: isize,
        #[wire(as = u8)]
        baz: bool,
        #[wire(as = u32, le)]
        qux: char,
    }

    const BYTES: [u8; 11] = [
        0x34, 0x12, 0xff, 0xff, 0xff, 0xfe, 1, 0xbb, 0x03, 0x00, 0x00,
    ];

    #[test]
    fn logical_scalars_use_checked_explicit_physical_widths() -> TestResult {
        let foo = Foo::view(BYTES)?;
        assert_eq!(foo.foo(), 0x1234);
        assert_eq!(foo.bar(), -2);
        assert!(foo.baz());
        assert_eq!(foo.qux(), 'λ');

        let mut output = [0u8; 11];
        Foo::builder(&mut output[..])
            .foo(0x1234)?
            .bar(-2)?
            .baz(true)?
            .qux('λ')?
            .finish()?;
        assert_eq!(output, BYTES);

        let mut invalid_bool = BYTES;
        invalid_bool[6] = 2;
        assert!(matches!(
            Foo::view(invalid_bool),
            Err(FooViewError::BazValue(_))
        ));

        let before = [0x55; 11];
        let mut output = before;
        let Err(error) = Foo::builder(&mut output[..]).foo(usize::from(u16::MAX) + 1) else {
            panic!("out-of-range logical scalar unexpectedly converted");
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(FooWriteError::FooValue(_))
        ));
        assert_eq!(output, before);

        let mut output = [0u8; 11];
        let writer = Foo::builder(&mut output[..]).foo(0)?;
        let Err(error) = writer.bar(isize::MAX) else {
            panic!("out-of-range second logical scalar unexpectedly converted");
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(FooWriteError::BarValue(_))
        ));
        Ok(())
    }
}

mod constant {
    use super::*;

    #[derive(WireView, WireBuilder)]
    struct Bar {
        #[wire(constant = 0xaa)]
        foo: u8,
        #[wire(le, constant = 0x1234)]
        bar: u16,
        #[wire(le, constant = 1.5)]
        baz: f32,
    }

    #[derive(WireBuilder)]
    struct Converted {
        #[wire(as = u8, constant = true)]
        flag: bool,
        #[wire(as = u32, le, constant = 'λ')]
        letter: char,
    }

    #[derive(WireBuilder)]
    struct TooWide {
        #[wire(as = u8, constant = 256usize)]
        foo: usize,
    }

    const BYTES: [u8; 7] = [0xaa, 0x34, 0x12, 0x00, 0x00, 0xc0, 0x3f];

    #[test]
    fn constants_share_scalar_encoding_and_have_no_builder_setters() -> TestResult {
        let bar = Bar::view(BYTES)?;
        assert_eq!(bar.foo(), 0xaa);
        assert_eq!(bar.bar(), 0x1234);
        assert_eq!(bar.baz(), 1.5);

        let mut output = [0u8; 7];
        Bar::builder(&mut output[..]).finish()?;
        assert_eq!(output, BYTES);
        Ok(())
    }

    #[test]
    fn converted_constants_use_checked_physical_representations() -> TestResult {
        let mut output = [0u8; 5];
        Converted::builder(&mut output[..]).finish()?;
        assert_eq!(output, [1, 0xbb, 0x03, 0x00, 0x00]);

        let mut output = [0u8; 1];
        let Err(error) = TooWide::builder(&mut output[..]).finish() else {
            panic!("out-of-range converted constant unexpectedly wrote");
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(TooWideWriteError::FooValue(_))
        ));
        Ok(())
    }
}
