#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

const COUNT: u16 = 200;

#[derive(WireView, WireBuilder)]
struct Null {}

#[derive(WireView, WireBuilder)]
struct Bool {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Bytes {
    len: u8,
    #[wire(bytes = len)]
    value: wire_repr::wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Array<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 0)]
    Null(Null),

    #[wire(value = 1)]
    Bool(Bool),
    #[wire(value = 2)]
    Array(Array<Value>),
    #[wire(value = 3)]
    Bytes(Bytes),
}

#[derive(WireView, WireBuilder)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    opcode: u8,
    right: wire_repr::wire::Recursive<T>,
}

#[derive(WireView, WireBuilder)]
struct DecoratedPair<T> {
    prefix: u8,
    left: wire_repr::wire::Recursive<T>,
    #[wire(le)]
    opcode: u16,
    right: wire_repr::wire::Recursive<T>,
    suffix: u8,
}
#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum PairValue {
    #[wire(value = 1)]
    Leaf(Bool),
    #[wire(value = 4)]
    Pair(Pair<PairValue>),
    #[wire(value = 5)]
    Decorated(DecoratedPair<PairValue>),
}

#[derive(WireView, WireBuilder)]
struct DemandPair<T> {
    #[wire(le)]
    metadata_len: u64,
    left: wire_repr::wire::Recursive<T>,
    #[wire(bytes = metadata_len)]
    metadata: wire_repr::wire::Bytes,
    #[wire(le)]
    opcode: u16,
    right: wire_repr::wire::Recursive<T>,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum DemandValue {
    #[wire(value = 1)]
    Leaf(Bool),
    #[wire(value = 6)]
    Pair(DemandPair<DemandValue>),
}

pub fn demand_view(seed: u64) -> u64 {
    let input = opaque_demand_input(seed);
    let view = match DemandValue::view::<32>(input.as_slice()) {
        Ok(view) => view,
        Err(_) => return u64::MAX,
    };
    let DemandValueVariant::Pair(pair) = view.variant() else {
        return u64::MAX;
    };
    let left = match pair.left() {
        Ok(left) => left,
        Err(_) => return u64::MAX,
    };
    let right = match pair.right() {
        Ok(right) => right,
        Err(_) => return u64::MAX,
    };
    let (DemandValueVariant::Leaf(left), DemandValueVariant::Leaf(right)) =
        (left.variant(), right.variant())
    else {
        return u64::MAX;
    };
    hash_bytes(pair.metadata())
        ^ (u64::from(pair.opcode()) << 32)
        ^ (u64::from(left.value()) << 8)
        ^ u64::from(right.value())
}

pub fn demand_build(seed: u64) -> u64 {
    let (metadata, metadata_len) = opaque_demand_metadata(seed);
    let metadata = &metadata[..metadata_len];
    let mut output = [0u8; 64];
    let complete = match DemandValue::builder(&mut output[..]).pair(|pair| {
        let pair = pair.left(|value| value.leaf(|leaf| leaf.value(seed as u8)))?;
        let pair = pair.metadata(metadata)?;
        let pair = pair.opcode((seed as u16).rotate_left(3))?;
        pair.right(|value| value.leaf(|leaf| leaf.value((seed as u8).wrapping_add(1))))
    }) {
        Ok(complete) => complete,
        Err(_) => return u64::MAX,
    };
    match complete.finish() {
        Ok(written) => hash_bytes(written.as_bytes()),
        Err(_) => u64::MAX,
    }
}

#[inline(never)]
fn opaque_demand_input(seed: u64) -> PairInput {
    let metadata_len = seed as usize % 16;
    let mut input = PairInput {
        bytes: [0; 256],
        len: 0,
    };
    input.bytes[0] = 6;
    input.bytes[1..9].copy_from_slice(&(metadata_len as u64).to_le_bytes());
    input.bytes[9..11].copy_from_slice(&[1, seed as u8]);
    for index in 0..metadata_len {
        input.bytes[11 + index] = (seed as u8).wrapping_mul(3).wrapping_add(index as u8);
    }
    let opcode = 11 + metadata_len;
    input.bytes[opcode..opcode + 2].copy_from_slice(&(seed as u16).rotate_left(3).to_le_bytes());
    input.bytes[opcode + 2..opcode + 4].copy_from_slice(&[1, (seed as u8).wrapping_add(1)]);
    input.len = opcode + 4;
    input
}

#[inline(never)]
fn opaque_demand_metadata(seed: u64) -> ([u8; 16], usize) {
    let metadata_len = seed as usize % 16;
    let mut metadata = [0u8; 16];
    for index in 0..metadata_len {
        metadata[index] = (seed as u8).wrapping_mul(3).wrapping_add(index as u8);
    }
    (metadata, metadata_len)
}
pub fn pair_view(seed: u64) -> u64 {
    let input = opaque_pair_input(seed);
    let root = match PairValue::view::<64>(input.as_slice()) {
        Ok(root) => root,
        Err(_) => return u64::MAX,
    };
    let PairValueVariant::Pair(pair) = root.variant() else {
        return u64::MAX;
    };
    let opcode = pair.opcode();
    let right = match pair.right() {
        Ok(right) => right,
        Err(_) => return u64::MAX,
    };
    let PairValueVariant::Leaf(right) = right.variant() else {
        return u64::MAX;
    };
    (u64::from(opcode) << 32) | (u64::from(right.value()) << 16) | input.len as u64
}

pub fn pair_build(seed: u64) -> u64 {
    let mut output = [0u8; 256];
    let depth = 4 + seed as u8 % 8;
    let complete = match write_pair(PairValue::builder(&mut output[..]), depth, seed as u8) {
        Ok(complete) => complete,
        Err(_) => return u64::MAX,
    };
    let written = match complete.finish() {
        Ok(written) => written,
        Err(_) => return u64::MAX,
    };
    hash_bytes(written.as_bytes())
}

fn write_pair<Cursor>(
    writer: PairValueWriter<Cursor>,
    depth: u8,
    seed: u8,
) -> Result<
    PairValueWriterComplete<Cursor>,
    wire_repr::WriteError<
        wire_repr::__private::RecursiveWriteError,
        <Cursor as wire_repr::__private::RecursiveCursor>::GrowError,
    >,
>
where
    Cursor: wire_repr::__private::RecursiveCursor,
{
    if depth == 0 {
        return writer.leaf(|leaf| leaf.value(seed));
    }
    writer.pair(|pair| {
        let pair = pair.left(|value| write_pair(value, depth - 1, seed))?;
        let pair = pair.opcode(seed ^ depth)?;
        pair.right(|value| value.leaf(|leaf| leaf.value(seed.wrapping_add(depth))))
    })
}

pub fn array_build(seed: u64) -> u64 {
    let mut output = [0u8; 256];
    let count = 16 + seed as usize % 16;
    let complete = match Value::builder(&mut output[..]).array(|array| {
        array.items(|mut items| {
            for index in 0..count {
                items = items.item(|value| {
                    value.bool(|body| body.value((seed as u8).wrapping_add(index as u8)))
                })?;
            }
            Ok(items)
        })
    }) {
        Ok(complete) => complete,
        Err(_) => return u64::MAX,
    };
    let written = match complete.finish() {
        Ok(written) => written,
        Err(_) => return u64::MAX,
    };
    hash_bytes(written.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

struct PairInput {
    bytes: [u8; 256],
    len: usize,
}

impl PairInput {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_pair_input(seed: u64) -> PairInput {
    let depth = 16 + seed as usize % 16;
    let mut input = PairInput {
        bytes: [0; 256],
        len: 0,
    };
    for _ in 0..depth {
        input.bytes[input.len] = 4;
        input.len += 1;
    }
    input.bytes[input.len..input.len + 2].copy_from_slice(&[1, seed as u8]);
    input.len += 2;
    for index in 0..depth {
        input.bytes[input.len..input.len + 3].copy_from_slice(&[
            index as u8,
            1,
            (seed as u8).wrapping_add(index as u8).wrapping_add(1),
        ]);
        input.len += 3;
    }
    input
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

pub fn mixed_lookup(seed: u64) -> u64 {
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
        ValueVariant::Bytes(value) => value.value().len() as u64 + 5,
    }
}

pub fn fixed_mode(seed: u64) -> u64 {
    mode_lookup(seed, 0)
}

pub fn formula_mode(seed: u64) -> u64 {
    mode_lookup(seed, 1)
}

pub fn interval_mode(seed: u64) -> u64 {
    mode_lookup(seed, 2)
}

pub fn ranked_mode(seed: u64) -> u64 {
    mode_lookup(seed, 3)
}

pub fn factorized_mode(seed: u64) -> u64 {
    mode_lookup(seed, 4)
}

pub fn factorized_fallback_mode(seed: u64) -> u64 {
    mode_lookup(seed, 9)
}

pub fn shape_mode(seed: u64) -> u64 {
    mode_lookup(seed, 5)
}

pub fn periodic_mode(seed: u64) -> u64 {
    mode_lookup(seed, 6)
}

pub fn packed_mode(seed: u64) -> u64 {
    mode_lookup(seed, 7)
}

pub fn replay_mode(seed: u64) -> u64 {
    mode_lookup(seed, 8)
}

fn mode_lookup(seed: u64, mode: u8) -> u64 {
    let input = opaque_mode_input(seed, mode);
    let root = match Value::view::<64>(input.as_slice()) {
        Ok(root) => root,
        Err(_) => return u64::MAX,
    };
    let ValueVariant::Array(array) = root.variant() else {
        return u64::MAX;
    };
    let items = array.items();
    if items.geometry_kind() != mode_kind(mode) {
        return u64::MAX;
    }
    let index = seed as usize % items.len();
    match items.get(index) {
        Ok(Some(item)) => observe::<64, _>(item),
        Ok(None) | Err(_) => u64::MAX,
    }
}

fn mode_kind(mode: u8) -> &'static str {
    match mode {
        0 => "fixed",
        1 => "exact_formula",
        2 => "interval_events",
        3 => "ranked_palette",
        4 => "factorized",
        5 => "recursive_shape",
        6 => "periodic_palette",
        7 => "packed_runs",
        8 => "replay",
        9 => "factorized",
        _ => "invalid",
    }
}

struct ModeInput {
    bytes: [u8; 32_768],
    len: usize,
}

impl ModeInput {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_mode_input(seed: u64, mode: u8) -> ModeInput {
    mode_input(seed, mode)
}

fn mode_input(seed: u64, mode: u8) -> ModeInput {
    let mut input = ModeInput {
        bytes: [0; 32_768],
        len: 0,
    };
    push_mode(&mut input, 2);
    let count_offset = input.len;
    push_mode_bytes(&mut input, &[0; 2]);
    let count = match mode {
        0 => {
            for _ in 0..128 {
                push_mode(&mut input, 0);
            }
            128
        }
        1 => {
            for index in 0..128 {
                push_mode_width(&mut input, 3 + index);
            }
            128
        }
        2 => {
            let mut count = 0;
            for run in 0..12 {
                for _ in 0..=run {
                    push_mode_width(&mut input, 5 + run * 3);
                    count += 1;
                }
            }
            count
        }
        3 => {
            for index in 0..200 {
                push_mode_width(&mut input, 3 + (mix(seed ^ index) % 50) as usize);
            }
            200
        }
        4 => {
            for index in 0..300usize {
                let variant = 2 + ((index % 16) * 7) % 13;
                let depth_class = (index / 16) % 64;
                let depth = (depth_class * 5 + depth_class / 7) % 17;
                push_mode_width(&mut input, 8 + variant + depth);
            }
            300
        }
        5 => {
            for index in 0..300 {
                push_mode(&mut input, 2);
                if index % 2 == 0 {
                    push_mode_bytes(&mut input, &0u16.to_le_bytes());
                } else {
                    push_mode_bytes(&mut input, &1u16.to_le_bytes());
                    push_mode(&mut input, 0);
                }
            }
            300
        }
        6 => {
            for index in 0..200 {
                push_mode(&mut input, if (index / 20) % 2 == 0 { 1 } else { 0 });
                if (index / 20) % 2 == 0 {
                    push_mode(&mut input, index as u8 & 1);
                }
            }
            200
        }
        7 => {
            let mut previous = usize::MAX;
            for run in 0..256usize {
                let mut class = (mix(seed ^ run as u64) % 50) as usize;
                if class == previous {
                    class = (class + 1) % 50;
                }
                previous = class;
                for _ in 0..2 {
                    push_mode_width(&mut input, 3 + class);
                }
            }
            512
        }
        8 => {
            for index in 0..600 {
                push_mode_width(&mut input, 3 + (mix(seed ^ index) % 40) as usize);
            }
            600
        }
        9 => {
            for index in 0..128usize {
                let low = ((index % 16) * 7) % 16;
                let high = ((index / 16) % 8) * 16;
                push_mode_width(&mut input, 3 + low + high);
            }
            128
        }
        _ => 0,
    };
    input.bytes[count_offset..count_offset + 2].copy_from_slice(&(count as u16).to_le_bytes());
    input
}

fn push_mode_width(input: &mut ModeInput, width: usize) {
    if width == 1 {
        push_mode(input, 0);
        return;
    }
    push_mode(input, 3);
    push_mode(input, (width - 2) as u8);
    for _ in 0..width - 2 {
        push_mode(input, 0);
    }
}

fn push_mode(input: &mut ModeInput, value: u8) {
    input.bytes[input.len] = value;
    input.len += 1;
}

fn push_mode_bytes(input: &mut ModeInput, value: &[u8]) {
    input.bytes[input.len..input.len + value.len()].copy_from_slice(value);
    input.len += value.len();
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
