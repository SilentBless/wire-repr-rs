use super::super::WireType;
use super::parse;

#[test]
fn accepts_nominal_bitfields_and_rejects_invalid_projections() {
    assert!(matches!(
            parse("#[wire(bitfield = u16, be, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: bool, #[wire(bits = 1..=3)] mode: u8 }").unwrap(),
            WireType::Bitfield(_)
        ));
    for source in [
        "#[wire(bitfield = u16, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: bool }",
        "#[wire(bitfield = u8)] struct Flags { #[wire(bit = 0)] enabled: bool }",
        "#[wire(bitfield = u8, reserved = preserve)] struct Flags { #[wire(bit = 0)] enabled: bool }",
        "#[wire(bitfield = u8, reserved = zero)] struct Flags { enabled: bool }",
        "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bit = 8)] enabled: bool }",
        "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bit = 0)] enabled: u8 }",
        "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bits = 0..=3)] mode: bool }",
        "#[wire(bitfield = u8, reserved = zero)] struct Flags { #[wire(bits = 0..=4)] mode: u8, #[wire(bit = 4)] other: bool }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}
