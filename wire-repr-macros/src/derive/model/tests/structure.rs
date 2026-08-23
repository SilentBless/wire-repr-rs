use super::super::{Codec, FieldKind, FieldPosition, WireType};
use super::parse;

#[test]
fn classifies_fixed_and_nested_fields() {
    let WireType::Struct(model) = parse("struct Packet { a: u8, b: i8, #[wire(be)] c: u16, #[wire(le)] d: i128, #[wire(codec = Custom)] e: Value, #[wire(prefix = Varint)] varint: u32, bytes: [u8; 32], child: Child }").unwrap() else { panic!() };
    assert!(matches!(
        model.fields[0].kind,
        FieldKind::Fixed(Codec::Builtin("U8"))
    ));
    assert!(matches!(
        model.fields[1].kind,
        FieldKind::Fixed(Codec::Builtin("I8"))
    ));
    assert!(matches!(
        model.fields[2].kind,
        FieldKind::Fixed(Codec::Builtin("BeU16"))
    ));
    assert!(matches!(
        model.fields[3].kind,
        FieldKind::Fixed(Codec::Builtin("LeI128"))
    ));
    assert!(matches!(
        model.fields[4].kind,
        FieldKind::Fixed(Codec::Custom(_))
    ));
    assert!(matches!(model.fields[5].kind, FieldKind::Prefix(_)));
    assert!(matches!(
        model.fields[6].kind,
        FieldKind::Fixed(Codec::OwnedBytes(_))
    ));
    assert!(matches!(model.fields[7].kind, FieldKind::Nested));
}

#[test]
fn rejects_unannotated_multibyte_integers() {
    for ty in ["u16", "i16", "u32", "i32", "u64", "i64", "u128", "i128"] {
        assert!(parse(&format!("struct Packet {{ value: {ty} }}")).is_err());
    }
}

#[test]
fn accepts_bounded_byte_fields_with_earlier_unsigned_sources() {
    let WireType::Struct(model) = parse(
            "struct Packet<'wire> { length: u8, #[wire(bytes = length)] payload: &'wire [u8], tail: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(model.fields[1].kind, FieldKind::Bytes { .. }));
    assert_eq!(model.preparation.controlled_by, vec![Some(1), None, None]);
    assert_eq!(
        model.preparation.position_sources,
        vec![false, false, false]
    );
}

#[test]
fn accepts_forward_positions_and_rejects_ambiguous_sources() {
    let WireType::Struct(model) = parse(
            "struct Packet<'wire> { offset: u8, length: u8, #[wire(at = offset, bytes = length)] payload: &'wire [u8] }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(
        model.fields[2].position.as_ref().expect("position"),
        FieldPosition::Source(source) if source == "offset"
    ));
    assert_eq!(model.preparation.controlled_by, vec![None, Some(2), None]);
    assert_eq!(model.preparation.position_sources, vec![true, false, false]);

    let WireType::Struct(model) =
        parse("struct Header { kind: u8, #[wire(at = 8)] value: u8 }").unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        model.fields[1].position,
        Some(FieldPosition::Static(8))
    ));

    for source in [
        "struct Packet { #[wire(at = missing)] value: u8 }",
        "struct Packet { #[wire(at = offset)] value: u8, offset: u8 }",
        "struct Packet { offset: i8, #[wire(at = offset)] value: u8 }",
        "struct Packet { offset: u8, #[wire(at = offset, pad_before = 1)] value: u8 }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn accepts_unsigned_prefix_byte_length_sources() {
    let WireType::Struct(model) = parse(
            "struct Packet<'wire> { #[wire(prefix = Varint)] length: u32, #[wire(bytes = length)] payload: &'wire [u8] }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(model.fields[0].kind, FieldKind::Prefix(_)));
    assert!(matches!(model.fields[1].kind, FieldKind::Bytes { .. }));
}

#[test]
fn rejects_ambiguous_or_noncanonical_byte_length_sources() {
    for source in [
        "struct Packet<'wire> { #[wire(bytes = missing)] payload: &'wire [u8] }",
        "struct Packet<'wire> { #[wire(bytes = length)] payload: &'wire [u8], length: u8 }",
        "struct Packet<'wire> { length: i8, #[wire(bytes = length)] payload: &'wire [u8] }",
        "struct Packet<'wire> { #[wire(codec = Custom)] length: u8, #[wire(bytes = length)] payload: &'wire [u8] }",
        "struct Packet<'wire> { length: u8, #[wire(bytes = length)] payload: &'wire [u16] }",
        "struct Packet<'wire> { length: u8, #[wire(bytes = length)] first: &'wire [u8], #[wire(bytes = length)] second: &'wire [u8] }",
        "struct Packet { #[wire] value: u8 }",
        "struct Packet { #[wire()] value: u8 }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn accepts_ordered_struct_validators_with_one_human_error_type() {
    let WireType::Struct(model) = parse(
            "#[wire(error = PacketError, validate = validate_model_first, validate = validate_model_second)] struct Packet { #[wire(validate = validate_field_first, validate = validate_field_second)] value: u8 }",
        )
        .unwrap()
        else {
            panic!()
        };

    assert_eq!(model.validators.len(), 2);
    assert!(model.validators[0].is_ident("validate_model_first"));
    assert!(model.validators[1].is_ident("validate_model_second"));
    assert!(model.validation_error.is_some());
    assert_eq!(model.fields[0].validators.len(), 2);
    assert!(model.fields[0].validators[0].is_ident("validate_field_first"));
    assert!(model.fields[0].validators[1].is_ident("validate_field_second"));

    let WireType::Struct(model) =
        parse("#[wire(error = ParentError)] struct Parent { child: Child }").unwrap()
    else {
        panic!()
    };
    assert!(model.validation_error.is_some());
}

#[test]
fn rejects_incomplete_or_unsupported_struct_validator_contracts() {
    for source in [
        "#[wire(validate = validate_model)] struct Packet { value: u8 }",
        "struct Packet { #[wire(validate = validate_field)] value: u8 }",
        "#[wire(error = PacketError)] struct Packet { value: u8 }",
        "#[wire(error = FirstError, error = SecondError, validate = validate_model)] struct Packet { value: u8 }",
        "#[wire(bitfield = u8, reserved = zero, error = FlagsError, validate = validate_flags)] struct Flags { #[wire(bit = 0)] enabled: bool }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }

    for source in [
        "#[wire(table = OpcodeTable, error = PacketError, validate = validate_model)] struct Packet { #[wire(table)] operation: Op }",
        "#[wire(table = OpcodeTable, error = PacketError)] struct Packet { #[wire(table, validate = validate_field)] operation: Op }",
    ] {
        if let Err(error) = parse(source) {
            panic!("{source}: {error}");
        }
    }
}

#[test]
fn rejects_container_attributes_on_structs() {
    assert!(parse("#[wire(tag = U8, unknown = reject)] struct Packet { value: u8 }").is_err());
}
