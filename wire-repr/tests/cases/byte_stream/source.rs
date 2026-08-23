use super::*;

#[test]
fn byte_source_exact_write_rejects_wrong_emission_lengths() {
    assert!(
        std::panic::catch_unwind(|| {
            let mut output = [0; 2];
            UnderEmittingSource.write_into(&mut output);
        })
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| {
            let mut output = [0; 1];
            OverEmittingSource.write_into(&mut output);
        })
        .is_err()
    );
}

#[test]
fn byte_sources_emit_ordered_chunks_and_repeated_runs() {
    let source = ChunkedSource;
    let mut sink = RecordingSink {
        output: [0; 6],
        written: 0,
        fill_calls: 0,
    };

    source.emit_to(&mut sink);
    assert_eq!(sink.output, [1, 2, 0xa5, 0xa5, 0xa5, 9]);
    assert_eq!(sink.written, source.byte_len());
    assert_eq!(sink.fill_calls, 1);

    let mut output = [0; 6];
    source.write_into(&mut output);
    assert_eq!(output, sink.output);
}

#[test]
fn runtime_byte_source_chunks_bound_borrowed_and_repeated_spans() {
    let borrowed = [1, 2, 3];
    let source = codec::ByteChain::new(&borrowed[..], ByteSegment::Rest { byte: 0xa5, len: 5 });
    let chunks: Vec<_> = source.chunks(2).collect();
    assert!(chunks.iter().all(|chunk| chunk.len() <= 2));
    assert_eq!(
        chunks,
        [
            ByteSegment::Bytes(&[1, 2]),
            ByteSegment::Bytes(&[3]),
            ByteSegment::Rest { byte: 0xa5, len: 2 },
            ByteSegment::Rest { byte: 0xa5, len: 2 },
            ByteSegment::Rest { byte: 0xa5, len: 1 },
        ]
    );
    assert!(std::panic::catch_unwind(|| source.chunks(0)).is_err());

    let mut output = [0; 8];
    source.write_into(&mut output);
    assert_eq!(output, [1, 2, 3, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]);
}

#[test]
fn runtime_byte_source_cursors_preserve_spans_and_flatten_bytes() {
    let borrowed = [1, 2, 3];
    let source = codec::ByteChain::new(&borrowed[..], ByteSegment::Rest { byte: 0xa5, len: 5 });
    let segments: Vec<_> = source.segments().collect();
    assert_eq!(
        segments,
        [
            ByteSegment::Bytes(&borrowed),
            ByteSegment::Rest { byte: 0xa5, len: 5 },
        ]
    );
    assert_eq!(
        match segments[0] {
            ByteSegment::Bytes(bytes) => bytes.as_ptr(),
            ByteSegment::Rest { .. } => unreachable!(),
        },
        borrowed.as_ptr()
    );
    assert_eq!(segments[0].len(), 3);
    assert!(!segments[0].is_empty());
    assert_eq!(segments[1].bytes().collect::<Vec<_>>(), vec![0xa5; 5]);
    assert_eq!(
        source.bytes().collect::<Vec<_>>(),
        vec![1, 2, 3, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]
    );
}

#[test]
fn runtime_byte_source_ranges_remain_zero_copy_across_segments() {
    let borrowed = [1, 2, 3];
    let source = codec::ByteChain::new(&borrowed[..], ByteSegment::Rest { byte: 0xa5, len: 5 });
    let range = source.range(2..=5);
    let segments: Vec<_> = range.segments().collect();

    assert_eq!(
        segments,
        [
            ByteSegment::Bytes(&borrowed[2..]),
            ByteSegment::Rest { byte: 0xa5, len: 3 },
        ]
    );
    let ByteSegment::Bytes(bytes) = segments[0] else {
        unreachable!()
    };
    assert_eq!(bytes.as_ptr(), borrowed[2..].as_ptr());
    assert_eq!(range.bytes().collect::<Vec<_>>(), [3, 0xa5, 0xa5, 0xa5]);

    assert!(std::panic::catch_unwind(|| source.range(..9)).is_err());
    let reversed_start = std::hint::black_box(5);
    let reversed_end = std::hint::black_box(4);
    assert!(std::panic::catch_unwind(|| source.range(reversed_start..reversed_end)).is_err());
}
