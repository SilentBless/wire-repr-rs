use wire_repr::wire_repr;

wire_repr! {
    pub layout Framed {
        tail @ 6: U8;
        align(4) @ 5;
        second @ 4: bytes(length);
        padding(1) @ 3;
        /// The first opaque range.
        first @ 2: bytes(length);
        length @ 1: U8;
    }

    pub layout EmptyRanges {
        tail @ 4: U8;
        second @ 3: bytes(length);
        first @ 2: bytes(length);
        length @ 1: U8;
    }

    pub layout AdjacentMutableRanges {
        first_length @ 1: U8;
        second_length @ 2: U8;
        first @ 3: bytes(first_length);
        second @ 4: bytes(second_length);
        tail @ 5: U8;
    }

    pub layout AbsoluteFramed {
        end @ 1: U8;
        payload @ 2: bytes_to(end);
        padding(1) @ 3;
        tail @ 4: U8;
    }

    pub layout WideLength {
        payload @ 2: bytes(length);
        length @ 1: BeU128;
    }

    pub layout ShortFramed {
        payload @ 2: bytes(length);
        length @ 1: U8;
    }
}

#[test]
fn fixed_lengths_frame_exact_ranges_and_preserve_caller_suffix() {
    let noncanonical = [2, b'a', b'b', 0xee, b'c', b'd', 0xff, 0xf0, 9, 0xaa];
    let (view, suffix) = Framed::view(&noncanonical).with_remainder().unwrap();

    assert_eq!(view.as_bytes(), &noncanonical[..9]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.length(), 2);
    assert_eq!(view.first(), b"ab");
    assert_eq!(view.second(), b"cd");
    assert_eq!(view.tail(), 9);

    let canonical = [2, b'a', b'b', 0xee, b'c', b'd', 0xf0, 0xf1, 9];
    let view = Framed::view(&canonical).without_trailing().unwrap();
    assert_eq!(view.length(), 2);
    assert_eq!(view.first(), b"ab");
    assert_eq!(view.second(), b"cd");
    assert_eq!(view.tail(), 9);
}

#[test]
fn adjacent_zero_length_ranges_are_exact_and_do_not_stall_physical_progress() {
    let bytes = [0, 9];
    let view = EmptyRanges::view(&bytes).without_trailing().unwrap();

    assert_eq!(view.length(), 0);
    assert!(view.first().is_empty());
    assert!(view.second().is_empty());
    assert_eq!(view.tail(), 9);
    assert_eq!(view.as_bytes(), &bytes);
}

#[test]
fn adjacent_mutable_ranges_respect_each_validated_boundary() {
    let mut bytes = [2, 2, b'a', b'b', b'c', b'd', 9];
    {
        let mut view = AdjacentMutableRangesViewMut::parse_exact_mut(&mut bytes).unwrap();
        view.first_mut().copy_from_slice(b"AB");
        view.second_mut().copy_from_slice(b"CD");
        assert_eq!(view.first(), b"AB");
        assert_eq!(view.second(), b"CD");
        assert_eq!(view.tail(), 9);
    }
    assert_eq!(bytes, [2, 2, b'A', b'B', b'C', b'D', 9]);

    let mut empty_first = [0, 2, b'x', b'y', 9];
    {
        let mut view = AdjacentMutableRangesViewMut::parse_exact_mut(&mut empty_first).unwrap();
        assert!(view.first_mut().is_empty());
        view.second_mut().copy_from_slice(b"XY");
        assert_eq!(view.tail(), 9);
    }
    assert_eq!(empty_first, [0, 2, b'X', b'Y', 9]);
}

#[test]
fn absolute_endpoints_frame_intermediate_ranges_and_preserve_later_entries() {
    let input = [3, b'a', b'b', 0xee, 9, 0xaa];
    let (view, suffix) = AbsoluteFramed::view(&input).with_remainder().unwrap();
    assert_eq!(view.as_bytes(), &input[..5]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.payload(), b"ab");
    assert_eq!(view.tail(), 9);

    let mut mutable = [3, b'a', b'b', 0xee, 9];
    {
        let mut view = AbsoluteFramedViewMut::parse_exact_mut(&mut mutable).unwrap();
        view.payload_mut().copy_from_slice(b"AB");
        assert_eq!(view.payload(), b"AB");
        assert_eq!(view.tail(), 9);
    }
    assert_eq!(mutable, [3, b'A', b'B', 0xee, 9]);

    assert!(matches!(
        AbsoluteFramed::view(&[0]).with_remainder(),
        Err(AbsoluteFramedError::RangeEndBeforeStart {
            position: 2,
            source_position: 1,
            end: 0,
            start: 1,
        })
    ));
}

#[test]
fn conversion_and_shortage_errors_precede_later_physical_entries() {
    let too_wide = [0xff; 16];
    assert!(matches!(
        WideLength::view(&too_wide).with_remainder(),
        Err(WideLengthError::InvalidRangeSource {
            position: 2,
            source_position: 1,
        })
    ));

    let short = [3, b'a'];
    assert!(matches!(
        ShortFramed::view(&short).with_remainder(),
        Err(ShortFramedError::InputTooShort {
            position: 2,
            expected: 3,
            available: 1,
        })
    ));

    assert!(matches!(
        ShortFramed::view(&[]).with_remainder(),
        Err(ShortFramedError::InputTooShort {
            position: 1,
            expected: 1,
            available: 0,
        })
    ));
}
