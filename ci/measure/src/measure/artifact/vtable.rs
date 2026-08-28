use std::collections::{BTreeMap, BTreeSet};

use super::{
    ResolvedRelocation, Symbol, analyze_bytes, disassembler, elf_dynamic_value, record_vtable,
};

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
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.vtable_references, 1);
}

#[test]
fn unreachable_x86_indirect_call_is_not_counted() {
    let analyzer = disassembler(object::Architecture::X86_64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0xeb, 0x06, 0xff, 0x15, 0, 0, 0, 0, 0xc3],
        0x1000,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.instructions, 2);
    assert_eq!(metrics.indirect_calls, 0);
}

#[test]
fn x86_relocated_linkage_call_is_not_dynamic_dispatch() {
    let analyzer = disassembler(object::Architecture::X86_64).unwrap();
    let target = 0x2000;
    let metrics = analyze_bytes(
        &analyzer,
        &[0xff, 0x15, 0, 0, 0, 0, 0xc3],
        0x1000,
        &BTreeMap::from([(
            target,
            Symbol {
                name: "linked_target".to_owned(),
                address: target,
                size: 1,
                section: object::SectionIndex(0),
            },
        )]),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeMap::from([(0x1006, target)]),
    )
    .unwrap();

    assert_eq!(metrics.direct_calls, 1);
    assert_eq!(metrics.indirect_calls, 0);
    assert_eq!(metrics.internal_calls, vec![target]);
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
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.vtable_references, 1);
}

#[test]
fn aarch64_tbz_uses_the_last_immediate_as_its_target() {
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[
            0x40, 0x00, 0x28, 0x36, 0x20, 0x00, 0x1f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6,
        ],
        0x1000,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.instructions, 3);
    assert_eq!(metrics.branches, 2);
    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.indirect_calls, 1);
}

#[test]
fn aarch64_authenticated_branch_is_terminal_dispatch() {
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[
            0x01, 0x08, 0x1f, 0xd7, 0x40, 0x00, 0x3f, 0xd6, 0xc0, 0x03, 0x5f, 0xd6,
        ],
        0x1000,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.instructions, 1);
    assert_eq!(metrics.branches, 1);
    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.indirect_calls, 1);
}

#[test]
fn aarch64_adrp_page_base_is_not_an_exact_vtable_reference() {
    let address = 0x1_0000_18b0;
    let page_base = 0x1_0049_c000;
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0xc1, 0x24, 0x00, 0xf0, 0x21, 0x60, 0x11, 0x91],
        address,
        &BTreeMap::new(),
        &BTreeSet::from([page_base]),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.vtable_references, 0);
}

#[test]
fn relocated_pointer_to_vtable_is_detected() {
    let candidate = 0x3000;
    let mut references = BTreeSet::new();
    record_vtable(
        0x2000,
        &BTreeSet::from([candidate]),
        &BTreeMap::from([(0x2000, candidate)]),
        &mut references,
    );

    assert_eq!(references, BTreeSet::from([candidate]));
}

#[test]
fn unresolved_elf_symbols_do_not_become_linkage_targets_from_addends() {
    for relocation in [
        object::elf::R_X86_64_64,
        object::elf::R_X86_64_GLOB_DAT,
        object::elf::R_X86_64_JUMP_SLOT,
    ] {
        assert_eq!(elf_dynamic_value(relocation, None, 0x1234), None);
    }
    assert_eq!(
        elf_dynamic_value(object::elf::R_X86_64_GLOB_DAT, Some(0x2000), 0x1234,),
        Some(ResolvedRelocation::Linkage(0x2000))
    );
}
