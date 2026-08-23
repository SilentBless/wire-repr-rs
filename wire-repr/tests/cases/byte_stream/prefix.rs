use super::*;

#[test]
fn borrowed_prefix_values_plans_and_exact_spans_are_preserved() {
    let input = [b'a', b'b', 0, 0x99];
    let extent = Terminated::validate_prefix(&input).unwrap();
    let (encoded, suffix) = extent.split_input(&input).unwrap();
    let value = Terminated::decode(encoded);
    assert_eq!(encoded, &[b'a', b'b', 0]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(value, b"ab");
    assert_eq!(value.as_ptr(), input.as_ptr());
    assert_eq!(
        Terminated::plan(value).map(render_plan::<3>),
        Ok([b'a', b'b', 0])
    );
    assert_eq!(
        Terminated::plan(&[b'a', 0]).map(|_| ()),
        Err(TerminatedEncodeError::EmbeddedTerminator)
    );
    assert_eq!(
        Terminated::validate_prefix(b"ab"),
        Err(TerminatedDecodeError::Incomplete)
    );
}

#[test]
fn prefix_extent_preserves_exact_spans_and_decode_follows_validation() {
    let canonical_input = [42, 0x99];
    let canonical_extent = TinyPrefix::validate_prefix(&canonical_input).unwrap();
    let (canonical_encoded, canonical_suffix) =
        canonical_extent.split_input(&canonical_input).unwrap();
    assert_eq!(canonical_encoded, &[42]);
    assert_eq!(canonical_suffix, &[0x99]);
    assert_eq!(TinyPrefix::decode(canonical_encoded), 41);

    let noncanonical_input = [0, 41, 0x99];
    let noncanonical_extent = TinyPrefix::validate_prefix(&noncanonical_input).unwrap();
    let (noncanonical_encoded, noncanonical_suffix) = noncanonical_extent
        .split_input(&noncanonical_input)
        .unwrap();
    assert_eq!(noncanonical_encoded, &[0, 41]);
    assert_eq!(noncanonical_suffix, &[0x99]);
    assert_eq!(TinyPrefix::decode(noncanonical_encoded), 41);
    assert_eq!(
        noncanonical_extent.encoded_len(),
        NonZeroUsize::new(2).unwrap()
    );

    let short_input = [0xa5, 0x5a];
    let overclaimed = PrefixExtent::new(NonZeroUsize::new(3).unwrap());
    assert_eq!(overclaimed.split_input(&short_input), None);

    assert_eq!(TinyPrefix::plan(41), Ok([42]));
    assert_eq!(TinyPrefix::plan(255), Err(TinyEncodeError::ReservedMarker));
}

#[test]
fn prefix_validation_distinguishes_empty_and_incomplete_input() {
    assert_eq!(
        TinyPrefix::validate_prefix(&[]),
        Err(TinyDecodeError::Empty)
    );
    assert_eq!(
        TinyPrefix::validate_prefix(&[0]),
        Err(TinyDecodeError::Incomplete)
    );
}
