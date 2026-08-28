use std::collections::{BTreeMap, BTreeSet};

use object::SectionIndex;

use super::{Symbol, analyze_bytes, disassembler};

#[test]
fn x86_direct_jump_is_a_tail_edge() {
    let target = 0x1005;
    let symbols = symbols(target);
    let analyzer = disassembler(object::Architecture::X86_64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0xe9, 0, 0, 0, 0],
        0x1000,
        &symbols,
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.internal_calls, [target]);
}

#[test]
fn aarch64_direct_branch_is_a_tail_edge() {
    let target = 0x2004;
    let symbols = symbols(target);
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0x01, 0x00, 0x00, 0x14],
        0x2000,
        &symbols,
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.internal_calls, [target]);
}

#[test]
fn x86_indirect_jump_is_gateable_dispatch() {
    let analyzer = disassembler(object::Architecture::X86_64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0xff, 0xe0],
        0x3000,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.indirect_calls, 1);
}

#[test]
fn aarch64_indirect_branch_is_gateable_dispatch() {
    let analyzer = disassembler(object::Architecture::Aarch64).unwrap();
    let metrics = analyze_bytes(
        &analyzer,
        &[0x00, 0x00, 0x1f, 0xd6],
        0x4000,
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
    .unwrap();

    assert_eq!(metrics.tail_calls, 1);
    assert_eq!(metrics.indirect_calls, 1);
}

fn symbols(address: u64) -> BTreeMap<u64, Symbol> {
    BTreeMap::from([(
        address,
        Symbol {
            name: "helper".to_owned(),
            address,
            size: 4,
            section: SectionIndex(0),
        },
    )])
}
