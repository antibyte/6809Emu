use std::fmt;

use m6809_core::{Cpu, CpuVariant, Memory};

/// A single disassembled instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisassembledInsn {
    pub address: u16,
    pub bytes: Vec<u8>,
    pub text: String,
}

/// Result of assembling source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledProgram {
    /// Load address from the first `ORG` directive (default `$0100`).
    pub origin: u16,
    pub bytes: Vec<u8>,
}

/// Assembler error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for AsmError {}

/// Disassemble machine code starting at `start_pc`.
///
/// Uses linear sweep: instruction length comes from decoded bytes, branches are
/// not followed. This avoids infinite loops on backward branches (e.g. `BRA *`).
pub fn disassemble(data: &[u8], start_pc: u16) -> Vec<DisassembledInsn> {
    disassemble_with_variant(data, start_pc, CpuVariant::Mc6809)
}

pub fn disassemble_with_variant(
    data: &[u8],
    start_pc: u16,
    variant: CpuVariant,
) -> Vec<DisassembledInsn> {
    if data.is_empty() {
        return Vec::new();
    }

    let original = data.to_vec();
    let mut mem = Memory::new();
    let _ = mem.load_binary(start_pc, &original);
    let mut pc = start_pc;
    let end = start_pc.wrapping_add(data.len() as u16);
    let mut out = Vec::with_capacity(data.len().min(128));
    let max_insns = data.len().max(1);

    while pc < end && out.len() < max_insns {
        let mut cpu = Cpu::new();
        cpu.variant = variant;
        if variant == CpuVariant::Hd6309 {
            cpu.mode_reg = 0x01;
        }
        cpu.pc = pc;
        let step = cpu.step(&mut mem);

        if step.bytes.is_empty() {
            break;
        }

        let text = if step.operands.is_empty() {
            step.mnemonic.clone()
        } else {
            format!("{} {}", step.mnemonic, step.operands)
        };

        out.push(DisassembledInsn {
            address: pc,
            bytes: step.bytes.clone(),
            text,
        });

        let advance = step.bytes.len() as u16;
        if advance == 0 {
            break;
        }
        let next_pc = pc.wrapping_add(advance);
        if next_pc <= pc || next_pc > end {
            break;
        }
        pc = next_pc;
    }

    out
}

/// Parsed source line: optional label + statement text.
struct ParsedLine {
    label: Option<String>,
    statement: String,
}

fn normalize_token(token: &str) -> String {
    token.trim_end_matches(':').to_uppercase()
}

fn is_statement_keyword(token: &str) -> bool {
    matches!(
        normalize_token(token).as_str(),
        "ORG" | "FCB" | "FDB" | "RMB" | "END" | "EQU" | "SET"
            | "NOP" | "SYNC" | "SWI" | "RTS" | "RTI" | "ABX" | "MUL" | "SEX" | "SEXW"
            | "CLRA" | "CLRB" | "LDA" | "LDB" | "LDX" | "LDY" | "LDU" | "LDD" | "LDS"
            | "ADDA" | "ADDB" | "SUBA" | "SUBB" | "CMPA" | "CMPB" | "ORA" | "ORB"
            | "ANDA" | "ANDB" | "EORA" | "EORB" | "ADDD" | "SUBD" | "STA" | "STB"
            | "STD" | "STU" | "STX" | "STY" | "CMPX" | "CMPY" | "CMPD" | "CMPU" | "CMPS"
            | "BRA" | "BRN" | "BNE" | "BEQ" | "BCC" | "BCS" | "BPL" | "BMI"
            | "BVC" | "BVS" | "BGE" | "BLT" | "BGT" | "BLE" | "BSR" | "LBRA" | "LBRN"
            | "LBHI" | "LBLS" | "LBCC" | "LBCS" | "LBNE" | "LBEQ" | "LBVC" | "LBVS"
            | "LBPL" | "LBMI" | "LBGE" | "LBLT" | "LBGT" | "LBLE" | "LBSR"
            | "JMP" | "JSR"             | "DEX" | "INX" | "DEY" | "INY" | "ORCC" | "ANDCC" | "TFR" | "EXG"
            | "PSHS" | "PULS" | "PSHU" | "PULU" | "PSHSW" | "PULSW" | "PSHUW" | "PULUW"
            | "LEAX" | "LEAY" | "LEAS" | "LEAU" | "CWAI"
            | "AIM" | "OIM" | "EIM" | "TIM"
            | "MULD" | "ADDW" | "SUBW" | "CMPW" | "LDW" | "STW" | "LDQ" | "STQ" | "LDMD" | "BITMD"
            | "INCW" | "DECW" | "CLRW" | "TSTW" | "DIVD" | "DIVQ"
            | "TFM" | "TFM+" | "TFM-" | "TFM+R" | "TFM+W"
            | "ADDR" | "ADCR" | "SUBR" | "SBCR" | "ANDR" | "ORR" | "EORR" | "CMPR"
            | "BAND" | "BIAND" | "BOR" | "BIOR" | "BEOR" | "BIEOR" | "LDBT" | "STBT"
    )
}

/// Split `label LDA #$42`, `label: LDA #$42`, or standalone `label:`.
fn parse_source_line(line: &str) -> ParsedLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedLine {
            label: None,
            statement: String::new(),
        };
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    // Standalone label line: "start:" or "start"
    if parts.len() == 1 {
        let token = parts[0];
        if token.ends_with(':') || !is_statement_keyword(token) {
            return ParsedLine {
                label: Some(normalize_token(token)),
                statement: String::new(),
            };
        }
    }

    // Inline label: "start LDA #$42" or "start: LDA #$42"
    if parts.len() >= 2 && is_statement_keyword(parts[1]) {
        let label = normalize_token(parts[0]);
        let rest = trimmed[parts[0].len()..]
            .trim_start()
            .trim_start_matches(':')
            .trim_start()
            .to_string();
        return ParsedLine {
            label: Some(label),
            statement: rest,
        };
    }

    ParsedLine {
        label: None,
        statement: trimmed.to_string(),
    }
}

fn advance_pc_for_statement(
    statement: &str,
    line_no: usize,
    scan_pc: &mut u32,
    origin: &mut u32,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<(), AsmError> {
    if statement.is_empty() {
        return Ok(());
    }

    let upper = statement.to_uppercase();
    if let Some(size) = instruction_size(statement) {
        *scan_pc += size;
    } else if upper.starts_with("ORG ") {
        let addr = parse_number(statement[4..].trim(), line_no)?;
        *scan_pc = addr;
        *origin = addr;
    } else if upper.starts_with("FCB") {
        let vals = parse_data_list(&statement[3..], line_no, false, labels)?;
        *scan_pc += vals.len() as u32;
    } else if upper.starts_with("FDB") {
        let vals = parse_data_list(&statement[3..], line_no, true, labels)?;
        *scan_pc += vals.len() as u32 * 2;
    } else if upper.starts_with("RMB ") {
        let count = parse_expression(statement[4..].trim(), line_no, labels)?;
        *scan_pc += count;
    } else if upper.starts_with("EQU ") || upper.starts_with("SET ") {
        // Label assignment only; does not advance PC.
    } else if upper == "END" {
        return Ok(());
    } else {
        return Err(AsmError {
            line: line_no,
            message: format!("unknown instruction for label pass: {statement}"),
        });
    }
    Ok(())
}

/// Assemble source text into machine code.
///
/// Supports common directives and instructions:
/// - `ORG $addr`
/// - `FCB $01,2,3` / `FDB $1234`
/// - Labels: `start:` or inline `start LDA #$42`
/// - `NOP`, `LDA #$xx`, `LDA <$xx`, `LDB #n`, `LDX #$xxxx`, `BRA *`, `BSR label`, `END`
pub fn assemble(source: &str) -> Result<AssembledProgram, AsmError> {
    let mut origin: u32 = 0x0100;
    let mut pc: u32 = origin;
    let mut output: Vec<(u32, u8)> = Vec::new();
    let mut labels: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    let lines: Vec<(usize, String)> = source
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.split(';').next().unwrap_or("").trim().to_string()))
        .filter(|(_, l)| !l.is_empty())
        .collect();

    // First pass: record labels.
    let mut scan_pc = origin;
    for (line_no, line) in &lines {
        let parsed = parse_source_line(line);
        let upper = parsed.statement.to_uppercase();
        if let Some(name) = parsed.label {
            if upper.starts_with("EQU ") || upper.starts_with("SET ") {
                let value = parse_expression(parsed.statement[4..].trim(), *line_no, &labels)?;
                labels.insert(name, value);
            } else {
                labels.insert(name, scan_pc);
            }
        }
        if upper == "END" {
            break;
        }
        advance_pc_for_statement(&parsed.statement, *line_no, &mut scan_pc, &mut origin, &labels)?;
    }

    // Second pass: emit bytes.
    for (line_no, line) in lines {
        let parsed = parse_source_line(&line);
        if parsed.statement.is_empty() {
            continue;
        }

        let upper = parsed.statement.to_uppercase();
        let parts: Vec<&str> = upper.split_whitespace().collect();
        if upper.starts_with("ORG ") {
            pc = parse_number(parsed.statement[4..].trim(), line_no)?;
            continue;
        }
        if upper == "END" {
            break;
        }
        if upper.starts_with("FCB") {
            for value in parse_data_list(&parsed.statement[3..], line_no, false, &labels)? {
                emit(&mut output, pc, value as u8);
                pc += 1;
            }
            continue;
        }
        if upper.starts_with("FDB") {
            for value in parse_data_list(&parsed.statement[3..], line_no, true, &labels)? {
                emit(&mut output, pc, (value >> 8) as u8);
                pc += 1;
                emit(&mut output, pc, value as u8);
                pc += 1;
            }
            continue;
        }
        if upper.starts_with("RMB ") {
            let count = parse_expression(parsed.statement[4..].trim(), line_no, &labels)?;
            for _ in 0..count {
                emit(&mut output, pc, 0);
                pc += 1;
            }
            continue;
        }
        if upper.starts_with("EQU ") || upper.starts_with("SET ") {
            continue;
        }

        let bytes = encode_instruction(&parsed.statement, pc, line_no, &labels, &parts)?;
        for b in bytes {
            emit(&mut output, pc, b);
            pc += 1;
        }
    }

    if output.is_empty() {
        return Ok(AssembledProgram {
            origin: origin as u16,
            bytes: Vec::new(),
        });
    }

    let min_addr = output.iter().map(|(a, _)| *a).min().unwrap_or(origin);
    let max_addr = output.iter().map(|(a, _)| *a).max().unwrap_or(min_addr);
    let mut bin = vec![0u8; (max_addr - min_addr + 1) as usize];
    for (addr, byte) in &output {
        let idx = (*addr - min_addr) as usize;
        if idx < bin.len() {
            bin[idx] = *byte;
        }
    }

    Ok(AssembledProgram {
        origin: min_addr as u16,
        bytes: bin,
    })
}

fn emit(out: &mut Vec<(u32, u8)>, addr: u32, byte: u8) {
    if let Some(existing) = out.iter_mut().find(|(a, _)| *a == addr) {
        existing.1 = byte;
    } else {
        out.push((addr, byte));
    }
}

fn instruction_size(line: &str) -> Option<u32> {
    let upper = line.to_uppercase();
    let parts: Vec<&str> = upper.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    match parts[0] {
        "NOP" | "CLRA" | "CLRB" | "SEX" | "SEXW" | "RTS" | "RTI" | "ABX" | "MUL" | "SYNC" | "SWI" => {
            Some(1)
        }
        "INX" | "DEX" | "INY" | "DEY" => Some(2), // LEAX/LEAY aliases (opcode + postbyte)
        "PSHS" | "PULS" | "PSHU" | "PULU" => Some(2),
        "LEAX" | "LEAY" | "LEAS" | "LEAU" => {
            let operand = parts.get(1).copied();
            if let Some(op) = operand {
                if is_indexed_operand(op) {
                    Some(2 + index_extra_size(op))
                } else {
                    operand_size(operand)
                }
            } else {
                None
            }
        }
        "MULD" => match parts.get(1).copied() {
            Some(op) if op.starts_with('#') => Some(4),
            Some(op) if op.starts_with('<') => Some(3),
            Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
            Some(op) if op.starts_with('>') || op.starts_with('$') => Some(4),
            None => Some(2),
            _ => None,
        },
        "PSHSW" | "PULSW" | "PSHUW" | "PULUW" => Some(2),
        "INCW" | "DECW" | "CLRW" | "TSTW" => Some(2),
        "AIM" | "OIM" | "EIM" | "TIM" => Some(4),
        "ADDW" | "SUBW" | "CMPW" | "LDW" => match parts.get(1).copied() {
            Some(op) if op.starts_with('#') => Some(4),
            Some(op) if op.starts_with('<') => Some(3),
            Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
            Some(op) if op.starts_with('>') || op.starts_with('$') => Some(4),
            _ => None,
        },
        "LDQ" => match parts.get(1).copied() {
            Some(op) if op.starts_with('#') => Some(5),
            Some(op) if op.starts_with('<') => Some(3),
            Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
            Some(op) if op.starts_with('>') || op.starts_with('$') => Some(4),
            _ => None,
        },
        "STW" | "STQ" => match parts.get(1).copied() {
            Some(op) if op.starts_with('<') => Some(3),
            Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
            Some(op) if op.starts_with('>') || op.starts_with('$') => Some(4),
            _ => None,
        },
        "DIVD" | "DIVQ" => match parts.get(1).copied() {
            Some(op) if op.starts_with('#') => Some(4),
            Some(op) if op.starts_with('<') => Some(3),
            Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
            _ => Some(4),
        },
        "TFM" | "TFM+" | "TFM-" | "TFM+R" | "TFM+W" => Some(3),
        "LDMD" | "BITMD" => Some(3),
        "ADDR" | "ADCR" | "SUBR" | "SBCR" | "ANDR" | "ORR" | "EORR" | "CMPR" => Some(3),
        "BAND" | "BIAND" | "BOR" | "BIOR" | "BEOR" | "BIEOR" | "LDBT" | "STBT" => Some(4),
        "ORCC" | "ANDCC" | "CWAI" | "TFR" | "EXG" => Some(2),
        "LDA" | "LDB" | "ADDA" | "ADDB" | "SUBA" | "SUBB" | "ANDA" | "ANDB" | "ORA" | "ORB" | "EORA" | "EORB" | "CMPA" | "CMPB" | "BITA" | "BITB" | "ADCA" | "ADCB" | "SBCA" | "SBCB" | "STA" | "STB" => {
            operand_size(parts.get(1).copied())
        }
        "LDX" | "LDY" | "LDU" | "LDD" | "LDS" | "CMPX" | "CMPY" | "CMPD" | "ADDD" | "SUBD" | "STD" | "STU" | "STX" | "STY" => {
            match parts.get(1).copied() {
                Some(op) if op.starts_with('#') => {
                    let page2_imm = matches!(parts[0], "LDY" | "LDS");
                    Some(if page2_imm { 4 } else { 3 })
                }
                Some(op) if op.starts_with('<') => Some(if parts[0] == "LDY" || parts[0] == "STY" { 3 } else { 2 }),
                Some(op) if op.starts_with('>') => Some(if parts[0] == "LDY" || parts[0] == "STY" { 4 } else { 3 }),
                Some(op) if is_indexed_operand(op) => Some(indexed_instruction_size(parts[0], op)),
                Some(op) if op.starts_with('$') => {
                    let size = dollar_address_size(op)?;
                    Some(if parts[0] == "LDY" || parts[0] == "STY" {
                        size + 1
                    } else {
                        size
                    })
                }
                _ => None,
            }
        }
        "BRA" | "BRN" | "BNE" | "BEQ" | "BCC" | "BCS" | "BPL" | "BMI" | "BVC" | "BVS" | "BSR" => Some(2),
        "LBRA" => Some(3),
        "LBRN" | "LBHI" | "LBLS" | "LBCC" | "LBCS" | "LBNE" | "LBEQ" | "LBVC" | "LBVS"
            | "LBPL" | "LBMI" | "LBGE" | "LBLT" | "LBGT" | "LBLE" => Some(4),
        "LBSR" => Some(3),
        "JMP" | "JSR" => {
            let op = parts.get(1).copied();
            match op {
                Some(o) if is_indexed_operand(o) => Some(2 + index_extra_size(o)),
                _ => operand_size(op),
            }
        }
        _ => None,
    }
}

fn operand_size(operand: Option<&str>) -> Option<u32> {
    match operand {
        Some(op) if op.starts_with('#') => Some(2),
        Some(op) if op.starts_with('<') => Some(2),
        Some(op) if op.starts_with('>') => Some(3),
        Some(op) if is_indexed_operand(op) => Some(2 + index_extra_size(op)),
        Some(op) if op.starts_with('$') => dollar_address_size(op),
        _ => None,
    }
}

fn dollar_address_size(operand: &str) -> Option<u32> {
    let value = parse_number(operand, 0).ok()?;
    Some(if value <= 0xFF { 2 } else { 3 })
}

fn encode_dollar_address(
    dir_opcode: u8,
    ext_opcode: u8,
    operand: &str,
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let value = parse_number(operand, line_no)?;
    if value <= 0xFF {
        Ok(vec![dir_opcode, value as u8])
    } else {
        Ok(vec![ext_opcode, (value >> 8) as u8, value as u8])
    }
}

fn is_indexed_operand(operand: &str) -> bool {
    operand.starts_with(',') || operand.contains(',') || operand.starts_with('[')
}

fn index_extra_size(operand: &str) -> u32 {
    let op = operand.trim().to_uppercase();
    // Strip indirect brackets
    let inner = if op.starts_with('[') && op.ends_with(']') {
        &op[1..op.len() - 1]
    } else {
        op.as_str()
    };

    // ,R or ,R+ or ,R++ or ,-R or ,--R — no extra bytes
    if let Some(reg_part) = inner.strip_prefix(',') {
        let reg_part = reg_part.trim();
        // Auto-inc/dec: no extra bytes
        if reg_part.ends_with('+') || reg_part.ends_with("++")
            || reg_part.starts_with('-') || reg_part.starts_with("--")
        {
            return 0;
        }
        return 0; // ,R = zero offset
    }

    let Some((off, _)) = inner.split_once(',') else {
        return 0;
    };
    let off = off.trim();
    if off.is_empty() {
        return 0;
    }
    // Accumulator offsets: A,R B,R D,R — no extra bytes
    if off == "A" || off == "B" || off == "D" {
        return 0;
    }
    if let Ok(value) = parse_signed_number(off, 0) {
        // 5-bit constant (-16..+15) — no extra bytes
        if (-16..=15).contains(&value) {
            return 0;
        }
        // 8-bit signed offset — 1 extra byte
        if (-128..=127).contains(&value) {
            return 1;
        }
        // 16-bit signed offset — 2 extra bytes
        if (-32768..=32767).contains(&value) {
            return 2;
        }
    }
    0
}

fn indexed_instruction_size(mnemonic: &str, operand: &str) -> u32 {
    let base = if mnemonic == "LDY" || mnemonic == "STY" { 3 } else { 2 };
    base + index_extra_size(operand)
}

fn parse_expression(
    text: &str,
    line: usize,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<u32, AsmError> {
    let t = text.trim();
    for (idx, ch) in t.char_indices() {
        if ch == '+' || ch == '-' {
            let left = t[..idx].trim();
            let right = t[idx + 1..].trim();
            if !left.is_empty() && !right.is_empty() {
                let left_val = parse_expression_atom(left, line, labels)?;
                let right_val = parse_number(right, line)?;
                return Ok(if ch == '+' {
                    left_val.wrapping_add(right_val)
                } else {
                    left_val.wrapping_sub(right_val)
                });
            }
        }
    }
    parse_expression_atom(t, line, labels)
}

fn parse_expression_atom(
    text: &str,
    line: usize,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<u32, AsmError> {
    let t = text.trim();
    if let Ok(value) = parse_number(t, line) {
        return Ok(value);
    }
    let upper = normalize_token(t);
    labels.get(&upper).copied().ok_or_else(|| AsmError {
        line,
        message: format!("undefined label in expression: {text}"),
    })
}

fn parse_signed_number(text: &str, line: usize) -> Result<i32, AsmError> {
    let t = text.trim().trim_matches('*');
    if let Some(hex) = t.strip_prefix("-$") {
        let value = i32::from_str_radix(hex, 16).map_err(|_| AsmError {
            line,
            message: format!("invalid hex number: {text}"),
        })?;
        return Ok(-value);
    }
    if let Some(hex) = t.strip_prefix('$') {
        let value = i32::from_str_radix(hex, 16).map_err(|_| AsmError {
            line,
            message: format!("invalid hex number: {text}"),
        })?;
        return Ok(value);
    }
    t.parse::<i32>().map_err(|_| AsmError {
        line,
        message: format!("invalid number: {text}"),
    })
}

fn parse_number(text: &str, line: usize) -> Result<u32, AsmError> {
    let t = text.trim().trim_matches('*');
    if let Some(hex) = t.strip_prefix('$') {
        u32::from_str_radix(hex, 16).map_err(|_| AsmError {
            line,
            message: format!("invalid hex number: {text}"),
        })
    } else if let Some(bin) = t.strip_prefix('%') {
        u32::from_str_radix(bin, 2).map_err(|_| AsmError {
            line,
            message: format!("invalid binary number: {text}"),
        })
    } else if let Some(oct) = t.strip_prefix('@') {
        u32::from_str_radix(oct, 8).map_err(|_| AsmError {
            line,
            message: format!("invalid octal number: {text}"),
        })
    } else {
        t.parse::<u32>().map_err(|_| AsmError {
            line,
            message: format!("invalid number: {text}"),
        })
    }
}

fn parse_data_list(
    text: &str,
    line: usize,
    words: bool,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<Vec<u32>, AsmError> {
    let mut values = Vec::new();
    for part in text.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let value = parse_expression(p, line, labels)?;
        if words {
            if value > 0xFFFF {
                return Err(AsmError {
                    line,
                    message: format!("FDB value out of range: {p}"),
                });
            }
        } else if value > 0xFF {
            return Err(AsmError {
                line,
                message: format!("FCB value out of range: {p}"),
            });
        }
        values.push(value);
    }
    Ok(values)
}

fn encode_instruction(
    line: &str,
    pc: u32,
    line_no: usize,
    labels: &std::collections::HashMap<String, u32>,
    parts: &[&str],
) -> Result<Vec<u8>, AsmError> {
    if parts.is_empty() {
        return Err(AsmError {
            line: line_no,
            message: "empty instruction".into(),
        });
    }

    match parts[0] {
        "NOP" => Ok(vec![0x12]),
        "SYNC" => Ok(vec![0x13]),
        "SWI" => Ok(vec![0x3F]),
        "RTS" => Ok(vec![0x39]),
        "RTI" => Ok(vec![0x3B]),
        "ABX" => Ok(vec![0x3A]),
        "MUL" => Ok(vec![0x3D]),
        "SEX" => Ok(vec![0x1D]),
        "INX" => Ok(vec![0x30, 0x01]),  // LEAX 1,X (5-bit constant +1, X=00)
        "DEX" => Ok(vec![0x30, 0x1F]),  // LEAX -1,X (5-bit constant -1, X=00)
        "INY" => Ok(vec![0x31, 0x21]),  // LEAY 1,Y (5-bit constant +1, Y=01)
        "DEY" => Ok(vec![0x31, 0x3F]),  // LEAY -1,Y (5-bit constant -1, Y=01)
        "ORCC" => encode_cc_imm(0x1A, parts, line_no),
        "ANDCC" => encode_cc_imm(0x1C, parts, line_no),
        "CWAI" => encode_cc_imm(0x3C, parts, line_no),
        "CLRA" => Ok(vec![0x4F]),
        "CLRB" => Ok(vec![0x5F]),
        "LDA" => encode_lda(line, parts, pc, line_no),
        "LDB" => encode_ldb(parts, line_no),
        "LDX" => encode_ldx(parts, line_no),
        "LDY" => encode_ld16_page2(0x8E, parts, line_no),
        "LDU" => encode_ld16(0xCE, parts, line_no),
        "LDD" => encode_ld16(0xCC, parts, line_no),
        "LDS" => encode_ld16_page2(0xCE, parts, line_no),
        "ADDA" => encode_alu8(0x8B, parts, line_no),
        "ADDB" => encode_alu8(0xCB, parts, line_no),
        "SUBA" => encode_alu8(0x80, parts, line_no),
        "SUBB" => encode_alu8(0xC0, parts, line_no),
        "CMPA" => encode_alu8(0x81, parts, line_no),
        "CMPB" => encode_alu8(0xC1, parts, line_no),
        "ORA" => encode_alu8(0x8A, parts, line_no),
        "ORB" => encode_alu8(0xCA, parts, line_no),
        "ANDA" => encode_alu8(0x84, parts, line_no),
        "ANDB" => encode_alu8(0xC4, parts, line_no),
        "EORA" => encode_alu8(0x88, parts, line_no),
        "EORB" => encode_alu8(0xC8, parts, line_no),
        "ADDD" => encode_addd(parts, line_no),
        "SUBD" => encode_subd(parts, line_no),
        "STA" => encode_sta_fixed(parts, line_no),
        "STB" => encode_stb(parts, line_no),
        "STD" => encode_st16(0xCC, parts, line_no),
        "STU" => encode_st16(0xCE, parts, line_no),
        "STX" => encode_st16(0x8E, parts, line_no),
        "STY" => encode_st16_page2(0x8E, parts, line_no),
        "CMPX" => encode_ld16(0x8C, parts, line_no),
        "BRA" => encode_branch(0x20, parts, pc, line_no, labels),
        "BRN" => encode_branch(0x21, parts, pc, line_no, labels),
        "BNE" => encode_branch(0x26, parts, pc, line_no, labels),
        "BEQ" => encode_branch(0x27, parts, pc, line_no, labels),
        "BCC" => encode_branch(0x24, parts, pc, line_no, labels),
        "BCS" => encode_branch(0x25, parts, pc, line_no, labels),
        "BPL" => encode_branch(0x2A, parts, pc, line_no, labels),
        "BMI" => encode_branch(0x2B, parts, pc, line_no, labels),
        "BSR" => encode_branch(0x8D, parts, pc, line_no, labels),
        "LBRA" => encode_long_branch(0x16, parts, pc, line_no, labels),
        "LBRN" => encode_long_branch_page2(0x21, parts, pc, line_no, labels),
        "LBHI" => encode_long_branch_page2(0x22, parts, pc, line_no, labels),
        "LBLS" => encode_long_branch_page2(0x23, parts, pc, line_no, labels),
        "LBCC" => encode_long_branch_page2(0x24, parts, pc, line_no, labels),
        "LBCS" => encode_long_branch_page2(0x25, parts, pc, line_no, labels),
        "LBNE" => encode_long_branch_page2(0x26, parts, pc, line_no, labels),
        "LBEQ" => encode_long_branch_page2(0x27, parts, pc, line_no, labels),
        "LBVC" => encode_long_branch_page2(0x28, parts, pc, line_no, labels),
        "LBVS" => encode_long_branch_page2(0x29, parts, pc, line_no, labels),
        "LBPL" => encode_long_branch_page2(0x2A, parts, pc, line_no, labels),
        "LBMI" => encode_long_branch_page2(0x2B, parts, pc, line_no, labels),
        "LBGE" => encode_long_branch_page2(0x2C, parts, pc, line_no, labels),
        "LBLT" => encode_long_branch_page2(0x2D, parts, pc, line_no, labels),
        "LBGT" => encode_long_branch_page2(0x2E, parts, pc, line_no, labels),
        "LBLE" => encode_long_branch_page2(0x2F, parts, pc, line_no, labels),
        "LBSR" => encode_long_branch(0x17, parts, pc, line_no, labels),
        "JMP" => encode_jump(0x0E, 0x6E, 0x7E, parts, line_no),
        "JSR" => encode_jump(0x9D, 0xAD, 0xBD, parts, line_no),
        "PSHS" => encode_psh_pul(0x34, parts, line_no),
        "PULS" => encode_psh_pul(0x35, parts, line_no),
        "PSHU" => encode_psh_pul(0x36, parts, line_no),
        "PULU" => encode_psh_pul(0x37, parts, line_no),
        "PSHSW" => Ok(vec![0x10, 0x38]),
        "PULSW" => Ok(vec![0x10, 0x39]),
        "PSHUW" => Ok(vec![0x10, 0x3A]),
        "PULUW" => Ok(vec![0x10, 0x3B]),
        "LEAX" => encode_lea(0x30, parts, pc, line_no),
        "LEAY" => encode_lea(0x31, parts, pc, line_no),
        "LEAS" => encode_lea(0x32, parts, pc, line_no),
        "LEAU" => encode_lea(0x33, parts, pc, line_no),
        "MULD" => {
            if parts.len() < 2 {
                Ok(vec![0x10, 0x3E])
            } else {
                encode_muld_page3(0x8F, 0x9F, 0xAF, 0xBF, parts, line_no)
            }
        }
        "AIM" => encode_logic_imm(0x02, parts, line_no),
        "OIM" => encode_logic_imm(0x01, parts, line_no),
        "EIM" => encode_logic_imm(0x05, parts, line_no),
        "TIM" => encode_logic_imm(0x0B, parts, line_no),
        "ADDW" => encode_alu16_page2(0x8B, 0x9B, 0xAB, 0xBB, parts, line_no),
        "SUBW" => encode_alu16_page2(0x80, 0x90, 0xA0, 0xB0, parts, line_no),
        "CMPW" => encode_alu16_page2(0x81, 0x91, 0xA1, 0xB1, parts, line_no),
        "LDW" => encode_alu16_page2(0x86, 0x96, 0xA6, 0xB6, parts, line_no),
        "STW" => encode_stw_page2(parts, line_no),
        "LDQ" => encode_ldq(parts, line_no),
        "STQ" => encode_stq(parts, line_no),
        "SEXW" => Ok(vec![0x14]),
        "LDMD" => encode_ldmd(parts, line_no),
        "INCW" => Ok(vec![0x10, 0x4C]),
        "DECW" => Ok(vec![0x10, 0x4A]),
        "CLRW" => Ok(vec![0x10, 0x4F]),
        "TSTW" => Ok(vec![0x10, 0x4D]),
        "DIVD" => encode_div_page3(0x8D, 0x9D, 0xAD, 0xBD, parts, line_no),
        "DIVQ" => encode_div_page3(0x8E, 0x9E, 0xAE, 0xBE, parts, line_no),
        "BITMD" => encode_bitmd(parts, line_no),
        "TFM" => encode_tfm(0x38, parts, line_no),
        "TFM+" => encode_tfm(0x38, parts, line_no),
        "TFM-" => encode_tfm(0x39, parts, line_no),
        "TFM+R" => encode_tfm(0x3A, parts, line_no),
        "TFM+W" => encode_tfm(0x3B, parts, line_no),
        "ADDR" => encode_inter_reg(0x30, parts, line_no),
        "ADCR" => encode_inter_reg(0x31, parts, line_no),
        "SUBR" => encode_inter_reg(0x32, parts, line_no),
        "SBCR" => encode_inter_reg(0x33, parts, line_no),
        "ANDR" => encode_inter_reg(0x34, parts, line_no),
        "ORR" => encode_inter_reg(0x35, parts, line_no),
        "EORR" => encode_inter_reg(0x36, parts, line_no),
        "CMPR" => encode_inter_reg(0x37, parts, line_no),
        "BAND" => encode_bit_transfer(0x30, parts, line_no),
        "BIAND" => encode_bit_transfer(0x31, parts, line_no),
        "BOR" => encode_bit_transfer(0x32, parts, line_no),
        "BIOR" => encode_bit_transfer(0x33, parts, line_no),
        "BEOR" => encode_bit_transfer(0x34, parts, line_no),
        "BIEOR" => encode_bit_transfer(0x35, parts, line_no),
        "LDBT" => encode_bit_transfer(0x36, parts, line_no),
        "STBT" => encode_bit_transfer(0x37, parts, line_no),
        "TFR" => encode_tfr_exg(0x1F, parts, line_no),
        "EXG" => encode_tfr_exg(0x1E, parts, line_no),
        _ => Err(AsmError {
            line: line_no,
            message: format!("unsupported instruction: {}", parts[0]),
        }),
    }
}

fn parse_operand<'a>(parts: &[&'a str], line: usize) -> Result<&'a str, AsmError> {
    if parts.len() < 2 {
        return Err(AsmError {
            line,
            message: "missing operand".into(),
        });
    }
    Ok(parts[1])
}

struct IndexOperand {
    postbyte: u8,
    extra_bytes: Vec<u8>,
}

fn index_register_bits(reg: &str, line: usize) -> Result<u8, AsmError> {
    match reg {
        "X" => Ok(0x00),
        "Y" => Ok(0x20),
        "U" => Ok(0x40),
        "S" => Ok(0x60),
        "PCR" => Ok(0x00), // PCR is encoded in mode bits, not register field; reg bits unused for PCR
        _ => Err(AsmError {
            line,
            message: format!("unsupported index register: {reg}"),
        }),
    }
}

fn parse_index_operand(operand: &str, line: usize) -> Result<IndexOperand, AsmError> {
    let op = operand.trim().to_uppercase();
    let indirect = op.starts_with('[') && op.ends_with(']');
    let inner = if indirect {
        &op[1..op.len() - 1]
    } else {
        op.as_str()
    };
    let indirect_bit = if indirect { 0x10 } else { 0x00 };

    // Handle ,R forms: ,R  ,R+  ,R++  ,-R  ,--R
    if let Some(rest) = inner.strip_prefix(',') {
        let rest = rest.trim();
        // ,--R (auto-decrement 2)
        if let Some(reg) = rest.strip_prefix("--") {
            let reg_bits = index_register_bits(reg.trim(), line)?;
            return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x03, extra_bytes: Vec::new() });
        }
        // ,-R (auto-decrement 1)
        if let Some(reg) = rest.strip_prefix('-') {
            let reg_bits = index_register_bits(reg.trim(), line)?;
            return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x02, extra_bytes: Vec::new() });
        }
        // ,R++ (auto-increment 2)
        if let Some(reg) = rest.strip_suffix("++") {
            let reg_bits = index_register_bits(reg.trim(), line)?;
            return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x01, extra_bytes: Vec::new() });
        }
        // ,R+ (auto-increment 1)
        if let Some(reg) = rest.strip_suffix('+') {
            let reg_bits = index_register_bits(reg.trim(), line)?;
            return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit, extra_bytes: Vec::new() });
        }
        // ,R (zero offset) — but NOT ,PCR (needs explicit offset)
        if rest == "PCR" {
            return Err(AsmError {
                line,
                message: "PCR requires an offset: use 0,PCR".into(),
            });
        }
        let reg_bits = index_register_bits(rest, line)?;
        return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x04, extra_bytes: Vec::new() });
    }

    // Parse offset,reg or acc,reg
    let Some((off_str, reg_str)) = inner.split_once(',') else {
        // [address] — indirect extended with no register
        if indirect {
            let addr = parse_number(inner.trim(), line)? as u16;
            return Ok(IndexOperand {
                postbyte: 0x90 | 0x0F, // indirect extended: 0x9F
                extra_bytes: vec![(addr >> 8) as u8, addr as u8],
            });
        }
        return Err(AsmError {
            line,
            message: format!("unsupported indexed operand: {operand}"),
        });
    };

    let reg_str = reg_str.trim();
    let off_str = off_str.trim();
    let is_pcr = reg_str == "PCR";
    let reg_bits = if is_pcr { 0x00 } else { index_register_bits(reg_str, line)? };

    // Accumulator offsets: A,R  B,R  D,R
    match off_str {
        "A" => return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x06, extra_bytes: Vec::new() }),
        "B" => return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x05, extra_bytes: Vec::new() }),
        "D" => return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | 0x0B, extra_bytes: Vec::new() }),
        _ => {}
    }

    let offset = parse_signed_number(off_str, line)?;

    // 5-bit constant offset (-16..+15) — no extra bytes, bit 7 = 0 (direct only, not PCR)
    if !is_pcr && !indirect && (-16..=15).contains(&offset) {
        let off5 = (offset as i8 as u8) & 0x1F;
        return Ok(IndexOperand { postbyte: reg_bits | off5, extra_bytes: Vec::new() });
    }

    // 8-bit signed offset: n8,R or n8,PCR — 1 extra byte
    if (-128..=127).contains(&offset) {
        let mode_bit = if is_pcr { 0x0C } else { 0x08 };
        return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | mode_bit, extra_bytes: vec![offset as i8 as u8] });
    }

    // 16-bit signed offset: n16,R or n16,PCR — 2 extra bytes
    if (-32768..=32767).contains(&offset) {
        let off = offset as i16;
        let mode_bit = if is_pcr { 0x0D } else { 0x09 };
        return Ok(IndexOperand { postbyte: 0x80 | reg_bits | indirect_bit | mode_bit, extra_bytes: vec![(off >> 8) as u8, off as u8] });
    }

    Err(AsmError { line, message: format!("indexed offset out of range: {operand}") })
}

fn encode_indexed(opcode: u8, operand: &str, line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_indexed_with_pc(opcode, operand, 0, line_no)
}

fn encode_indexed_with_pc(opcode: u8, operand: &str, pc: u32, line_no: usize) -> Result<Vec<u8>, AsmError> {
    let idx = parse_index_operand(operand, line_no)?;
    let mut bytes = vec![opcode, idx.postbyte];

    // For PCR modes, compute offset relative to PC after instruction
    let pbm = idx.postbyte & 0x8F;
    let is_pcr = pbm == 0x8C || pbm == 0x8D;
    if is_pcr && !idx.extra_bytes.is_empty() {
        let target = if idx.extra_bytes.len() == 1 {
            idx.extra_bytes[0] as i32
        } else {
            i16::from_be_bytes([idx.extra_bytes[0], idx.extra_bytes[1]]) as i32
        };
        // Try 8-bit first; if target fits in PCR8 delta, switch to 8-bit mode
        let insn_size_8 = 3u32; // opcode + postbyte + 1 byte
        let delta_8 = target as i64 - (pc + insn_size_8) as i64;
        if pbm == 0x8D && (-128..=127).contains(&delta_8) {
            // Switch from PCR16 to PCR8
            bytes[1] = (bytes[1] & !0x0F) | 0x0C; // change mode to 0x8C
            bytes.push(delta_8 as i8 as u8);
        } else if pbm == 0x8C {
            if !(-128..=127).contains(&delta_8) {
                return Err(AsmError { line: line_no, message: format!("PCR offset out of range: {delta_8}") });
            }
            bytes.push(delta_8 as i8 as u8);
        } else {
            // PCR16
            let insn_size_16 = 4u32;
            let delta_16 = target as i64 - (pc + insn_size_16) as i64;
            if !(-32768..=32767).contains(&delta_16) {
                return Err(AsmError { line: line_no, message: format!("PCR offset out of range: {delta_16}") });
            }
            let d = delta_16 as i16;
            bytes.push((d >> 8) as u8);
            bytes.push(d as u8);
        }
    } else {
        bytes.extend(idx.extra_bytes);
    }
    Ok(bytes)
}

fn encode_cc_imm(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    let rest = operand.strip_prefix('#').ok_or_else(|| AsmError {
        line: line_no,
        message: format!("unsupported CC operand: {operand}"),
    })?;
    let value = parse_number(rest, line_no)?;
    if value > 0xFF {
        return Err(AsmError {
            line: line_no,
            message: format!("CC immediate out of range: {operand}"),
        });
    }
    Ok(vec![opcode, value as u8])
}

fn encode_lda(line: &str, parts: &[&str], pc: u32, line_no: usize) -> Result<Vec<u8>, AsmError> {
    let op = parse_operand(parts, line_no)?;
    if let Some(rest) = op.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x86, value as u8])
    } else if let Some(rest) = op.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x96, value as u8])
    } else if let Some(rest) = op.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0xB6, (value >> 8) as u8, value as u8])
    } else if is_indexed_operand(op) {
        encode_indexed_with_pc(0xA6, op, pc, line_no)
    } else if op.starts_with('$') {
        encode_dollar_address(0x96, 0xB6, op, line_no)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported LDA operand: {line}"),
        })
    }
}

fn encode_ldb(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let op = parse_operand(parts, line_no)?;
    if let Some(rest) = op.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0xC6, value as u8])
    } else if let Some(rest) = op.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0xD6, value as u8])
    } else if let Some(rest) = op.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0xF6, (value >> 8) as u8, value as u8])
    } else if op.starts_with('$') {
        encode_dollar_address(0xD6, 0xF6, op, line_no)
    } else if is_indexed_operand(op) {
        encode_indexed(0xE6, op, line_no)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported LDB operand: {op}"),
        })
    }
}

fn encode_ldx(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_ld16(0x8E, parts, line_no)
}

fn encode_ld16(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode, (value >> 8) as u8, value as u8])
    } else if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode + 0x10, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode + 0x30, (value >> 8) as u8, value as u8])
    } else if operand.starts_with('$') {
        encode_dollar_address(opcode + 0x10, opcode + 0x30, operand, line_no)
    } else if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![opcode + 0x20, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        Ok(bytes)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported operand: {operand}"),
        })
    }
}

fn encode_ld16_page2(page_op: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x10, page_op, (value >> 8) as u8, value as u8])
    } else if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x10, page_op + 0x10, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x10, page_op + 0x30, (value >> 8) as u8, value as u8])
    } else if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![0x10, page_op + 0x20, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        Ok(bytes)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported operand: {operand}"),
        })
    }
}

fn encode_alu8(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode, value as u8])
    } else if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode + 0x10, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![opcode + 0x30, (value >> 8) as u8, value as u8])
    } else if operand.starts_with('$') {
        encode_dollar_address(opcode + 0x10, opcode + 0x30, operand, line_no)
    } else if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![opcode + 0x20, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        Ok(bytes)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported operand: {operand}"),
        })
    }
}

fn encode_addd(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_ld16(0xC3, parts, line_no)
}

fn encode_subd(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_ld16(0x83, parts, line_no)
}

fn encode_st16(load_base: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![load_base + 0x11, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![load_base + 0x31, (value >> 8) as u8, value as u8])
    } else if operand.starts_with('$') {
        encode_dollar_address(load_base + 0x11, load_base + 0x31, operand, line_no)
    } else if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![load_base + 0x21, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        Ok(bytes)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported store operand: {operand}"),
        })
    }
}

fn encode_st16_page2(page_load_op: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x10, page_load_op + 0x11, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![0x10, page_load_op + 0x31, (value >> 8) as u8, value as u8])
    } else if operand.starts_with('$') {
        let bytes = encode_dollar_address(page_load_op + 0x11, page_load_op + 0x31, operand, line_no)?;
        Ok(std::iter::once(0x10).chain(bytes).collect())
    } else if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![0x10, page_load_op + 0x21, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        Ok(bytes)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported store operand: {operand}"),
        })
    }
}

fn encode_stb(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_st16(0xC6, parts, line_no)
}

fn encode_sta_fixed(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_st16(0x86, parts, line_no)
}

fn encode_jump(
    dir_op: u8,
    idx_op: u8,
    ext_op: u8,
    parts: &[&str],
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![dir_op, value as u8])
    } else if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        Ok(vec![ext_op, (value >> 8) as u8, value as u8])
    } else if operand.starts_with('$') {
        encode_dollar_address(dir_op, ext_op, operand, line_no)
    } else if is_indexed_operand(operand) {
        encode_indexed(idx_op, operand, line_no)
    } else {
        Err(AsmError {
            line: line_no,
            message: format!("unsupported jump operand: {operand}"),
        })
    }
}

fn encode_long_branch(
    opcode: u8,
    parts: &[&str],
    pc: u32,
    line_no: usize,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<Vec<u8>, AsmError> {
    let target_text = if parts.len() > 1 {
        parts[1]
    } else {
        "*"
    };

    let target = if let Some(addr) = labels.get(target_text) {
        *addr
    } else {
        parse_number(target_text, line_no)?
    };

    let offset_pc = pc + 3;
    let delta = target as i64 - offset_pc as i64;
    if !(-32768..=32767).contains(&delta) {
        return Err(AsmError {
            line: line_no,
            message: format!("long branch out of range: delta={delta}"),
        });
    }
    let delta16 = delta as i16;
    Ok(vec![opcode, (delta16 >> 8) as u8, delta16 as u8])
}

fn encode_long_branch_page2(
    opcode: u8,
    parts: &[&str],
    pc: u32,
    line_no: usize,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<Vec<u8>, AsmError> {
    let target_text = if parts.len() > 1 {
        parts[1]
    } else {
        "*"
    };

    let target = if let Some(addr) = labels.get(target_text) {
        *addr
    } else {
        parse_number(target_text, line_no)?
    };

    let offset_pc = pc + 4;
    let delta = target as i64 - offset_pc as i64;
    if !(-32768..=32767).contains(&delta) {
        return Err(AsmError {
            line: line_no,
            message: format!("long branch out of range: delta={delta}"),
        });
    }
    let delta16 = delta as i16;
    Ok(vec![0x10, opcode, (delta16 >> 8) as u8, delta16 as u8])
}

fn stack_register_bit(reg: &str, line: usize) -> Result<u8, AsmError> {
    match reg {
        "CC" => Ok(0x01),
        "A" => Ok(0x02),
        "B" => Ok(0x04),
        "D" => Ok(0x06), // D = A|B
        "DP" => Ok(0x08),
        "X" => Ok(0x10),
        "Y" => Ok(0x20),
        "U" => Ok(0x40),
        "S" => Ok(0x40), // S has same bit as U (the "other" stack register)
        "PC" => Ok(0x80),
        _ => Err(AsmError {
            line,
            message: format!("unsupported stack register: {reg}"),
        }),
    }
}

fn parse_stack_postbyte(operand: &str, line: usize) -> Result<u8, AsmError> {
    let op = operand.trim().to_uppercase();
    if op.starts_with('$') {
        let value = parse_number(&op, line)?;
        if value > 0xFF {
            return Err(AsmError {
                line,
                message: format!("stack postbyte out of range: {operand}"),
            });
        }
        return Ok(value as u8);
    }

    let mut postbyte = 0u8;
    for part in op.split(',') {
        let reg = part.trim();
        if reg.is_empty() {
            continue;
        }
        postbyte |= stack_register_bit(reg, line)?;
    }
    if postbyte == 0 {
        return Err(AsmError {
            line,
            message: "empty stack register list".into(),
        });
    }
    Ok(postbyte)
}

fn encode_lea(opcode: u8, parts: &[&str], pc: u32, line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if !is_indexed_operand(operand) {
        return Err(AsmError {
            line: line_no,
            message: format!("{operand}: LEA requires indexed addressing"),
        });
    }
    encode_indexed_with_pc(opcode, operand, pc, line_no)
}

fn encode_logic_imm(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    if parts.len() < 3 {
        return Err(AsmError {
            line: line_no,
            message: "logic instruction requires address and immediate".into(),
        });
    }
    let addr = parse_number(parts[1], line_no)?;
    let imm_part = parts[2];
    let imm_rest = imm_part.strip_prefix('#').ok_or_else(|| AsmError {
        line: line_no,
        message: format!("expected immediate operand, got {imm_part}"),
    })?;
    let imm = parse_number(imm_rest, line_no)?;
    if addr > 0xFF || imm > 0xFF {
        return Err(AsmError {
            line: line_no,
            message: "direct logic operands must be 8-bit".into(),
        });
    }
    Ok(vec![0x10, opcode, addr as u8, imm as u8])
}

fn encode_alu16_page2(
    imm_op: u8,
    dir_op: u8,
    idx_op: u8,
    ext_op: u8,
    parts: &[&str],
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x10, imm_op, (value >> 8) as u8, value as u8]);
    }
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x10, dir_op, value as u8]);
    }
    if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![0x10, idx_op, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        return Ok(bytes);
    }
    if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x10, ext_op, (value >> 8) as u8, value as u8]);
    }
    let value = parse_number(operand, line_no)?;
    Ok(vec![0x10, ext_op, (value >> 8) as u8, value as u8])
}

fn encode_stw_page2(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_st16_page2(0x86, parts, line_no)
}

fn encode_stq_page2(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x10, 0xDD, value as u8]);
    }
    if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![0x10, 0xED, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        return Ok(bytes);
    }
    if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x10, 0xFD, (value >> 8) as u8, value as u8]);
    }
    if operand.starts_with('$') {
        let bytes = encode_dollar_address(0xDD, 0xFD, operand, line_no)?;
        return Ok(std::iter::once(0x10).chain(bytes).collect());
    }
    let value = parse_number(operand, line_no)?;
    Ok(vec![0x10, 0xFD, (value >> 8) as u8, value as u8])
}

fn encode_ldq(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![
            0xCD,
            ((value >> 24) & 0xFF) as u8,
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ]);
    }
    encode_alu16_page2(0x00, 0xDC, 0xEC, 0xFC, parts, line_no)
}

fn encode_stq(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    encode_stq_page2(parts, line_no)
}

fn tfm_register_code(reg: &str, line: usize) -> Result<u8, AsmError> {
    let name = reg.trim().trim_end_matches('+').trim_end_matches('-');
    match name {
        "D" => Ok(0),
        "X" => Ok(1),
        "Y" => Ok(2),
        "U" => Ok(3),
        "S" => Ok(4),
        "PC" => Ok(5),
        "W" => Ok(6),
        "V" => Ok(7),
        other => Err(AsmError {
            line,
            message: format!("unsupported TFM register: {other}"),
        }),
    }
}

fn encode_tfm(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    if parts.len() < 2 {
        return Err(AsmError {
            line: line_no,
            message: "TFM requires source and destination registers".into(),
        });
    }
    let pair = parts[1];
    let (src, dst) = pair
        .split_once(',')
        .ok_or_else(|| AsmError {
            line: line_no,
            message: format!("TFM operand must be src,dst: {pair}"),
        })?;
    let src_code = tfm_register_code(src, line_no)?;
    let dst_code = tfm_register_code(dst, line_no)?;
    Ok(vec![0x11, opcode, (src_code << 4) | dst_code])
}

fn encode_muld_page3(
    imm_op: u8,
    dir_op: u8,
    idx_op: u8,
    ext_op: u8,
    parts: &[&str],
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    encode_div_page3(imm_op, dir_op, idx_op, ext_op, parts, line_no)
}

fn inter_register_code(reg: &str, line: usize) -> Result<u8, AsmError> {
    match reg {
        "D" => Ok(0),
        "X" => Ok(1),
        "Y" => Ok(2),
        "U" => Ok(3),
        "S" => Ok(4),
        "PC" => Ok(5),
        "W" => Ok(6),
        "V" => Ok(7),
        "A" => Ok(8),
        "B" => Ok(9),
        "CC" => Ok(0xA),
        "DP" => Ok(0xB),
        "E" => Ok(0xE),
        "F" => Ok(0xF),
        _ => Err(AsmError {
            line,
            message: format!("unsupported inter-register operand: {reg}"),
        }),
    }
}

fn encode_tfr_exg(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    if parts.len() < 2 {
        return Err(AsmError {
            line: line_no,
            message: "TFR/EXG requires two registers".into(),
        });
    }
    let pair = parts[1];
    let (src, dst) = pair.split_once(',').ok_or_else(|| AsmError {
        line: line_no,
        message: format!("expected src,dst: {pair}"),
    })?;
    let src_code = inter_register_code(src.trim(), line_no)?;
    let dst_code = inter_register_code(dst.trim(), line_no)?;
    Ok(vec![opcode, (src_code << 4) | dst_code])
}

fn encode_inter_reg(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    if parts.len() < 2 {
        return Err(AsmError {
            line: line_no,
            message: "inter-register instruction requires two registers".into(),
        });
    }
    let pair = parts[1];
    let (src, dst) = pair.split_once(',').ok_or_else(|| AsmError {
        line: line_no,
        message: format!("expected src,dst: {pair}"),
    })?;
    let src_code = inter_register_code(src.trim(), line_no)?;
    let dst_code = inter_register_code(dst.trim(), line_no)?;
    let is_16 = matches!(src.trim(), "D" | "X" | "Y" | "U" | "S" | "PC" | "W" | "V");
    let postbyte = if is_16 {
        ((src_code & 0x07) << 4) | 0x08 | (dst_code & 0x07)
    } else {
        (src_code << 4) | dst_code
    };
    Ok(vec![0x10, opcode, postbyte])
}

fn encode_div_page3(
    imm_op: u8,
    dir_op: u8,
    idx_op: u8,
    ext_op: u8,
    parts: &[&str],
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    if let Some(rest) = operand.strip_prefix('#') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x11, imm_op, (value >> 8) as u8, value as u8]);
    }
    if let Some(rest) = operand.strip_prefix('<') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x11, dir_op, value as u8]);
    }
    if is_indexed_operand(operand) {
        let idx = parse_index_operand(operand, line_no)?;
        let mut bytes = vec![0x11, idx_op, idx.postbyte];
        bytes.extend(idx.extra_bytes);
        return Ok(bytes);
    }
    if let Some(rest) = operand.strip_prefix('>') {
        let value = parse_number(rest, line_no)?;
        return Ok(vec![0x11, ext_op, (value >> 8) as u8, value as u8]);
    }
    let value = parse_number(operand, line_no)?;
    Ok(vec![0x11, ext_op, (value >> 8) as u8, value as u8])
}

fn encode_bitmd(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    let rest = operand.strip_prefix('#').ok_or_else(|| AsmError {
        line: line_no,
        message: "BITMD requires immediate operand".into(),
    })?;
    let value = parse_number(rest, line_no)?;
    if value > 0xFF {
        return Err(AsmError {
            line: line_no,
            message: "BITMD value out of range".into(),
        });
    }
    Ok(vec![0x11, 0x3C, value as u8])
}

fn encode_bit_transfer(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    if parts.len() < 5 {
        return Err(AsmError {
            line: line_no,
            message: "bit transfer requires reg,srcbit,dstbit,address".into(),
        });
    }
    let reg_code = match parts[1] {
        "A" => 0u8,
        "B" => 1,
        "CC" => 2,
        other => {
            return Err(AsmError {
                line: line_no,
                message: format!("unsupported bit register: {other}"),
            });
        }
    };
    let src_bit = parse_number(parts[2], line_no)?;
    let dst_bit = parse_number(parts[3], line_no)?;
    if src_bit > 7 || dst_bit > 7 {
        return Err(AsmError {
            line: line_no,
            message: "bit index must be 0..7".into(),
        });
    }
    let addr = parse_number(parts[4].trim_start_matches('<'), line_no)?;
    if addr > 0xFF {
        return Err(AsmError {
            line: line_no,
            message: "bit transfer requires direct address".into(),
        });
    }
    let postbyte = (reg_code << 6) | ((src_bit as u8) << 3) | (dst_bit as u8);
    Ok(vec![0x11, opcode, postbyte, addr as u8])
}

fn encode_ldmd(parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    let rest = operand.strip_prefix('#').ok_or_else(|| AsmError {
        line: line_no,
        message: "LDMD requires immediate operand".into(),
    })?;
    let value = parse_number(rest, line_no)?;
    if value > 0xFF {
        return Err(AsmError {
            line: line_no,
            message: "LDMD value out of range".into(),
        });
    }
    Ok(vec![0x11, 0x3D, value as u8])
}

fn encode_psh_pul(opcode: u8, parts: &[&str], line_no: usize) -> Result<Vec<u8>, AsmError> {
    let operand = parse_operand(parts, line_no)?;
    let postbyte = parse_stack_postbyte(operand, line_no)?;
    Ok(vec![opcode, postbyte])
}

fn encode_branch(
    opcode: u8,
    parts: &[&str],
    pc: u32,
    line_no: usize,
    labels: &std::collections::HashMap<String, u32>,
) -> Result<Vec<u8>, AsmError> {
    let target_text = if parts.len() > 1 {
        parts[1]
    } else {
        "*"
    };

    let target = if let Some(addr) = labels.get(target_text) {
        *addr
    } else {
        parse_number(target_text, line_no)?
    };

    let offset_pc = pc + 2;
    let delta = target as i64 - offset_pc as i64;
    if !(-128..=127).contains(&delta) {
        return Err(AsmError {
            line: line_no,
            message: format!("branch out of range: delta={delta}"),
        });
    }
    Ok(vec![opcode, delta as i8 as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm_bytes(source: &str) -> Vec<u8> {
        assemble(source).unwrap().bytes
    }

    #[test]
    fn assemble_reports_org_origin() {
        let prog = assemble("ORG $C000\nNOP\nEND").unwrap();
        assert_eq!(prog.origin, 0xC000);
        assert_eq!(prog.bytes, vec![0x12]);
    }

    #[test]
    fn disassemble_nop() {
        let insns = disassemble(&[0x12], 0x0100);
        assert_eq!(insns.len(), 1);
        assert_eq!(insns[0].text, "NOP");
        assert_eq!(insns[0].address, 0x0100);
    }

    #[test]
    fn assemble_lda_immediate() {
        let bytes = asm_bytes("LDA #$42");
        assert_eq!(bytes, vec![0x86, 0x42]);
    }

    #[test]
    fn assemble_with_org() {
        let bytes = asm_bytes("ORG $0200\nNOP\nLDA #$10");
        assert_eq!(bytes.len(), 3);
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[1], 0x86);
        assert_eq!(bytes[2], 0x10);
    }

    #[test]
    fn assemble_inline_label() {
        let bytes = asm_bytes(
            "ORG $0100\nstart LDA #$42\nNOP\nBRA start\nEND",
        );
        assert_eq!(bytes, vec![0x86, 0x42, 0x12, 0x20, 0xFB]);
    }

    #[test]
    fn assemble_label_colon_same_line() {
        let bytes = asm_bytes("ORG $0100\nloop: NOP\nBRA loop\nEND");
        assert_eq!(bytes, vec![0x12, 0x20, 0xFD]);
    }

    #[test]
    fn round_trip_ldx() {
        let bytes = asm_bytes("LDX #$1234");
        let insns = disassemble(&bytes, 0x0100);
        assert_eq!(insns[0].text, "LDX #$1234");
    }

    #[test]
    fn disassemble_bra_loop_terminates() {
        let bytes = asm_bytes("ORG $0100\nstart LDA #$42\nNOP\nBRA start\nEND");
        let insns = disassemble(&bytes, 0x0100);
        assert_eq!(insns.len(), 3);
        assert_eq!(insns[0].text, "LDA #$42");
        assert_eq!(insns[1].text, "NOP");
        assert_eq!(insns[2].text, "BRA $0100");
    }

    #[test]
    fn assemble_dex() {
        let bytes = asm_bytes("DEX");
        assert_eq!(bytes, vec![0x30, 0x1F]); // LEAX -1,X
    }

    #[test]
    fn assemble_rmb() {
        let bytes = asm_bytes("ORG $0100\nRMB 4\nNOP\nEND");
        assert_eq!(bytes.len(), 5);
        assert_eq!(bytes[4], 0x12);
    }

    #[test]
    fn assemble_equ_rmb() {
        let bytes = asm_bytes("COUNT EQU 2\nORG $0100\nRMB COUNT\nNOP\nEND");
        assert_eq!(bytes.len(), 3);
        assert_eq!(bytes[2], 0x12);
    }

    #[test]
    fn assemble_stx_indexed_y() {
        let bytes = asm_bytes("STX ,Y");
        assert_eq!(bytes, vec![0xAF, 0xA4]); // ,Y = 0x80|0x20|0x04 = 0xA4
    }

    #[test]
    fn assemble_lbra() {
        let bytes = asm_bytes("ORG $0100\nLBRA target\nNOP\nNOP\ntarget NOP\nEND");
        assert_eq!(bytes[0], 0x16);
        assert_eq!(bytes[1], 0x00);
        assert_eq!(bytes[2], 0x02);
    }

    #[test]
    fn assemble_pshs_puls() {
        let bytes = asm_bytes(
            "ORG $0100\nLDS #$01FF\nLDA #$41\nPSHS A\nPULS B\nNOP\nEND",
        );
        assert_eq!(bytes[0..4], [0x10, 0xCE, 0x01, 0xFF]);
        assert_eq!(bytes[4..6], [0x86, 0x41]);
        assert_eq!(bytes[6..8], [0x34, 0x02]); // PSHS A: A=0x02
        assert_eq!(bytes[8..10], [0x35, 0x04]); // PULS B: B=0x04
        assert_eq!(bytes[10], 0x12);
    }

    #[test]
    fn assemble_pshs_multiple() {
        let bytes = asm_bytes("PSHS A,B,X");
        assert_eq!(bytes, vec![0x34, 0x16]); // A=0x02, B=0x04, X=0x10 → 0x16
    }

    #[test]
    fn assemble_6309_muld_ldw() {
        let bytes = asm_bytes("LDW #$000A\nMULD");
        assert_eq!(bytes, vec![0x10, 0x86, 0x00, 0x0A, 0x10, 0x3E]);
    }

    #[test]
    fn assemble_6309_leax() {
        let bytes = asm_bytes("LEAX 5,X");
        assert_eq!(bytes, vec![0x30, 0x05]); // 5,X = 5-bit constant 5 = 0x05
    }

    #[test]
    fn assemble_6309_divd_tfm() {
        let bytes = asm_bytes("DIVD #$0002\nTFM+ X+,Y+");
        assert_eq!(bytes, vec![0x11, 0x8D, 0x00, 0x02, 0x11, 0x38, 0x12]);
    }

    #[test]
    fn assemble_pshsw_and_muld_operand() {
        let bytes = asm_bytes("PSHSW\nMULD #$000A");
        assert_eq!(bytes[0..2], [0x10, 0x38]);
        assert_eq!(bytes[2..6], [0x11, 0x8F, 0x00, 0x0A]);
    }

    #[test]
    fn assemble_6309_ldq_sexw_stq() {
        let bytes = asm_bytes("LDQ #$00010002\nSEXW\nSTQ >$3000");
        assert_eq!(bytes[0..5], [0xCD, 0x00, 0x01, 0x00, 0x02]);
        assert_eq!(bytes[5], 0x14);
        assert_eq!(bytes[6..10], [0x10, 0xFD, 0x30, 0x00]);
    }

    #[test]
    fn assemble_lds_branch_label() {
        let bytes = asm_bytes(
            "ORG $C000\nLDS #$0400\nLDX #$05FF\nloop STA ,X\nDEX\nCMPX #$03FF\nBNE loop\nEND",
        );
        assert_eq!(bytes[0..4], [0x10, 0xCE, 0x04, 0x00]);
        assert_eq!(bytes[bytes.len() - 2], 0x26);
        assert_eq!(bytes[bytes.len() - 1] as i8, -9);
    }

    #[test]
    fn assemble_ldy_beq_done_label() {
        let bytes = asm_bytes(
            "ORG $C000\nLDS #$0400\nLDY #$0400\nLDX #$C080\nPL LDA ,X\nBEQ DONE\nSTA ,Y\nLEAX 1,X\nLEAY 1,Y\nBRA PL\nDONE JMP $0100\nEND",
        );
        let jmp_idx = bytes.windows(3).position(|w| w == [0x7E, 0x01, 0x00]).expect("jmp");
        let beq_idx = bytes.iter().position(|&b| b == 0x27).expect("beq");
        let delta = (jmp_idx as i64) - (beq_idx as i64 + 2);
        assert_eq!(bytes[beq_idx + 1] as i8, delta as i8);
    }

    #[test]
    fn assemble_coco2_io_extended_address() {
        let bytes = asm_bytes(
            "ORG $0100\nLDA #$FE\nSTA $FF00\nLDA $FF02\nSTA $0100\nEND",
        );
        assert_eq!(bytes[0..2], [0x86, 0xFE]);
        assert_eq!(bytes[2..5], [0xB7, 0xFF, 0x00]);
        assert_eq!(bytes[5..8], [0xB6, 0xFF, 0x02]);
        assert_eq!(bytes[8..11], [0xB7, 0x01, 0x00]);
    }

    #[test]
    fn disassemble_hd6309_leax() {
        use m6809_core::CpuVariant;
        // 5,X = postbyte 0x05 (5-bit constant offset, bit7=0, reg=00=X, off=00101=5)
        let insns = disassemble_with_variant(&[0x30, 0x05, 0x12], 0x0100, CpuVariant::Hd6309);
        assert_eq!(insns[0].text, "LEAX 5,X");
    }

    #[test]
    fn lbrn_assembles_to_page2_prefix() {
        let bytes = asm_bytes("ORG $0100\nLBRN target\nNOP\ntarget NOP\nEND");
        assert_eq!(bytes[0], 0x10, "LBRN must emit page-2 prefix 0x10");
        assert_eq!(bytes[1], 0x21, "LBRN must emit 0x21 (LBRN condition)");
        let off = i16::from_be_bytes([bytes[2], bytes[3]]);
        assert_eq!(off, 1, "LBRN to target at $0105, offset_pc=$0104, delta=+1");
    }

    #[test]
    fn cwai_assembles_to_3c() {
        let bytes = asm_bytes("ORG $0100\nCWAI #$FF\nEND");
        assert_eq!(bytes, vec![0x3C, 0xFF]);
    }

    #[test]
    fn lbrn_disassembles_as_lbrn_not_lbsr() {
        use m6809_core::CpuVariant;
        let insns = disassemble_with_variant(&[0x10, 0x21, 0x00, 0x00], 0x0100, CpuVariant::Mc6809);
        assert_eq!(insns[0].text, "LBRN $0104", "0x10 0x21 must disassemble as LBRN");
    }

    #[test]
    fn assemble_comma_x_zero_offset() {
        let bytes = asm_bytes("LDA ,X");
        assert_eq!(bytes, vec![0xA6, 0x84]); // ,X = 0x80|0x00|0x04 = 0x84
    }

    #[test]
    fn assemble_const5_offset() {
        let bytes = asm_bytes("LDA 5,X");
        assert_eq!(bytes, vec![0xA6, 0x05]); // 5,X = 5-bit constant 5
    }

    #[test]
    fn assemble_const5_negative() {
        let bytes = asm_bytes("LDA -2,X");
        assert_eq!(bytes, vec![0xA6, 0x1E]); // -2,X = 5-bit constant -2 = 0x1E
    }

    #[test]
    fn assemble_auto_inc() {
        let bytes = asm_bytes("LDA ,X+");
        assert_eq!(bytes, vec![0xA6, 0x80]); // ,X+ = 0x80|0x00|0x00 = 0x80
    }

    #[test]
    fn assemble_auto_inc2() {
        let bytes = asm_bytes("LDA ,X++");
        assert_eq!(bytes, vec![0xA6, 0x81]); // ,X++ = 0x80|0x00|0x01 = 0x81
    }

    #[test]
    fn assemble_auto_dec() {
        let bytes = asm_bytes("LDA ,-X");
        assert_eq!(bytes, vec![0xA6, 0x82]); // ,-X = 0x80|0x00|0x02 = 0x82
    }

    #[test]
    fn assemble_auto_dec2() {
        let bytes = asm_bytes("LDA ,--X");
        assert_eq!(bytes, vec![0xA6, 0x83]); // ,--X = 0x80|0x00|0x03 = 0x83
    }

    #[test]
    fn assemble_acc_a_offset() {
        let bytes = asm_bytes("LDA A,X");
        assert_eq!(bytes, vec![0xA6, 0x86]); // A,X = 0x80|0x00|0x06 = 0x86
    }

    #[test]
    fn assemble_acc_b_offset() {
        let bytes = asm_bytes("LDA B,X");
        assert_eq!(bytes, vec![0xA6, 0x85]); // B,X = 0x80|0x00|0x05 = 0x85
    }

    #[test]
    fn assemble_acc_d_offset() {
        let bytes = asm_bytes("LDA D,X");
        assert_eq!(bytes, vec![0xA6, 0x8B]); // D,X = 0x80|0x00|0x0B = 0x8B
    }

    #[test]
    fn assemble_off8_signed() {
        let bytes = asm_bytes("LDA 100,X");
        assert_eq!(bytes, vec![0xA6, 0x88, 100]); // n8,X = 0x88 + 1 byte
    }

    #[test]
    fn assemble_off16_signed() {
        let bytes = asm_bytes("LDA 1000,X");
        assert_eq!(bytes, vec![0xA6, 0x89, 0x03, 0xE8]); // n16,X = 0x89 + 2 bytes
    }

    #[test]
    fn assemble_pcr8() {
        // LDA $0105,PCR at ORG $0100: instruction is 3 bytes (A6 8C offset),
        // PC_after = $0103, offset = $0105 - $0103 = 2
        let bytes = asm_bytes("ORG $0100\nLDA $0105,PCR\nEND");
        assert_eq!(bytes, vec![0xA6, 0x8C, 0x02]);
    }

    #[test]
    fn assemble_indirect_extended() {
        let bytes = asm_bytes("LDA [$1234]");
        assert_eq!(bytes, vec![0xA6, 0x9F, 0x12, 0x34]); // [addr] = 0x9F + 2 bytes
    }

    #[test]
    fn assemble_indirect_zero_offset() {
        let bytes = asm_bytes("LDA [,X]");
        assert_eq!(bytes, vec![0xA6, 0x94]); // [,X] = 0x90|0x00|0x04 = 0x94
    }

    #[test]
    fn roundtrip_indexed_modes() {
        use m6809_core::CpuVariant;
        // Assemble → disassemble roundtrip for key modes
        let cases = vec![
            (",X", vec![0xA6, 0x84]),
            ("5,X", vec![0xA6, 0x05]),
            (",X+", vec![0xA6, 0x80]),
            (",X++", vec![0xA6, 0x81]),
            (",-X", vec![0xA6, 0x82]),
            (",--X", vec![0xA6, 0x83]),
            ("A,X", vec![0xA6, 0x86]),
            ("B,X", vec![0xA6, 0x85]),
            ("D,X", vec![0xA6, 0x8B]),
        ];
        for (src, expected_bytes) in cases {
            let bytes = asm_bytes(&format!("LDA {src}"));
            assert_eq!(bytes, expected_bytes, "assemble mismatch for {src}");
            let insns = disassemble_with_variant(&bytes, 0x0100, CpuVariant::Mc6809);
            assert_eq!(insns[0].text, format!("LDA {src}"), "disasm mismatch for {src}");
        }
    }

    #[test]
    fn pshs_d_equals_a_and_b() {
        let bytes = asm_bytes("PSHS D");
        assert_eq!(bytes, vec![0x34, 0x06]); // D = A|B = 0x02|0x04 = 0x06
    }

    #[test]
    fn pshs_cc_assembles() {
        let bytes = asm_bytes("PSHS CC");
        assert_eq!(bytes, vec![0x34, 0x01]); // CC = 0x01
    }

    #[test]
    fn puls_cc_assembles() {
        let bytes = asm_bytes("PULS CC");
        assert_eq!(bytes, vec![0x35, 0x01]); // CC = 0x01
    }

    #[test]
    fn assemble_all_long_conditionals() {
        let cases = vec![
            ("LBHI", 0x22u8), ("LBLS", 0x23), ("LBCC", 0x24), ("LBCS", 0x25),
            ("LBNE", 0x26), ("LBEQ", 0x27), ("LBVC", 0x28), ("LBVS", 0x29),
            ("LBPL", 0x2A), ("LBMI", 0x2B), ("LBGE", 0x2C), ("LBLT", 0x2D),
            ("LBGT", 0x2E), ("LBLE", 0x2F),
        ];
        for (mnemonic, opcode) in cases {
            let bytes = asm_bytes(&format!("ORG $0100\n{mnemonic} target\nNOP\ntarget NOP\nEND"));
            assert_eq!(bytes[0], 0x10, "{mnemonic} must emit page-2 prefix 0x10");
            assert_eq!(bytes[1], opcode, "{mnemonic} must emit opcode ${opcode:02X}");
            assert_eq!(bytes.len(), 6, "{mnemonic} total binary should be 6 bytes (4+1+1)");
        }
    }

    #[test]
    fn assemble_lbsr() {
        let bytes = asm_bytes("ORG $0100\nLBSR target\nNOP\ntarget NOP\nEND");
        assert_eq!(bytes[0], 0x17, "LBSR must emit 0x17 (page-1 LBSR)");
        assert_eq!(bytes.len(), 5, "LBSR total binary should be 5 bytes (3+1+1)");
    }

    #[test]
    fn disassemble_long_conditionals() {
        use m6809_core::CpuVariant;
        let cases = vec![
            (0x22, "LBHI"), (0x23, "LBLS"), (0x24, "LBCC"), (0x25, "LBCS"),
            (0x26, "LBNE"), (0x27, "LBEQ"), (0x28, "LBVC"), (0x29, "LBVS"),
            (0x2A, "LBPL"), (0x2B, "LBMI"), (0x2C, "LBGE"), (0x2D, "LBLT"),
            (0x2E, "LBGT"), (0x2F, "LBLE"),
        ];
        for (opcode, name) in cases {
            let insns = disassemble_with_variant(&[0x10, opcode, 0x00, 0x00], 0x0100, CpuVariant::Mc6809);
            assert!(insns[0].text.starts_with(name), "0x10 ${opcode:02X} must disassemble as {name}, got {}", insns[0].text);
        }
    }

    #[test]
    fn fdb_accepts_label_reference() {
        let bytes = asm_bytes("ORG $0100\ntarget NOP\nORG $0102\nFDB target\nEND");
        assert_eq!(bytes, vec![0x12, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn tfr_assembles_4bit_encoding() {
        // TFR D,X → opcode 0x1F, postbyte (D=0 << 4 | X=1) = 0x01
        let bytes = asm_bytes("TFR D,X");
        assert_eq!(bytes, vec![0x1F, 0x01]);
    }

    #[test]
    fn exg_separate_opcode_from_tfr() {
        // EXG A,B → opcode 0x1E, postbyte (A=8 << 4 | B=9) = 0x89
        let bytes = asm_bytes("EXG A,B");
        assert_eq!(bytes, vec![0x1E, 0x89]);
    }

    #[test]
    fn tfr_postbyte_4bit_encoding() {
        let cases = vec![
            ("TFR D,X",  0x1F, 0x01),  // D=0, X=1
            ("TFR X,Y",  0x1F, 0x12),  // X=1, Y=2
            ("TFR A,B",  0x1F, 0x89),  // A=8, B=9
            ("TFR CC,DP", 0x1F, 0xAB), // CC=0xA, DP=0xB
            ("TFR PC,X", 0x1F, 0x51),  // PC=5, X=1
        ];
        for (src, expected_op, expected_pb) in cases {
            let bytes = asm_bytes(src);
            assert_eq!(bytes, vec![expected_op, expected_pb], "mismatch for {src}");
        }
    }

    #[test]
    fn orcc_andcc_new_opcodes() {
        let bytes = asm_bytes("ORCC #$01");
        assert_eq!(bytes, vec![0x1A, 0x01]);
        let bytes = asm_bytes("ANDCC #$FE");
        assert_eq!(bytes, vec![0x1C, 0xFE]);
    }

    #[test]
    fn inx_dex_iny_dey_lea_aliases() {
        assert_eq!(asm_bytes("INX"), vec![0x30, 0x01]); // LEAX 1,X
        assert_eq!(asm_bytes("DEX"), vec![0x30, 0x1F]); // LEAX -1,X
        assert_eq!(asm_bytes("INY"), vec![0x31, 0x21]); // LEAY 1,Y
        assert_eq!(asm_bytes("DEY"), vec![0x31, 0x3F]); // LEAY -1,Y
    }
}