use bytes::{Bytes, BytesMut};
use wire_repr::PreparedLayout;

use super::schema::{OwnedFixed, OwnedFlags, OwnedOperation, OwnedPacket};

pub(super) fn semantic_surface(payload: &[u8]) -> (u8, bool, bool) {
    let packet = OwnedPacket {
        length: u8::MAX,
        payload,
    };
    let data = OwnedOperation::Data(super::schema::OwnedBody { value: 1 });
    let halt = OwnedOperation::Halt;
    (
        packet.length,
        matches!(data, OwnedOperation::Data(_)),
        matches!(halt, OwnedOperation::Halt),
    )
}

pub(super) fn decode_fixed(input: Bytes) -> (u8, u16) {
    let view = OwnedFixed::view(input).without_trailing().unwrap();
    (view.lead(), view.word())
}

pub(super) fn decode_packet(input: Bytes) -> (u8, usize) {
    let view = OwnedPacket::view(input).without_trailing().unwrap();
    (view.payload()[0], view.as_bytes().len())
}

pub(super) fn decode_operation(input: Bytes) -> Option<u8> {
    let view = OwnedOperation::view(input).without_trailing().unwrap();
    view.data().map(|body| body.value())
}

pub(super) fn decode_flags(input: Bytes) -> (bool, u8) {
    let view = OwnedFlags::view(input).without_trailing().unwrap();
    (view.enabled(), view.mode())
}

pub(super) fn encode_packet(payload: &[u8], output: &mut BytesMut) {
    OwnedPacket::builder()
        .payload(payload)
        .prepare()
        .unwrap()
        .commit_into(output)
        .unwrap();
}

pub(super) fn select_payload(payload: &[u8], output: &mut [u8]) {
    let plan = OwnedPacket::builder().payload(payload).prepare().unwrap();
    plan.bytes()
        .include(|fields| fields.payload)
        .write_into(output);
}
