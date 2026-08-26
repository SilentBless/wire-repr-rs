#![allow(dead_code)]

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(le)]
    value: u16,
}

#[derive(WireView)]
struct Bar {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct Header {
    kind: u8,
}

pub fn fixed(seed: u64) -> u64 {
    let input = fixed_input(seed);
    let Ok(views) = Foo::views(&input) else {
        return u64::MAX;
    };
    views.fold(0u64, |sum, view| sum.wrapping_add(u64::from(view.value())))
}

pub fn variable(seed: u64) -> u64 {
    let input = variable_input(seed);
    let Ok(mut views) = Bar::views(&input) else {
        return u64::MAX;
    };
    let mut sum = 0u64;
    loop {
        match views.next() {
            Ok(Some(view)) => {
                sum = view
                    .body()
                    .iter()
                    .fold(sum, |sum, byte| sum.rotate_left(5) ^ u64::from(*byte));
            }
            Ok(None) => return sum,
            Err(_) => return u64::MAX,
        }
    }
}

pub fn cursor(seed: u64) -> u64 {
    let input = cursor_input(seed);
    let Ok((header, mut cursor)) = Header::cursor(&input) else {
        return u64::MAX;
    };
    let Ok(body) = Bar::next(&mut cursor) else {
        return u64::MAX;
    };
    u64::from(header.kind())
        ^ body
            .body()
            .iter()
            .fold(0u64, |value, byte| value.rotate_left(5) ^ u64::from(*byte))
        ^ cursor.remaining().len() as u64
}

#[inline(never)]
fn fixed_input(seed: u64) -> [u8; 128] {
    let mut output = [0u8; 128];
    let mut index = 0usize;
    while index < 64 {
        let value = (seed as u16).wrapping_add(index as u16).to_le_bytes();
        output[index * 2] = value[0];
        output[index * 2 + 1] = value[1];
        index += 1;
    }
    output
}

#[inline(never)]
fn variable_input(seed: u64) -> [u8; 128] {
    let mut output = [0u8; 128];
    let mut cursor = 0usize;
    let mut index = 0usize;
    while index < 32 {
        let length = [1usize, 5, 2, 4][index % 4];
        output[cursor] = length as u8;
        let mut item = 0usize;
        while item < length {
            output[cursor + 1 + item] = seed.wrapping_add(index as u64 + item as u64) as u8;
            item += 1;
        }
        cursor += 1 + length;
        index += 1;
    }
    output
}

#[inline(never)]
fn cursor_input(seed: u64) -> [u8; 7] {
    let length = ((seed >> 3) as usize % 4 + 1) as u8;
    [
        seed as u8,
        length,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
        9,
    ]
}
