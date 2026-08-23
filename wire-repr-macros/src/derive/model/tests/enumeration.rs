use super::super::{EnumTag, UnknownPolicy, VariantSelector, WireType};
use super::parse;

#[test]
fn accepts_valid_enum_source_model() {
    let WireType::Enum(model) =
        parse("#[wire(tag = BeU16, unknown = reject)] enum Op { Ping = 1, Data(Body) = 2 }")
            .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        model.variants[0].selector,
        VariantSelector::Integer(1)
    ));
    assert!(matches!(model.tag, EnumTag::Integer(_)));
    assert!(matches!(model.unknown, UnknownPolicy::Reject));
    let Err(error) = parse("#[wire(tag = U8)] enum Op { Ping = 1 }") else {
        panic!("missing unknown policy must be rejected");
    };
    assert!(error.to_string().contains("explicit unknown policy"));
    assert!(parse("#[wire(tag = U8, unknown = preserve)] enum Op { Ping = 1 }").is_err());
    assert!(parse("#[wire(tag = U8, unknown = preserve)] #[repr(u8)] enum OpenOp { Ping = 1, #[wire(unknown)] Other(u8) }").is_ok());
}

#[test]
fn accepts_fixed_byte_enum_selectors_and_unknown_capture() {
    let WireType::Enum(model) = parse(
            "#[wire(tag = [u8; 2], unknown = preserve)] enum Op { #[wire(tag = b\"OK\")] Ok, #[wire(tag = b\"NO\")] No(Body), #[wire(unknown)] Other([u8; 2]) }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(matches!(model.tag, EnumTag::Bytes { width: 2 }));
    assert!(matches!(model.unknown, UnknownPolicy::Preserve));
    assert!(
        matches!(model.variants[0].selector, VariantSelector::Bytes(ref value) if value == b"OK")
    );
    assert!(
        matches!(model.variants[1].selector, VariantSelector::Bytes(ref value) if value == b"NO")
    );
    assert!(matches!(
        model.variants[2].selector,
        VariantSelector::Unknown
    ));
}

#[test]
fn rejects_invalid_fixed_byte_enum_selectors() {
    for source in [
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"A\")] A }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A, #[wire(tag = b\"OK\")] B }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { A = 1 }",
        "#[wire(tag = [u8; 2])] enum Op { #[wire(tag = b\"OK\")] A }",
        "#[wire(tag = [u8; 2], unknown = preserve)] enum Op { #[wire(tag = b\"OK\")] A }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A, #[wire(unknown)] Other([u8; 2]) }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { A }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] A([u8; 2]), #[wire(unknown)] B([u8; 2]) }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] Other }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(unknown)] Other([u8; 1]) }",
        "#[wire(tag = [u8; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\", unknown)] Other([u8; 2]) }",
        "#[wire(tag = [u8; 0], unknown = reject)] enum Op { #[wire(tag = b\"\")] Empty }",
        "#[wire(tag = [u16; 2], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A }",
        "#[wire(tag = [u8; WIDTH], unknown = reject)] enum Op { #[wire(tag = b\"OK\")] A }",
        "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(tag = b\"A\")] A = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(unknown)] Other([u8; 1]) }",
        "#[wire(tag = [u8; 2], table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ok)] Ok }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn accepts_named_operation_inputs_and_rejects_mixed_dispatch() {
    let WireType::Enum(model) = parse(
            "#[wire(tag = U8, opcodes = Opcodes, opcodes_error = OpcodeMapError, unknown = reject)] enum MappedOperation { #[wire(opcodes = Opcode::Ping)] Ping(Ping), #[wire(opcodes = Opcode::Halt)] Halt }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(
        model
            .operation_input
            .as_ref()
            .is_some_and(|input| input.name == "opcodes")
    );

    let WireType::Enum(model) = parse(
            "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping(Body), #[wire(table = Opcode::Halt)] Halt }",
        )
        .unwrap()
        else {
            panic!()
        };
    let input = model.operation_input.as_ref().expect("operation input");
    assert!(input.name == "table");
    assert!(input.error.is_some());
    assert!(
        model
            .variants
            .iter()
            .all(|variant| variant.operation_selector.is_some())
    );

    let WireType::Struct(model) = parse(
            "#[wire(table = OpcodeTable)] struct Packet { lead: u8, #[wire(table)] first: Op, #[wire(table)] second: Op }",
        )
        .unwrap()
        else {
            panic!()
        };
    assert!(model.operation_input.is_some());
    assert!(model.fields[1].operation_input.is_some());
    assert!(model.fields[2].operation_input.is_some());

    for source in [
        "#[wire(tag = U8, table = OpcodeTable, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
        "#[wire(tag = U8, table_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
        "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { Ping = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping = 1 }",
        "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { Ping }",
        "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] A, #[wire(table = Opcode::Ping)] B }",
        "struct Packet { #[wire(table)] operation: Op }",
        "#[wire(table = OpcodeTable)] struct Packet { #[wire(table)] value: u8 }",
        "#[wire(table = OpcodeTable, table_error = OpcodeError)] struct Packet { #[wire(table)] operation: Op }",
        "#[wire(table = OpcodeTable)] struct Packet { #[wire(other)] operation: Op }",
        "#[wire(tag = U8, table = OpcodeTable, other_error = OpcodeError, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
        "#[wire(tag = U8, table = OpcodeTable, table_error = OpcodeError, other = OtherTable, unknown = reject)] enum Op { #[wire(table = Opcode::Ping)] Ping }",
        "#[wire(tag = U8, view = OpcodeTable, unknown = reject)] enum Op { #[wire(view = Opcode::Ping)] Ping }",
        "#[wire(tag = U8, cursor = OpcodeTable, unknown = reject)] enum Op { #[wire(cursor = Opcode::Ping)] Ping }",
        "#[wire(builder = OpcodeTable)] struct Packet { #[wire(builder)] operation: Op }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn rejects_invalid_enum_source_models() {
    for source in [
        "enum Op { Ping = 1 }",
        "#[wire(tag = I8, unknown = reject)] enum Op { Ping = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { Ping }",
        "#[wire(tag = U8, unknown = reject)] enum Op { Ping = 1 + 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { A = 1, B = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { A = 256 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { A(u8, u8) = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { A { value: u8 } = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { #[wire(tag = 1, unknown = reject)] A = 1 }",
        "#[wire(tag = U8, unknown = reject)] enum Op { A(#[wire(be)] Body) = 1 }",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}
