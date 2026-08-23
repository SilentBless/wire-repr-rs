use bytes::{Bytes, BytesMut};

use super::probes::{
    decode_fixed, decode_flags, decode_operation, decode_packet, encode_packet, select_payload,
    semantic_surface,
};
use super::schema::OwnedPacket;

#[test]
fn owned_codegen_probes_cover_generated_read_and_write_shapes() {
    assert_eq!(
        decode_fixed(Bytes::from_static(&[7, 0x12, 0x34])),
        (7, 0x1234)
    );
    assert_eq!(decode_packet(Bytes::from_static(&[3, 8, 9, 10])), (8, 4));
    assert_eq!(decode_operation(Bytes::from_static(&[1, 11])), Some(11));
    assert_eq!(decode_operation(Bytes::from_static(&[2])), None);
    assert_eq!(decode_flags(Bytes::from_static(&[0, 5])), (true, 2));

    let payload = [8, 9, 10];
    assert_eq!(semantic_surface(&payload), (u8::MAX, true, true));
    let mut output = BytesMut::with_capacity(6);
    output.extend_from_slice(&[0xaa, 0xbb]);
    let pointer = output.as_ptr();
    let capacity = output.capacity();
    encode_packet(&payload, &mut output);
    assert_eq!(&output[..], &[0xaa, 0xbb, 3, 8, 9, 10]);
    assert_eq!(output.as_ptr(), pointer);
    assert_eq!(output.capacity(), capacity);

    let mut selected = [0; 3];
    select_payload(&payload, &mut selected);
    assert_eq!(selected, payload);
}

#[test]
fn owned_view_clone_shares_exact_backing_and_outlives_input_handle() {
    let input = Bytes::from_static(&[3, 8, 9, 10]);
    let view = OwnedPacket::view(input.clone()).without_trailing().unwrap();
    let cloned = view.clone();
    drop(input);
    drop(view);

    assert_eq!(cloned.as_bytes(), &[3, 8, 9, 10]);
    assert_eq!(cloned.payload(), &[8, 9, 10]);
}
