use super::*;

#[test]
fn enum_body_validation_is_fail_closed_and_composes_into_parents() {
    let invalid = [1, 0, 0];
    assert!(matches!(
        Operation::view(&invalid).without_trailing(),
        Err(OperationValidationError::Ping(PingError::Zero))
    ));
    assert!(matches!(
        Operation::view(&[1, 0, 0, 0xaa]).without_trailing(),
        Err(OperationValidationError::Ping(PingError::Zero))
    ));
    assert_eq!(
        Operation::view(&invalid)
            .unchecked()
            .without_trailing()
            .unwrap()
            .ping()
            .unwrap()
            .value(),
        0
    );

    let parent = [9, 1, 0, 0, 8, 2, 7];
    assert!(matches!(
        Packet::view(&parent).without_trailing(),
        Err(PacketValidationError::NestedFirst(
            OperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(
        Packet::view(&parent)
            .unchecked()
            .without_trailing()
            .unwrap()
            .first()
            .ping()
            .unwrap()
            .value(),
        0
    );

    let cursor_input = [1, 0, 0, 2];
    let mut cursor = Operation::cursor(&cursor_input);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            OperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(cursor.remaining(), &cursor_input);
    let mut unchecked = cursor.unchecked();
    assert_eq!(
        unchecked.next().unwrap().unwrap().ping().unwrap().value(),
        0
    );
    assert_eq!(unchecked.remaining(), &[2]);
}

#[test]
fn table_operation_body_validation_is_fail_closed() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x41, 0, 0, 0xaa];
    assert!(matches!(
        TableOperation::view(&input)
            .table(&table)
            .without_trailing(),
        Err(TableOperationValidationError::Ping(PingError::Zero))
    ));
    assert_eq!(
        TableOperation::view(&input)
            .table(&table)
            .unchecked()
            .with_remainder()
            .unwrap()
            .0
            .ping()
            .unwrap()
            .value(),
        0
    );
    let mut cursor = TableOperation::cursor(&input).table(&table);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            TableOperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(cursor.remaining(), &input);
}
