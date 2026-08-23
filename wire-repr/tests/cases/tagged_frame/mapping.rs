use super::*;

#[test]
fn borrowed_struct_forwarding_and_custom_errors_keep_generated_contracts_coherent() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x7f, 0xaa, 0xbb];
    let parent = BorrowedTableParent::view(&input)
        .table(&table)
        .without_trailing()
        .unwrap();
    assert!(parent.child().signal().is_halt());
    assert_eq!(parent.child().payload(), &[0xaa, 0xbb]);

    let plan = BorrowedTableParent {
        child: BorrowedTableChild {
            signal: TableSignal::Halt,
            payload: &[0xaa, 0xbb],
        },
    }
    .table(&table)
    .prepare()
    .unwrap();
    let mut output = [0_u8; 3];
    assert_eq!(plan.commit_into(&mut output).unwrap().0.as_bytes(), &input);

    assert!(matches!(
        CustomTableEnvelope::view(&[0x7f, 0xaa])
            .table(&table)
            .without_trailing(),
        Err(CustomTableEnvelopeError::Decode)
    ));
}

#[test]
fn borrowed_table_structs_support_borrowed_enum_bodies() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x41, 2, 0xaa, 0xbb];
    let view = BorrowedTableEnvelope::view(&input)
        .table(&table)
        .without_trailing()
        .unwrap();
    let body = view.operation().ping().unwrap();
    assert_eq!(body.payload(), &[0xaa, 0xbb]);

    let value = BorrowedTableEnvelope {
        operation: BorrowedTableOperation::Ping(BorrowedBody {
            length: 2,
            payload: &[0xaa, 0xbb],
        }),
    };
    let mut output = [0_u8; 4];
    let (written, suffix) = value.table(&table).build_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &input);
    assert!(suffix.is_empty());
}

#[test]
fn opcode_validated_cursor_retains_the_failing_item() {
    let opcodes = Opcodes {
        ping: 0x31,
        halt: 0x62,
        fail: false,
    };
    let input = [9, 0x62, 8, 0x62, 7, 9, 0x55, 8, 0x62, 7];
    let mut cursor = MappedPacket::cursor(&input).opcodes(&opcodes);
    assert!(cursor.next().unwrap().is_some());
    assert_eq!(cursor.remaining(), &input[5..]);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            MappedPacketValidationError::Decode(MappedPacketDecodeError::First(
                MappedOperationDecodeError::UnknownTag { tag: 0x55 }
            ))
        ))
    ));
    assert_eq!(cursor.remaining(), &input[5..]);
    let mut unchecked = cursor.unchecked();
    assert!(matches!(
        unchecked.next(),
        Err(wire_repr::ViewCursorError::Item(
            MappedPacketDecodeError::First(MappedOperationDecodeError::UnknownTag { tag: 0x55 })
        ))
    ));
    assert_eq!(unchecked.remaining(), &input[5..]);
}

#[test]
fn runtime_opcode_mapping_is_bidirectional_and_explicit() {
    let opcodes = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };

    let view = MappedOperation::view(&[0x41, 0x12, 0x34])
        .opcodes(&opcodes)
        .without_trailing()
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x41, 0x12, 0x34]);
    assert_eq!(view.ping().unwrap().value(), 0x1234);

    assert!(matches!(
        MappedOperation::view(&[0x55])
            .opcodes(&opcodes)
            .without_trailing(),
        Err(MappedOperationValidationError::Decode(
            MappedOperationDecodeError::UnknownTag { tag: 0x55 }
        ))
    ));

    let failing = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: true,
    };
    assert!(matches!(
        MappedOperation::view(&[0x41])
            .opcodes(&failing)
            .without_trailing(),
        Err(MappedOperationValidationError::Decode(
            MappedOperationDecodeError::OperationMapping(OpcodeMapError)
        ))
    ));

    let plan = MappedOperation::Halt.opcodes(&opcodes).prepare().unwrap();
    let mut output = [0xa5; 2];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0x7f]);
    assert_eq!(suffix, &mut [0xa5]);

    let mut short = [];
    assert!(
        MappedOperation::Halt
            .opcodes(&opcodes)
            .build_into(&mut short)
            .is_err()
    );
}

#[test]
fn table_named_structs_forward_explicitly_without_retaining_the_table() {
    let input = [0x41, 0x12, 0x34, 0x7f, 0xaa];
    let view = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        let (view, suffix) = TablePacket::view(&input)
            .table(&table)
            .with_remainder()
            .unwrap();
        assert_eq!(suffix, &[0xaa]);
        view
    };
    assert_eq!(view.first().ping().unwrap().value(), 0x1234);
    assert!(view.second().is_halt());

    let plan = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        TablePacket {
            first: TableOperation::Ping(Ping { value: 0x1234 }),
            second: TableOperation::Halt,
        }
        .table(&table)
        .prepare()
        .unwrap()
    };
    let mut output = [0_u8; 4];
    assert_eq!(
        plan.commit_into(&mut output).unwrap().0.as_bytes(),
        &[0x41, 0x12, 0x34, 0x7f]
    );

    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let cursor_input = [0x7f, 0x7f, 0x55, 0x7f];
    let mut cursor = TablePacket::cursor(&cursor_input).table(&table);
    assert!(cursor.next().unwrap().is_some());
    assert_eq!(cursor.remaining(), &cursor_input[2..]);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.remaining(), &cursor_input[2..]);
    let mut unchecked = cursor.unchecked();
    assert!(unchecked.next().is_err());
    assert_eq!(unchecked.remaining(), &cursor_input[2..]);
}
