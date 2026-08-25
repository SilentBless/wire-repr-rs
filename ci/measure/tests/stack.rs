use wire_repr_measure::measure::artifact::stack::stack_bytes;

#[test]
fn measures_maximum_cfi_frame_for_exact_symbol() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    pushq %rbp
    .cfi_def_cfa_offset 16
    .cfi_def_cfa_register %rbp
    subq $64, %rsp
    .cfi_def_cfa_offset 80
    addq $64, %rsp
    .cfi_def_cfa_offset 16
    popq %rbp
    .cfi_def_cfa_offset 8
    retq
    .cfi_endproc
other:
    .cfi_startproc
    .cfi_def_cfa_offset 256
    retq
    .cfi_endproc
"#;

    assert_eq!(stack_bytes(assembly, "measure_entry").unwrap(), 72);
}
#[test]
fn measures_register_based_cfi_frames() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    stp x29, x30, [sp, #-32]!
    .cfi_def_cfa x29, 32
    ret
    .cfi_endproc
"#;

    assert_eq!(stack_bytes(assembly, "measure_entry").unwrap(), 32);
}

#[test]
fn measures_split_stack_and_frame_pointer_prologues() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    sub sp, sp, #144
    add x29, sp, #128
    .cfi_def_cfa w29, 16
    add sp, sp, #144
    ret
    .cfi_endproc
"#;

    assert_eq!(stack_bytes(assembly, "measure_entry").unwrap(), 144);
}

#[test]
fn measures_shifted_aarch64_stack_immediates() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    sub sp, sp, #1, lsl #12
    add sp, sp, #1, lsl #12
    ret
    .cfi_endproc
other_entry:
    .cfi_startproc
    sub sp, sp, #0x2, lsl #12
    ret
    .cfi_endproc
"#;

    assert_eq!(stack_bytes(assembly, "measure_entry").unwrap(), 4096);
    assert_eq!(stack_bytes(assembly, "other_entry").unwrap(), 8192);
}

#[test]
fn rejects_dynamic_stack_adjustments() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    sub sp, sp, x0
    ret
    .cfi_endproc
"#;

    assert!(stack_bytes(assembly, "measure_entry").is_err());
}

#[test]
fn rejects_missing_assembly_symbol() {
    let error = stack_bytes("other:\n  retq\n", "measure_entry").unwrap_err();
    assert!(error.to_string().contains("measure_entry"));
}
