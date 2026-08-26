#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
#[wire(as = u16, be)]
struct Foo {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    kind: u8,
    #[wire(bits = 8..=11)]
    code: u8,
}

#[derive(WireView, WireBuilder)]
struct Inline {
    #[wire(le)]
    raw: u16,
    #[wire(bits_of = raw, bit = 0)]
    enabled: bool,
    #[wire(bits_of = raw, bits = 1..=3)]
    kind: u8,
    tail: u8,
}

#[derive(WireBuilder)]
struct Outer<T> {
    value: T,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(as = u128, le)]
struct FullWidth {
    #[wire(bits = 0..=127)]
    value: u128,
}

#[derive(WireView, WireBuilder)]
struct HighBit {
    #[wire(le)]
    raw: u128,
    #[wire(bits_of = raw, bit = 127)]
    high: bool,
}

#[test]
fn nominal_bitfield_views_decode_declared_ranges_and_keep_exact_bits() -> TestResult {
    let view = Foo::view([0b1010_1010, 0b0101_1011])?;
    assert!(view.enabled());
    assert_eq!(view.kind(), 5);
    assert_eq!(view.code(), 10);
    assert_eq!(view.as_bytes(), &[0b1010_1010, 0b0101_1011]);
    Ok(())
}

#[test]
fn fresh_bitfield_builders_zero_unassigned_bits() -> TestResult {
    let mut output = [0xff; 2];
    Foo::builder(&mut output[..])
        .enabled(true)?
        .kind(5)?
        .code(10)?
        .finish()?;
    assert_eq!(output, [0b0000_1010, 0b0000_1011]);
    Ok(())
}

#[test]
fn bitfield_setter_rejects_values_outside_the_declared_width() {
    let mut output = [0u8; 2];
    let error = match Foo::builder(&mut output[..]).kind(8) {
        Ok(_) => panic!("out-of-range bitfield value unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(error, FooBuildError::OutOfRange { field: "kind" }));
}

#[test]
fn inline_projections_share_one_earlier_physical_scalar() -> TestResult {
    let view = Inline::view([0b1010_1011, 0b0101_0101, 9])?;
    assert!(view.enabled());
    assert_eq!(view.kind(), 5);
    assert_eq!(view.raw(), 0x55ab);
    assert_eq!(view.tail(), 9);

    let mut output = [0xff; 3];
    Inline::builder(&mut output[..])
        .enabled(true)?
        .kind(5)?
        .tail(9)?
        .finish()?;
    assert_eq!(output, [0b0000_1011, 0, 9]);
    Ok(())
}

#[test]
fn nominal_and_inline_bitfields_compose_as_fixed_children() -> TestResult {
    let mut nominal = [0u8; 3];
    Outer::<Foo>::builder(&mut nominal[..])
        .value(|foo| foo.enabled(true).kind(5).code(10))?
        .tail(9)?
        .finish()?;
    assert_eq!(nominal, [0x0a, 0x0b, 9]);

    let mut inline = [0u8; 4];
    Outer::<Inline>::builder(&mut inline[..])
        .value(|inline| inline.enabled(true).kind(5).tail(9))?
        .tail(7)?
        .finish()?;
    assert_eq!(inline, [0x0b, 0, 9, 7]);
    Ok(())
}

#[test]
fn inline_projection_rejects_a_value_wider_than_its_range() {
    let mut output = [0u8; 3];
    let error = match Inline::builder(&mut output[..]).kind(8) {
        Ok(_) => panic!("out-of-range projection unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(InlineWriteError::Layout(wire_repr::LayoutError {
            field: "kind"
        }))
    ));
}

#[test]
fn u128_full_width_and_high_bit_paths_do_not_shift_by_128() -> TestResult {
    let mut full = [0u8; 16];
    FullWidth::builder(&mut full[..])
        .value(u128::MAX)?
        .finish()?;
    assert_eq!(full, [0xff; 16]);
    assert_eq!(FullWidth::view(full)?.value(), u128::MAX);

    let mut high = [0u8; 16];
    HighBit::builder(&mut high[..]).high(true)?.finish()?;
    assert_eq!(high[15], 0x80);
    assert!(HighBit::view(high)?.high());
    Ok(())
}
