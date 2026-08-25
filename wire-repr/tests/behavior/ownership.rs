#![allow(dead_code)]

use bytes::Bytes;
use std::sync::mpsc;
use std::thread;
use wire_repr::WireView;

#[derive(WireView)]
struct Bar {
    #[wire(le)]
    foo: u32,
}

#[derive(WireView)]
struct Foo<T> {
    #[wire(le)]
    foo: u32,
    bar: T,
}

type FooBar = Foo<Bar>;

const BYTES: [u8; 8] = [1, 0, 0, 0, 2, 0, 0, 0];

fn borrowed_foo(input: &[u8]) -> impl FooView<Bar> + '_ {
    FooBar::view(input).unwrap()
}

#[test]
fn borrowed_view_retains_the_callers_slice_lifetime_and_identity() {
    let foo = borrowed_foo(&BYTES);
    assert_eq!(foo.as_bytes().as_ptr(), BYTES.as_ptr());
    assert_eq!(foo.foo(), 1);
    assert_eq!(foo.bar().foo(), 2);
}

#[test]
fn vec_backed_view_moves_through_a_thread_channel_with_its_allocation() {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let input = BYTES.to_vec();
        let pointer = input.as_ptr() as usize;
        let foo = FooBar::view(input).unwrap();
        sender.send((foo, pointer)).unwrap();
    })
    .join()
    .unwrap();

    let (foo, pointer) = receiver.recv().unwrap();
    assert_eq!(foo.as_bytes().as_ptr() as usize, pointer);
    assert_eq!(foo.foo(), 1);
    assert_eq!(foo.bar().foo(), 2);
}

#[test]
fn bytes_backed_view_moves_through_a_thread_channel_with_its_allocation() {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let input = Bytes::from(BYTES.to_vec());
        let pointer = input.as_ptr() as usize;
        let foo = FooBar::view(input).unwrap();
        sender.send((foo, pointer)).unwrap();
    })
    .join()
    .unwrap();

    let (foo, pointer) = receiver.recv().unwrap();
    assert_eq!(foo.as_bytes().as_ptr() as usize, pointer);
    assert_eq!(foo.foo(), 1);

    assert_eq!(foo.bar().foo(), 2);
}
