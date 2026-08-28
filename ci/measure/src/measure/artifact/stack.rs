use thiserror::Error;

#[derive(Debug, Error)]
pub enum StackError {
    #[error("assembly has no function `{0}`")]
    MissingSymbol(String),
    #[error("invalid CFI offset `{value}` in `{symbol}`")]
    InvalidOffset { symbol: String, value: String },
    #[error("invalid stack adjustment in `{symbol}`: `{instruction}`")]
    InvalidStack { symbol: String, instruction: String },
}

pub fn stack_bytes(assembly: &str, symbol: &str) -> Result<u64, StackError> {
    let labels = [format!("{symbol}:"), format!("_{symbol}:")];
    let mut inside = false;
    let mut stack = 0u64;
    let mut maximum_stack = 0u64;
    let mut saw_stack_adjustment = false;
    let mut cfa = 8i64;
    let mut maximum_cfa = None;
    let mut cfa_baseline = 8i64;

    for raw in assembly.lines() {
        let line = raw.trim();
        if !inside {
            if labels.iter().any(|label| line == label) {
                inside = true;
            }
            continue;
        }
        if line == ".cfi_endproc" {
            return Ok(finish(
                saw_stack_adjustment,
                maximum_stack,
                maximum_cfa,
                cfa_baseline,
            ));
        }

        if let Some(delta) = stack_delta(line, symbol)? {
            saw_stack_adjustment = true;
            if delta >= 0 {
                stack = stack.saturating_add(delta as u64);
                maximum_stack = maximum_stack.max(stack);
            } else {
                stack = stack.saturating_sub(delta.unsigned_abs());
            }
        }

        if line.starts_with("stp ")
            || line.starts_with("sub\tsp")
            || line.starts_with("sub sp")
            || line.contains(" x29")
            || line.contains(" w29")
        {
            cfa_baseline = 0;
        }
        if let Some(value) = line.strip_prefix(".cfi_def_cfa_offset ") {
            cfa = parse(value, symbol)?;
            maximum_cfa = Some(maximum_cfa.map_or(cfa, |value: i64| value.max(cfa)));
        } else if let Some(value) = line.strip_prefix(".cfi_def_cfa ") {
            let (register, value) = value
                .rsplit_once(',')
                .map_or(("", value), |(register, offset)| {
                    (register.trim(), offset.trim())
                });
            if matches!(register, "x29" | "w29" | "sp") {
                cfa_baseline = 0;
            }
            cfa = parse(value, symbol)?;
            maximum_cfa = Some(maximum_cfa.map_or(cfa, |value| value.max(cfa)));
        } else if let Some(value) = line.strip_prefix(".cfi_adjust_cfa_offset ") {
            cfa += parse(value, symbol)?;
            maximum_cfa = Some(maximum_cfa.map_or(cfa, |value| value.max(cfa)));
        }
    }

    if inside {
        Ok(finish(
            saw_stack_adjustment,
            maximum_stack,
            maximum_cfa,
            cfa_baseline,
        ))
    } else {
        Err(StackError::MissingSymbol(symbol.to_owned()))
    }
}

fn finish(
    saw_stack_adjustment: bool,
    maximum_stack: u64,
    maximum_cfa: Option<i64>,
    cfa_baseline: i64,
) -> u64 {
    if saw_stack_adjustment {
        maximum_stack
    } else {
        maximum_cfa.map_or(0, |maximum| {
            maximum.saturating_sub(cfa_baseline).max(0) as u64
        })
    }
}

fn stack_delta(line: &str, symbol: &str) -> Result<Option<i64>, StackError> {
    let compact = line.replace([' ', '\t'], "");
    let x86_instruction = x86_instruction(&compact).filter(|instruction| {
        instruction.starts_with("sub")
            || instruction.starts_with("add")
            || instruction.starts_with("and")
    });
    let x86_stack_destination = if let Some(instruction) = x86_instruction {
        Some(
            parse_x86_stack_destination(instruction).ok_or_else(|| StackError::InvalidStack {
                symbol: symbol.to_owned(),
                instruction: line.to_owned(),
            })?,
        )
    } else {
        None
    };
    let adjustment = if compact.starts_with("push") {
        Some(8)
    } else if compact.starts_with("pop") {
        Some(-8)
    } else if compact.starts_with("sub") && x86_stack_destination == Some(true) {
        x86_instruction
            .and_then(|instruction| number_after(instruction, '$'))
            .map(|value| value as i64)
    } else if compact.starts_with("add") && x86_stack_destination == Some(true) {
        x86_instruction
            .and_then(|instruction| number_after(instruction, '$'))
            .map(|value| -(value as i64))
    } else if compact.starts_with("subsp,sp,#") {
        Some(aarch64_immediate(&compact, line, symbol)? as i64)
    } else if compact.starts_with("addsp,sp,#") {
        Some(-(aarch64_immediate(&compact, line, symbol)? as i64))
    } else if compact.starts_with("subsp,sp,")
        || compact.starts_with("addsp,sp,")
        || (compact.starts_with("and") && x86_stack_destination == Some(true))
    {
        return Err(StackError::InvalidStack {
            symbol: symbol.to_owned(),
            instruction: line.to_owned(),
        });
    } else if (compact.starts_with("stp") || compact.starts_with("str"))
        && compact.contains("[sp,#-")
        && compact.contains("]!")
    {
        number_after(&compact, '-').map(|value| value as i64)
    } else if (compact.starts_with("ldp") || compact.starts_with("ldr"))
        && compact.contains("[sp],#")
    {
        number_after(&compact, '#').map(|value| -(value as i64))
    } else {
        return Ok(None);
    };
    adjustment
        .map(Some)
        .ok_or_else(|| StackError::InvalidStack {
            symbol: symbol.to_owned(),
            instruction: line.to_owned(),
        })
}

fn x86_instruction(instruction: &str) -> Option<&str> {
    let instruction = instruction
        .split_once("//")
        .map_or(instruction, |(instruction, _)| instruction);
    let instruction = instruction
        .split_once(';')
        .map_or(instruction, |(instruction, _)| instruction);
    let stack_pointer = instruction.find("%rsp")?;
    if instruction
        .find('#')
        .is_some_and(|comment| stack_pointer > comment)
    {
        return None;
    }
    Some(
        instruction
            .split_once('#')
            .map_or(instruction, |(instruction, _)| instruction),
    )
}

fn parse_x86_stack_destination(instruction: &str) -> Option<bool> {
    let mut depth = 0usize;
    let mut separator = None;
    for (index, character) in instruction.char_indices() {
        match character {
            '(' => depth = depth.checked_add(1)?,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                if separator.replace(index).is_some() {
                    return None;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let destination = &instruction[separator? + 1..];
    Some(destination == "%rsp")
}

fn aarch64_immediate(compact: &str, instruction: &str, symbol: &str) -> Result<u64, StackError> {
    let mut value = number_after(compact, '#').ok_or_else(|| StackError::InvalidStack {
        symbol: symbol.to_owned(),
        instruction: instruction.to_owned(),
    })?;
    if let Some((_, shift)) = compact.split_once(",lsl#") {
        if shift != "12" {
            return Err(StackError::InvalidStack {
                symbol: symbol.to_owned(),
                instruction: instruction.to_owned(),
            });
        }
        value = value
            .checked_shl(12)
            .ok_or_else(|| StackError::InvalidStack {
                symbol: symbol.to_owned(),
                instruction: instruction.to_owned(),
            })?;
    }
    Ok(value)
}

fn number_after(value: &str, marker: char) -> Option<u64> {
    let value = value.split_once(marker)?.1;
    let token = value.strip_prefix("0x").map_or_else(
        || {
            value
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        },
        |hex| {
            hex.chars()
                .take_while(char::is_ascii_hexdigit)
                .collect::<String>()
        },
    );
    if value.starts_with("0x") {
        u64::from_str_radix(&token, 16).ok()
    } else {
        token.parse().ok()
    }
}

fn parse(value: &str, symbol: &str) -> Result<i64, StackError> {
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .parse()
        .map_err(|_| StackError::InvalidOffset {
            symbol: symbol.to_owned(),
            value: value.to_owned(),
        })
}
