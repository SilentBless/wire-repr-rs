#![allow(dead_code)]

use wire_repr::{ByteSelection, WireBuilder, WireView, computed};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn checksum(selection: impl ByteSelection) -> u32 {
    selection.bytes().map(u32::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Foo {
    first: u8,
    #[wire(le, computed = checksum(exclude(self)))]
    checksum: u32,
    tail: u8,
}

#[derive(WireBuilder)]
struct Outer<T> {
    value: T,
    tail: u8,
}

fn ordered(first: u8, selection: impl ByteSelection) -> u16 {
    u16::from(first) + selection.bytes().map(u16::from).sum::<u16>()
}

#[derive(WireView, WireBuilder)]
struct Ordered {
    first: u8,
    #[wire(le, computed = ordered(first, include(first, tail)))]
    checksum: u16,
    tail: u8,
}

fn checksum16(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Dynamic {
    #[wire(le, computed = checksum16(exclude(self)))]
    checksum: u16,
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

fn sum16(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

fn doubled(value: u16) -> u16 {
    value * 2
}

#[derive(WireView, WireBuilder)]
struct DependencyOrder {
    first: u8,
    #[wire(le, computed = doubled(second))]
    third: u16,
    #[wire(le, computed = sum16(include(first, tail)))]
    second: u16,
    tail: u8,
}

#[derive(Debug, thiserror::Error)]
#[error("computed callback rejected input")]
struct ComputeError;

#[computed]
fn checked_checksum(selection: impl ByteSelection) -> Result<u16, ComputeError> {
    let value = selection.bytes().map(u16::from).sum::<u16>();
    (value <= 10).then_some(value).ok_or(ComputeError)
}

#[derive(WireView, WireBuilder)]
struct Fallible {
    first: u8,
    #[wire(le, try_computed = checked_checksum(exclude(self)))]
    checksum: u16,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct ComputedChild {
    first: u8,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct NestedComputed {
    #[wire(le, computed = checksum16(include(child.first, suffix)))]
    checksum: u16,
    child: ComputedChild,
    suffix: u8,
}

#[test]
fn computed_field_patches_its_stored_value_from_physical_selection() -> TestResult {
    let mut output = [0u8; 6];
    Foo::builder(&mut output[..]).first(1)?.tail(9)?.finish()?;
    assert_eq!(output, [1, 10, 0, 0, 0, 9]);

    let view = Foo::view(output)?;
    assert_eq!(view.checksum(), 10);
    Ok(())
}

#[test]
fn computed_fields_compose_through_detached_child_writers() -> TestResult {
    let mut output = [0u8; 7];
    Outer::<Foo>::builder(&mut output[..])
        .value(|foo| foo.first(1).tail(9))?
        .tail(7)?
        .finish()?;
    assert_eq!(output, [1, 10, 0, 0, 0, 9, 7]);
    Ok(())
}

#[test]
fn computed_callbacks_mix_logical_values_and_physical_selections() -> TestResult {
    let mut output = [0u8; 4];
    Ordered::builder(&mut output[..])
        .first(2)?
        .tail(3)?
        .finish()?;
    assert_eq!(output, [2, 7, 0, 3]);
    Ok(())
}

#[test]
fn fixed_offset_computed_destination_can_select_demand_geometry() -> TestResult {
    let mut output = [0u8; 6];
    Dynamic::builder(&mut output[..])
        .body(&[1, 2, 3][..])?
        .finish()?;
    assert_eq!(output, [9, 0, 3, 1, 2, 3]);
    Ok(())
}
#[test]
fn computed_dependency_dag_orders_callbacks_independently_of_field_order() -> TestResult {
    let mut output = [0u8; 6];
    DependencyOrder::builder(&mut output[..])
        .first(1)?
        .tail(2)?
        .finish()?;
    assert_eq!(output, [1, 6, 0, 3, 0, 2]);
    Ok(())
}

#[test]
fn fallible_computed_error_keeps_the_destination_field_site() {
    let mut output = [0u8; 4];
    let error = match Fallible::builder(&mut output[..])
        .first(9)
        .and_then(|writer| writer.tail(9))
        .and_then(|writer| writer.finish())
    {
        Ok(_) => panic!("fallible callback unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FallibleWriteError::ChecksumComputed(_))
    ));
}

#[test]
fn computed_callbacks_accept_nested_physical_paths() -> TestResult {
    let mut output = [0u8; 5];
    NestedComputed::builder(&mut output[..])
        .child(|child| child.first(3).tail(9))?
        .suffix(4)?
        .finish()?;
    assert_eq!(output, [7, 0, 3, 9, 4]);
    assert_eq!(NestedComputed::view(output)?.checksum(), 7);
    Ok(())
}
