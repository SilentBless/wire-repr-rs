use super::Operation;

// The first six registers are callee-saved under both supported x86-64 ABIs.
const CALLEE_SAVED: usize = 6;
const REGISTERS: [&[&str]; 15] = [
    &["rbx", "ebx", "bx", "bl", "bh"],
    &["rbp", "ebp", "bp", "bpl"],
    &["r12", "r12d", "r12w", "r12b"],
    &["r13", "r13d", "r13w", "r13b"],
    &["r14", "r14d", "r14w", "r14b"],
    &["r15", "r15d", "r15w", "r15b"],
    &["rax", "eax", "ax", "al", "ah"],
    &["rcx", "ecx", "cx", "cl", "ch"],
    &["rdx", "edx", "dx", "dl", "dh"],
    &["rsi", "esi", "si", "sil"],
    &["rdi", "edi", "di", "dil"],
    &["r8", "r8d", "r8w", "r8b"],
    &["r9", "r9d", "r9w", "r9b"],
    &["r10", "r10d", "r10w", "r10b"],
    &["r11", "r11d", "r11w", "r11b"],
];

#[derive(Default)]
pub(super) struct CachedLinkage<'a> {
    sources: [Option<&'a str>; REGISTERS.len()],
}

impl<'a> CachedLinkage<'a> {
    pub(super) fn clear(&mut self) {
        self.sources.fill(None);
    }

    pub(super) fn source(&self, operation: &Operation) -> Option<&'a str> {
        let register = operation
            .disasm
            .strip_prefix("call ")
            .or_else(|| operation.disasm.strip_prefix("jmp "))?;
        REGISTERS
            .iter()
            .position(|aliases| aliases[0] == register)
            .and_then(|index| self.sources[index])
    }

    pub(super) fn observe(&mut self, operation: &'a Operation, linkage: bool) {
        if operation.esil.is_empty() || matches!(operation.kind.as_str(), "unk" | "ill") {
            self.clear();
            return;
        }
        if operation.kind.ends_with("call") {
            if !linkage && self.source(operation).is_none() {
                self.clear();
            } else {
                self.sources[CALLEE_SAVED..].fill(None);
            }
            return;
        }
        let copied_source = if operation.kind == "mov" {
            operation
                .disasm
                .strip_prefix("mov ")
                .and_then(|operands| operands.split_once(','))
                .and_then(|(_, source)| {
                    REGISTERS
                        .iter()
                        .position(|aliases| aliases[0] == source.trim())
                })
                .and_then(|index| self.sources[index])
        } else {
            None
        };
        let mut previous = "";
        for token in operation.esil.split(',') {
            if token.ends_with('=')
                && !matches!(token, "==" | "!=" | "<=" | ">=")
                && let Some(index) = REGISTERS
                    .iter()
                    .position(|aliases| aliases.contains(&previous))
            {
                self.sources[index] = None;
            }
            previous = token;
        }
        if operation.kind == "mov"
            && let Some(destination) = operation.disasm.strip_prefix("mov ")
            && let Some((register, _)) = destination.split_once(',')
            && let Some(index) = REGISTERS.iter().position(|aliases| aliases[0] == register)
        {
            self.sources[index] = if linkage {
                Some(&operation.disasm)
            } else {
                copied_source
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation(kind: &str, disasm: &str, esil: &str) -> Operation {
        serde_json::from_value(serde_json::json!({
            "type": kind,
            "disasm": disasm,
            "esil": esil,
        }))
        .unwrap()
    }

    #[test]
    fn reads_preserve_imports_but_unlisted_assignments_discard_them() {
        let load = operation("mov", "mov rbx, qword [reloc.memcpy]", "0x1000,[8],rbx,=");
        let call = operation("rcall", "call rbx", "rbx,rip,8,rsp,-=,rsp,=[],rip,=");
        let read = operation("mov", "mov rax, rbx", "rbx,rax,=");
        let compare = operation("cmp", "cmp rax, rbx", "rbx,rax,==");
        let update = operation("add", "inc rbx", "rbx,++=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        cached.observe(&read, false);
        cached.observe(&compare, false);
        assert_eq!(cached.source(&call), Some(load.disasm.as_str()));
        cached.observe(&update, false);
        assert_eq!(cached.source(&call), None);
    }

    #[test]
    fn opaque_nonempty_effects_and_unknown_calls_discard_provenance() {
        let load = operation("mov", "mov rbx, qword [reloc.memcpy]", "0x1000,[8],rbx,=");
        let known_call = operation("rcall", "call rbx", "rbx,rip,8,rsp,-=,rsp,=[],rip,=");
        let unknown_effect = operation("unk", "opaque", "CUSTOM_EFFECT");
        let unknown_call = operation("rcall", "call rax", "rax,rip,8,rsp,-=,rsp,=[],rip,=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        cached.observe(&unknown_effect, false);
        assert_eq!(cached.source(&known_call), None);
        cached.observe(&load, true);
        cached.observe(&unknown_call, false);
        assert_eq!(cached.source(&known_call), None);
    }

    #[test]
    fn callee_saved_import_survives_calls_but_not_alias_writes() {
        let load = operation("mov", "mov rbx, qword [reloc.memcpy]", "0x1000,[8],rbx,=");
        let call = operation("rcall", "call rbx", "rbx,rip,8,rsp,-=,rsp,=[],rip,=");
        let overwrite = operation("xor", "xor ebx, ebx", "0,ebx,=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        assert_eq!(cached.source(&call), Some(load.disasm.as_str()));
        cached.observe(&call, false);
        assert_eq!(cached.source(&call), Some(load.disasm.as_str()));
        cached.observe(&overwrite, false);
        assert_eq!(cached.source(&call), None);
    }

    #[test]
    fn unknown_semantics_and_block_boundaries_discard_provenance() {
        let load = operation("mov", "mov r12, qword [reloc.malloc]", "0x1000,[8],r12,=");
        let call = operation("rcall", "call r12", "r12,rip,8,rsp,-=,rsp,=[],rip,=");
        let unknown = operation("unk", "unknown", "");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        cached.observe(&unknown, false);
        assert_eq!(cached.source(&call), None);
        cached.observe(&load, true);
        cached.clear();
        assert_eq!(cached.source(&call), None);
    }

    #[test]
    fn copied_import_survives_source_overwrite_but_not_a_call() {
        let load = operation("mov", "mov r12, qword [reloc.memcpy]", "0x1000,[8],r12,=");
        let copy = operation("mov", "mov rax, r12", "r12,rax,=");
        let overwrite = operation("lea", "lea r12, qword [rsp+rbx]", "rbx,rsp,+,r12,=");
        let call = operation("rcall", "call rax", "rax,rip,8,rsp,-=,rsp,=[],rip,=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        cached.observe(&copy, false);
        cached.observe(&overwrite, false);
        assert_eq!(cached.source(&call), Some(load.disasm.as_str()));
        cached.observe(&call, false);
        assert_eq!(cached.source(&call), None);
    }

    #[test]
    fn partial_and_unnamed_register_copies_discard_import_provenance() {
        let load = operation("mov", "mov r12, qword [reloc.memcpy]", "0x1000,[8],r12,=");
        let copy = operation("mov", "mov rax, r12", "r12,rax,=");
        let narrow = operation("mov", "mov eax, r12d", "r12d,eax,=");
        let unnamed = operation("mov", "mov rax, rdi", "rdi,rax,=");
        let call = operation("rcall", "call rax", "rax,rip,8,rsp,-=,rsp,=[],rip,=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        cached.observe(&copy, false);
        cached.observe(&narrow, false);
        assert_eq!(cached.source(&call), None);
        cached.observe(&copy, false);
        cached.observe(&unnamed, false);
        assert_eq!(cached.source(&call), None);
    }

    #[test]
    fn a_direct_named_call_clobbers_caller_saved_provenance() {
        let load = operation("mov", "mov rax, qword [reloc.memcpy]", "0x1000,[8],rax,=");
        let direct_call = operation(
            "ircall",
            "call qword [reloc.memset]",
            "0x2000,[8],rip,8,rsp,-=,rsp,=[],rip,=",
        );
        let call = operation("rcall", "call rax", "rax,rip,8,rsp,-=,rsp,=[],rip,=");
        let mut cached = CachedLinkage::default();
        cached.observe(&load, true);
        assert_eq!(cached.source(&call), Some(load.disasm.as_str()));
        cached.observe(&direct_call, true);
        assert_eq!(cached.source(&call), None);
    }
}
