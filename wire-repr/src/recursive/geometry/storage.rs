// moved bounded storage layout
pub(super) const STORAGE_BYTES: usize = 356;
pub(super) const PALETTE_CAPACITY: usize = 64;
pub(super) const PERIOD_CAPACITY: usize = 64;
pub(super) const INTERVAL_CAPACITY: usize = 24;
pub(super) const RANKED_CAPACITY: usize = 256;
pub(super) const RANKED_BLOCK: usize = 32;
pub(super) const PACKED_RUN_CAPACITY: usize = 256;
pub(super) const PACKED_RUN_BLOCK: usize = 64;

pub(super) const GEOMETRY_REPLAY: u8 = 0;
pub(super) const GEOMETRY_FIXED: u8 = 1;
pub(super) const GEOMETRY_FORMULA: u8 = 2;
pub(super) const GEOMETRY_INTERVAL: u8 = 3;
pub(super) const GEOMETRY_RANKED: u8 = 4;
pub(super) const GEOMETRY_FACTORIZED: u8 = 5;
pub(super) const GEOMETRY_RECURSIVE_SHAPE: u8 = 6;
pub(super) const GEOMETRY_PERIODIC: u8 = 7;
pub(super) const GEOMETRY_PACKED_RUNS: u8 = 8;

pub(super) const SHAPE_HASHES: usize = 0;
pub(super) const SHAPE_WIDTHS: usize = SHAPE_HASHES + PALETTE_CAPACITY * 2;
pub(super) const SHAPE_CODES: usize = SHAPE_WIDTHS + PALETTE_CAPACITY * 2;

pub(super) const RANKED_PALETTE: usize = 0;
pub(super) const RANKED_CODES: usize = RANKED_PALETTE + PALETTE_CAPACITY * 2;
pub(super) const RANKED_PREFIXES: usize = RANKED_CODES + RANKED_CAPACITY * 6 / 8;

pub(super) const PACKED_PALETTE: usize = 0;
pub(super) const PACKED_CODES: usize = PACKED_PALETTE + PALETTE_CAPACITY * 2;
pub(super) const PACKED_PREFIXES: usize = PACKED_CODES + PACKED_RUN_CAPACITY * 6 / 8;

pub(super) const FACTOR_LENGTHS: [usize; 4] = [16, 64, 8, 32];
pub(super) const FACTOR_BLOCKS: [usize; 4] = [1, 16, 1_024, 8_192];
pub(super) const FACTOR_OFFSETS: [usize; 4] = [0, 16, 80, 88];
pub(super) const FACTOR_COMPONENTS: usize = 120;
pub(super) const FACTOR_INITIALIZED: usize = FACTOR_COMPONENTS * 2;
pub(super) fn factor_digits(index: usize) -> [usize; 4] {
    [
        index % FACTOR_LENGTHS[0],
        (index / FACTOR_BLOCKS[1]) % FACTOR_LENGTHS[1],
        (index / FACTOR_BLOCKS[2]) % FACTOR_LENGTHS[2],
        (index / FACTOR_BLOCKS[3]) % FACTOR_LENGTHS[3],
    ]
}

pub(super) fn factor_component(storage: &[u8; STORAGE_BYTES], axis: usize, class: usize) -> i32 {
    i32::from(get_i16(storage, (FACTOR_OFFSETS[axis] + class) * 2))
}

pub(super) fn factor_initialized(storage: &[u8; STORAGE_BYTES], axis: usize, class: usize) -> bool {
    let component = FACTOR_OFFSETS[axis] + class;
    storage[FACTOR_INITIALIZED + component / 8] & (1 << (component % 8)) != 0
}

pub(super) fn set_factor_component(
    storage: &mut [u8; STORAGE_BYTES],
    axis: usize,
    class: usize,
    value: i16,
) {
    let component = FACTOR_OFFSETS[axis] + class;
    put_i16(storage, component * 2, value);
    storage[FACTOR_INITIALIZED + component / 8] |= 1 << (component % 8);
}

pub(super) fn get_code(storage: &[u8; STORAGE_BYTES], base: usize, index: usize) -> usize {
    let bit = index * 6;
    let byte = base + bit / 8;
    let shift = bit % 8;
    let low = u16::from(storage[byte]);
    let high = storage.get(byte + 1).copied().map_or(0, u16::from);
    usize::from(((low | high << 8) >> shift) & 0x3f)
}

pub(super) fn set_code(storage: &mut [u8; STORAGE_BYTES], base: usize, index: usize, code: usize) {
    let bit = index * 6;
    let byte = base + bit / 8;
    let shift = bit % 8;
    let mut value = u16::from(storage[byte]);
    if byte + 1 < storage.len() {
        value |= u16::from(storage[byte + 1]) << 8;
    }
    let mask = 0x3fu16 << shift;
    value = (value & !mask) | ((code as u16) << shift);
    storage[byte] = value as u8;
    if shift > 2 {
        storage[byte + 1] = (value >> 8) as u8;
    }
}

pub(super) fn get_u16(storage: &[u8; STORAGE_BYTES], offset: usize) -> u16 {
    u16::from_le_bytes([storage[offset], storage[offset + 1]])
}

pub(super) fn put_u16(storage: &mut [u8; STORAGE_BYTES], offset: usize, value: u16) {
    storage[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn get_u32(storage: &[u8; STORAGE_BYTES], offset: usize) -> u32 {
    u32::from_le_bytes(storage[offset..offset + 4].try_into().expect("u32 slot"))
}

pub(super) fn put_u32(storage: &mut [u8; STORAGE_BYTES], offset: usize, value: u32) {
    storage[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn get_i16(storage: &[u8; STORAGE_BYTES], offset: usize) -> i16 {
    i16::from_le_bytes([storage[offset], storage[offset + 1]])
}

pub(super) fn put_i16(storage: &mut [u8; STORAGE_BYTES], offset: usize, value: i16) {
    storage[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
