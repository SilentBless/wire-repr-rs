#![allow(dead_code)]

use wire_repr::WireView;

#[derive(WireView)]
pub struct Frame {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

#[derive(WireView)]
pub struct Header {
    kind: u8,
}

pub fn inspect_sequences(input: &[u8]) {
    let mut views = Frame::views(input).expect("Frame has a leading extent");
    for _ in 0..=input.len() {
        let before = views.position();
        let remaining = views.remaining().len();
        match views.next() {
            Ok(Some(view)) => {
                assert_eq!(view.body().len(), usize::from(view.length()));
                assert_eq!(views.position(), before + view.as_bytes().len());
                assert!(views.remaining().len() < remaining);
            }
            Ok(None) => {
                assert_eq!(views.position(), input.len());
                break;
            }
            Err(_) => {
                assert_eq!(views.position(), before);
                assert_eq!(views.remaining().len(), remaining);
                break;
            }
        }
    }

    if input.is_empty() {
        return;
    }
    let Ok((header, mut cursor)) = Header::cursor(input) else {
        return;
    };
    assert_eq!(header.kind(), input[0]);
    let before = cursor.position();
    let remaining = cursor.remaining().len();
    match Frame::next(&mut cursor) {
        Ok(view) => {
            assert_eq!(view.body().len(), usize::from(view.length()));
            assert_eq!(cursor.position(), before + view.as_bytes().len());
        }
        Err(_) => {
            assert_eq!(cursor.position(), before);
            assert_eq!(cursor.remaining().len(), remaining);
        }
    }
}
