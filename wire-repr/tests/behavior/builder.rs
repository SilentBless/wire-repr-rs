#![allow(dead_code)]

use core::convert::Infallible;
use core::ops::Range;

use bytes::BytesMut;
use wire_repr::{WireBuilder, output};

type TestResult = Result<(), Box<dyn std::error::Error>>;

mod derived {
    use super::*;

    #[derive(WireBuilder)]
    struct Bar {
        #[wire(le, constant = 0x4433_2211)]
        foo: u32,
    }

    #[derive(WireBuilder)]
    struct Foo<T> {
        foo: u8,
        #[wire(be)]
        bar: u16,
        baz: T,
    }

    type FooBar = Foo<Bar>;

    #[test]
    fn fixed_builder_writes_progressively_and_returns_the_represented_range() -> TestResult {
        let mut output = [0xcc; 9];
        {
            let mut written = FooBar::builder(&mut output[..])
                .foo(0xaa)?
                .bar(0x1234)?
                .baz(|bar| bar)?
                .finish()?;

            assert_eq!(written.range(), Range { start: 0, end: 7 });
            assert_eq!(
                written.as_bytes(),
                [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
            );
            written.as_bytes_mut()[0] = 0xab;
        }
        assert_eq!(
            output,
            [0xab, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44, 0xcc, 0xcc]
        );
        Ok(())
    }

    #[test]
    fn setter_order_does_not_change_physical_order_across_growth() -> TestResult {
        let mut output = Vec::new();
        let written = FooBar::builder(&mut output)
            .baz(|bar| bar)?
            .bar(0x1234)?
            .foo(0xaa)?
            .finish()?;

        assert_eq!(
            written.as_bytes(),
            [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        Ok(())
    }

    #[test]
    fn public_writer_cannot_create_an_out_of_bounds_or_backwards_child_range() -> TestResult {
        let mut output = [0u8; 1];
        let mut writer = wire_repr::Writer::new(&mut output[..]);
        let Err(error) = writer.child_at(2) else {
            panic!("out-of-bounds child cursor unexpectedly succeeded");
        };
        assert_eq!(error.to_string(), "output needs 2 bytes, 1 available");

        writer.write(&[1])?;
        let Err(error) = writer.child_at(0) else {
            panic!("backwards child cursor unexpectedly succeeded");
        };
        assert_eq!(
            error.to_string(),
            "nested output position 0 precedes written prefix 1"
        );
        assert_eq!(writer.finish().range(), 0..1);
        Ok(())
    }

    #[test]
    fn fixed_short_output_reports_need_more_and_keeps_partial_bytes() -> TestResult {
        let mut output = [0x55; 6];
        let builder = FooBar::builder(&mut output[..]).foo(0xaa)?.bar(0x1234)?;
        let Err(error) = builder.baz(|bar| bar) else {
            panic!("short fixed output unexpectedly accepted the nested child");
        };

        assert_eq!(error.to_string(), "output needs 7 bytes, 6 available");
        assert_eq!(output, [0xaa, 0x12, 0x34, 0x55, 0x55, 0x55]);
        Ok(())
    }

    #[test]
    fn vec_and_bytes_mut_grow_automatically_by_output_capability() -> TestResult {
        let mut vec = Vec::new();
        {
            let written = FooBar::builder(&mut vec)
                .foo(0xaa)?
                .bar(0x1234)?
                .baz(|bar| bar)?
                .finish()?;
            assert_eq!(written.range(), 0..7);
            assert_eq!(
                written.as_bytes(),
                [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
            );
        }
        assert!(vec.len() >= 7);

        let mut bytes = BytesMut::new();
        let written = FooBar::builder(&mut bytes)
            .foo(0xaa)?
            .bar(0x1234)?
            .baz(|bar| bar)?
            .finish()?;
        assert_eq!(written.range(), 0..7);
        assert_eq!(
            written.as_bytes(),
            [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        Ok(())
    }

    #[test]
    fn owned_output_moves_an_unfinished_writer_through_a_channel() -> TestResult {
        let writer = FooBar::builder(output::owned(Vec::new())).foo(0xaa)?;
        let (writer_tx, writer_rx) = std::sync::mpsc::sync_channel(1);
        writer_tx.send(writer).expect("worker stopped");
        let worker = std::thread::spawn(move || {
            let writer = writer_rx.recv().expect("writer channel closed");
            writer
                .bar(0x1234)
                .expect("bar write failed")
                .baz(|bar| bar)
                .expect("nested write failed")
        });
        let complete = worker.join().expect("writer worker panicked");
        let written = complete.finish()?;
        assert_eq!(written.range(), 0..7);
        let (owned, range) = written.into_parts();
        assert_eq!(range, 0..7);
        assert_eq!(
            &owned.as_ref()[range],
            &[0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        let vec = owned.into_inner();
        assert!(vec.len() >= 7);

        let written = FooBar::builder(output::owned(BytesMut::new()))
            .foo(0xaa)?
            .bar(0x1234)?
            .baz(|bar| bar)?
            .finish()?;
        let (owned, range) = written.into_parts();
        assert_eq!(
            &owned.as_ref()[range],
            &[0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        let bytes = owned.into_inner();
        assert!(bytes.len() >= 7);
        Ok(())
    }

    #[test]
    fn bounded_output_uses_existing_collection_storage_without_exceeding_limit() -> TestResult {
        let mut vec = Vec::with_capacity(16);
        let capacity = vec.capacity();
        let builder = FooBar::builder(output::bounded(&mut vec, 6))
            .foo(0xaa)?
            .bar(0x1234)?;
        let Err(error) = builder.baz(|bar| bar) else {
            panic!("bounded output unexpectedly exceeded its limit");
        };

        assert_eq!(error.to_string(), "output needs 7 bytes, limit is 6");
        assert_eq!(vec.capacity(), capacity);
        assert_eq!(&vec[..3], [0xaa, 0x12, 0x34]);
        Ok(())
    }

    #[test]
    fn caller_growth_callback_can_expand_without_allocating_in_wire_repr() -> TestResult {
        #[derive(Debug)]
        struct Window {
            bytes: [u8; 16],
            len: usize,
        }

        impl AsRef<[u8]> for Window {
            fn as_ref(&self) -> &[u8] {
                &self.bytes[..self.len]
            }
        }

        impl AsMut<[u8]> for Window {
            fn as_mut(&mut self) -> &mut [u8] {
                &mut self.bytes[..self.len]
            }
        }

        let mut window = Window {
            bytes: [0; 16],
            len: 0,
        };
        let mut growth_calls = 0usize;
        let output = output::grow_with(&mut window, |window, request| {
            growth_calls += 1;
            window.len = request.suggested_len.min(window.bytes.len());
            Ok::<_, Infallible>(())
        });
        let written = FooBar::builder(output)
            .foo(0xaa)?
            .bar(0x1234)?
            .baz(|bar| bar)?
            .finish()?;

        assert_eq!(
            written.as_bytes(),
            [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        assert!(growth_calls > 0);
        Ok(())
    }

    #[test]
    fn relocating_growth_preserves_the_high_water_mark() -> TestResult {
        struct Relocating {
            buffers: [[u8; 16]; 2],
            active: usize,
            len: usize,
        }

        impl AsRef<[u8]> for Relocating {
            fn as_ref(&self) -> &[u8] {
                &self.buffers[self.active][..self.len]
            }
        }

        impl AsMut<[u8]> for Relocating {
            fn as_mut(&mut self) -> &mut [u8] {
                &mut self.buffers[self.active][..self.len]
            }
        }

        let mut output = Relocating {
            buffers: [[0; 16]; 2],
            active: 0,
            len: 0,
        };
        let adapter = output::grow_with(&mut output, |output, request| {
            let mut prefix = [0u8; 16];
            prefix[..request.high_water_mark]
                .copy_from_slice(&output.as_ref()[..request.high_water_mark]);
            output.active ^= 1;
            output.len = request.suggested_len.min(16);
            output.as_mut()[..request.high_water_mark]
                .copy_from_slice(&prefix[..request.high_water_mark]);
            Ok::<_, Infallible>(())
        });
        let written = FooBar::builder(adapter)
            .foo(0xaa)?
            .bar(0x1234)?
            .baz(|bar| bar)?
            .finish()?;

        assert_eq!(
            written.as_bytes(),
            [0xaa, 0x12, 0x34, 0x11, 0x22, 0x33, 0x44]
        );
        Ok(())
    }

    #[test]
    fn caller_growth_failure_is_returned_at_the_field_that_needs_space() {
        let mut bytes = Vec::new();
        let output = output::grow_with(&mut bytes, |_output, _request| {
            Err(std::io::Error::other("pool empty"))
        });
        let Err(error) = FooBar::builder(output).foo(0xaa) else {
            panic!("failed caller growth unexpectedly wrote the first field");
        };

        assert_eq!(error.to_string(), "output growth failed: pool empty");
        use std::error::Error as _;
        let output_source = error
            .source()
            .expect("write error must expose output source");
        assert_eq!(
            output_source
                .source()
                .expect("output error must expose growth source")
                .to_string(),
            "pool empty"
        );
        assert!(bytes.is_empty());
    }
}

mod manual {
    use super::*;

    struct Bar;

    impl wire_repr::WireBuilder for Bar {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<u16> for Bar {
        type Error = Infallible;

        fn write<O: wire_repr::Output>(
            value: u16,
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            writer.write(&value.to_be_bytes())?;
            Ok(())
        }
    }

    #[derive(WireBuilder)]
    struct Foo {
        foo: u8,
        bar: Bar,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
    #[error("manual child failed")]
    struct ManualError;

    struct Failing;

    impl wire_repr::WireBuilder for Failing {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<()> for Failing {
        type Error = ManualError;

        fn write<O: wire_repr::Output>(
            _value: (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            Err(wire_repr::WriteError::Schema(ManualError))
        }
    }

    #[derive(WireBuilder)]
    struct Parent {
        prefix: u8,
        child: Failing,
    }

    #[test]
    fn generated_writer_composes_public_manual_capability() -> TestResult {
        let mut output = [0u8; 3];
        let written = Foo::builder(&mut output[..])
            .foo(1)?
            .bar(|()| 0x0203)?
            .finish()?;

        assert_eq!(written.as_bytes(), [1, 2, 3]);
        Ok(())
    }

    #[test]
    fn generated_writer_preserves_parent_field_context_for_nested_errors() -> TestResult {
        let mut output = [0u8; 1];
        let writer = Parent::builder(&mut output[..]).prefix(1)?;
        let Err(error) = writer.child(|()| ()) else {
            panic!("failing manual child unexpectedly succeeded");
        };

        assert!(matches!(
            error,
            wire_repr::WriteError::Schema(ParentWriteError::Child(ManualError))
        ));
        Ok(())
    }
}
