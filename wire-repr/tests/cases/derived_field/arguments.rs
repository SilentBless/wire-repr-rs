use super::*;

#[test]
fn callback_arguments_preserve_mixed_semantic_and_physical_order() {
    let mut output = [0; 5];
    let (written, suffix) = OrderedCallback::builder()
        .kind(2)
        .first(7)
        .second(8)
        .tail(9)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[213, 2, 7, 8, 9]);
    assert!(suffix.is_empty());
}

#[test]
fn callback_can_pull_a_nested_prepared_field_selection() {
    let data = [8, 9];
    let mut output = [0; 4];
    let (written, _) = NestedSelection::builder()
        .tail(NestedTail {
            kind: 7,
            data: &data,
        })
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[7, 7, 8, 9]);
}

#[test]
fn callback_reads_selected_prepared_bytes_in_dependency_order() {
    let payload = [1, 2];
    let mut output = [0; 4];
    let (written, _) = Checksummed::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[5, 2, 1, 2]);
    let view = Checksummed::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert_eq!(view.checksum(), 5);
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), payload);
}

#[test]
fn callbacks_accept_zero_arguments_and_empty_include_arguments() {
    let mut constant = [0; 1];
    let (written, suffix) = NoArgumentCallback::builder()
        .build_into(&mut constant)
        .unwrap();
    assert_eq!(written.as_bytes(), &[7]);
    assert!(suffix.is_empty());

    let mut empty = [0; 2];
    let (written, suffix) = EmptyIncludeCallback::builder()
        .value(9)
        .build_into(&mut empty)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0, 9]);
    assert!(suffix.is_empty());

    let mut all = [0; 2];
    let (written, suffix) = EmptyExcludeCallback::builder()
        .value(9)
        .build_into(&mut all)
        .unwrap();
    assert_eq!(written.as_bytes(), &[1, 9]);
    assert!(suffix.is_empty());
}

#[test]
fn exclude_self_source_contains_generated_physical_gaps() {
    let mut output = [0xff; 6];
    let (written, _) = PositionedChecksum::builder()
        .marker(7)
        .tail(8)
        .payload(&[])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[5, 0, 0, 0, 7, 8]);
}

#[test]
fn include_read_sets_order_computations_without_using_declaration_order() {
    let payload = [1, 2];
    let mut output = [0; 4];
    let (written, _) = IncludedDependency::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[6, 3, 1, 2]);
}

#[test]
fn semantic_callback_receives_the_ordinary_field_by_reference() {
    let mut output = [0; 2];
    let (written, suffix) = SemanticCallback::builder()
        .value(7)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[8, 7]);
    assert!(suffix.is_empty());
}
