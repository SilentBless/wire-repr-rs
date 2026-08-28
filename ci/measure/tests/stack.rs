use wire_repr_measure::measure::artifact::stack::{StackError, stack_bytes};

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
fn ignores_x86_stack_pointer_used_only_as_a_source() {
    let assembly = r#"
measure_entry:
    .cfi_startproc
    subq $32, %rsp # allocate frame
    addq %rsp, %r11 # rsp is only a source
    subq %rsp, %rax
    andq %rsp, %r10
    addq 8(%rsp,%rax,4), %r9
    addq $32, %rsp # release frame
    retq
    .cfi_endproc
"#;

    assert_eq!(stack_bytes(assembly, "measure_entry").unwrap(), 32);
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
    sub sp, sp, #144 // unrelated %rsp, comment
    add x29, sp, #128 // unrelated %rsp
    .cfi_def_cfa w29, 16
    add sp, sp, #144 // unrelated %rsp
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
fn rejects_dynamic_x86_stack_adjustments() {
    for instruction in [
        "subq %r11, %rsp",
        "subq %r11, %rsp # $32 is commentary",
        "subq %r11, %rsp # dynamic",
        "andq $-16, %rsp",
        "andq %r11, %rsp",
        "subq 8(%rax,%rcx,4), %rsp",
        "addq (%rax,%rcx,8), %rsp # dynamic, indexed",
    ] {
        let assembly = format!(
            "measure_entry:\n    .cfi_startproc\n    {instruction}\n    retq\n    .cfi_endproc\n"
        );
        assert!(
            matches!(
                stack_bytes(&assembly, "measure_entry"),
                Err(StackError::InvalidStack { .. })
            ),
            "{instruction}"
        );
    }
}

#[test]
fn rejects_missing_assembly_symbol() {
    let error = stack_bytes("other:\n  retq\n", "measure_entry").unwrap_err();
    assert!(error.to_string().contains("measure_entry"));
}
