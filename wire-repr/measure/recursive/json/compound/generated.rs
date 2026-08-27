#![allow(dead_code)]

use wire_repr::WireView;

const COUNT: u16 = 200;

#[derive(WireView)]
struct Null {}

#[derive(WireView)]
struct Bool {
    value: u8,
}

#[derive(WireView)]
struct Array<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 0)]
    Null(Null),

    #[wire(value = 1)]
    Bool(Bool),
    #[wire(value = 2)]
    Array(Array<Value>),
}
pub fn direct_batch(seed: u64) -> u64 {
    let input = opaque_input(seed, true);
    let root = match Value::view::<64>(input.as_slice()) {
        Ok(root) => root,
        Err(_) => return u64::MAX,
    };
    let ValueVariant::Array(array) = root.variant() else {
        return u64::MAX;
    };
    let items = array.items();
    let mut hash = seed;
    for query in 0..COUNT {
        let index = (usize::from(query) * 37 + seed as usize) % usize::from(COUNT);
        let item = match items.get(index) {
            Ok(Some(item)) => item,
            Ok(None) | Err(_) => return u64::MAX,
        };
        hash = hash.rotate_left(7) ^ observe::<64, _>(item);
    }
    hash
}

pub fn direct_lookup(seed: u64) -> u64 {
    lookup(seed, true).unwrap_or(u64::MAX)
}

pub fn replay_lookup(seed: u64) -> u64 {
    lookup(seed, false).unwrap_or(u64::MAX)
}

fn lookup(seed: u64, periodic: bool) -> Result<u64, ()> {
    let input = opaque_input(seed, periodic);
    let root = Value::view::<64>(input.as_slice()).map_err(|_| ())?;
    let ValueVariant::Array(array) = root.variant() else {
        return Err(());
    };
    let item = array
        .items()
        .get(seed as usize % usize::from(COUNT))
        .map_err(|_| ())?
        .ok_or(())?;
    Ok(observe::<64, _>(item))
}

pub fn iterate(seed: u64) -> u64 {
    let input = opaque_input(seed, false);
    let root = match Value::view::<64>(input.as_slice()) {
        Ok(root) => root,
        Err(_) => return u64::MAX,
    };
    let ValueVariant::Array(array) = root.variant() else {
        return u64::MAX;
    };
    let mut hash = seed;
    for item in array.items().iter() {
        let Ok(item) = item else {
            return u64::MAX;
        };
        hash = hash.rotate_left(7) ^ observe::<64, _>(item);
    }
    hash
}

fn observe<const DEPTH: usize, V: ValueView<DEPTH>>(value: V) -> u64 {
    match value.variant() {
        ValueVariant::Null(_) => 0,
        ValueVariant::Bool(value) => u64::from(value.value()) + 1,
        ValueVariant::Array(value) => value.count() as u64 + 3,
    }
}

struct Input {
    bytes: [u8; 512],
    len: usize,
}

impl Input {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_input(seed: u64, periodic: bool) -> Input {
    input(seed, periodic)
}

fn input(seed: u64, periodic: bool) -> Input {
    let mut input = Input {
        bytes: [0; 512],
        len: 0,
    };
    push(&mut input, 2);
    push_bytes(&mut input, &COUNT.to_le_bytes());
    for index in 0..COUNT {
        let class = mix(seed ^ u64::from(index)) % 10;
        if periodic {
            if (index / 20) % 2 == 0 {
                push(&mut input, 1);
                push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
            } else {
                push(&mut input, 0);
            }
        } else if class == 0 {
            push(&mut input, 2);
            push_bytes(&mut input, &2u16.to_le_bytes());
            push(&mut input, 0);
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        } else if class.is_multiple_of(2) {
            push(&mut input, 0);
        } else {
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        }
    }
    input
}

fn push(input: &mut Input, value: u8) {
    input.bytes[input.len] = value;
    input.len += 1;
}

fn push_bytes(input: &mut Input, value: &[u8]) {
    input.bytes[input.len..input.len + value.len()].copy_from_slice(value);
    input.len += value.len();
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
