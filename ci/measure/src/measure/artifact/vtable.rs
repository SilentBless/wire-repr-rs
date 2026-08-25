use std::collections::{BTreeMap, BTreeSet};

use super::{analyze_bytes, disassembler};

#[test]
fn x86_rip_relative_vtable_reference_is_detected() {
    let address = 0x1000;
    let candidate = 0x1007;
    let analyzer = disassembler(object::Architecture::X86_64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0x48, 0x8d, 0x05, 0, 0, 0, 0],
        address,
        &BTreeMap::new(),
        &BTreeSet::from([candidate]),
    )
    .unwrap();

    assert_eq!(metrics.vtable_references, 1);
}

#[test]
fn aarch64_adrp_add_vtable_reference_is_detected() {
    let address = 0x1_0000_18b0;
    let candidate = 0x1_0049_c458;
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0xc1, 0x24, 0x00, 0xf0, 0x21, 0x60, 0x11, 0x91],
        address,
        &BTreeMap::new(),
        &BTreeSet::from([candidate]),
    )
    .unwrap();

    assert_eq!(metrics.vtable_references, 1);
}
