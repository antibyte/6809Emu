use std::collections::HashMap;
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
    /// Maps the 1-based **original source line number** to the address of the
    /// code emitted by that line. Lines that produce no code (comments, blank
    /// lines, `EQU`/`SET`, `END`, and pure-label lines) are omitted. Used to set
    /// breakpoints directly in source code via the original line number.
    pub line_map: HashMap<usize, u16>,
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

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, AsmError> {
    Err(AsmError {
        line,
        message: message.into(),
    })
}

/// Longest instruction (e.g. `LDQ #imm32`, `AIM #i,n16,X`, `11 A6 9F hh ll`).
const MAX_INSN_LEN: usize = 5;

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
        // Decoding executes the instruction on scratch memory; restore the
        // bytes of this instruction in case an earlier one stored over them.
        let offset = usize::from(pc - start_pc);
        let len = (original.len() - offset).min(MAX_INSN_LEN);
        let _ = mem.load_binary(pc, &original[offset..offset + len]);

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

// ── Source parsing ───────────────────────────────────────────────────────

/// Parsed source line: optional label + statement text.
struct ParsedLine {
    label: Option<String>,
    statement: String,
}

fn normalize_token(token: &str) -> String {
    token.trim_end_matches(':').to_uppercase()
}

const DIRECTIVES: [&str; 7] = ["ORG", "FCB", "FDB", "RMB", "END", "EQU", "SET"];

fn is_statement_keyword(token: &str) -> bool {
    let name = normalize_token(token);
    DIRECTIVES.contains(&name.as_str()) || lookup(&name).is_some()
}

/// Split `label LDA #$42`, `label: LDA #$42`, or standalone `label:`.
/// `indented`: the source line starts with blanks. It only decides the
/// ambiguous case where both leading tokens are mnemonics: `CLR STA ,X` in
/// column 1 is the label CLR, an indented `BNE CLR` branches to it.
fn parse_source_line(line: &str, indented: bool) -> ParsedLine {
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
    let first_is_label = parts[0].ends_with(':')
        || !is_statement_keyword(parts[0])
        || !indented;
    if parts.len() >= 2 && is_statement_keyword(parts[1]) && first_is_label {
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

// ── Expressions ──────────────────────────────────────────────────────────

/// Symbol context for evaluating operands.
struct Env<'a> {
    labels: &'a HashMap<String, u32>,
    /// Address of the current statement (`*`).
    pc: u32,
    /// First pass: undefined symbols evaluate to 0 (only sizes matter there).
    lenient: bool,
}

impl Env<'_> {
    /// Evaluate `term {+|- term}` where a term is a number (`$hex`, `%bin`,
    /// `@oct`, decimal), a label or `*`; a leading `-` negates.
    fn eval(&self, text: &str, line: usize) -> Result<i64, AsmError> {
        let mut total = 0i64;
        let mut sign = 1i64;
        let mut term = String::new();
        for ch in text.trim().chars() {
            match ch {
                '+' | '-' if !term.is_empty() => {
                    total += sign * self.term(&term, line)?;
                    term.clear();
                    sign = if ch == '-' { -1 } else { 1 };
                }
                '-' => sign = -sign,
                '+' => {}
                c if c.is_whitespace() => {}
                c => term.push(c),
            }
        }
        if term.is_empty() {
            return err(line, format!("missing value: {text}"));
        }
        Ok(total + sign * self.term(&term, line)?)
    }

    fn term(&self, text: &str, line: usize) -> Result<i64, AsmError> {
        if text == "*" {
            return Ok(i64::from(self.pc));
        }
        if let Ok(value) = parse_number(text, line) {
            return Ok(i64::from(value));
        }
        match self.labels.get(&normalize_token(text)) {
            Some(value) => Ok(i64::from(*value)),
            None if self.lenient => Ok(0),
            None => err(line, format!("undefined label: {text}")),
        }
    }

    fn byte(&self, text: &str, line: usize) -> Result<u8, AsmError> {
        let value = self.eval(text, line)?;
        if !(-128..=0xFF).contains(&value) {
            return err(line, format!("8-bit value out of range: {text}"));
        }
        Ok(value as u8)
    }

    fn word(&self, text: &str, line: usize) -> Result<u16, AsmError> {
        let value = self.eval(text, line)?;
        if !(-0x8000..=0xFFFF).contains(&value) {
            return err(line, format!("16-bit value out of range: {text}"));
        }
        Ok(value as u16)
    }
}

/// A plain numeric literal (no labels): its size can be decided in pass 1.
fn is_numeric(text: &str) -> bool {
    let t = text.trim();
    parse_number(t.strip_prefix('-').unwrap_or(t), 0).is_ok()
}

/// Evaluate a directive operand (`*` is the current address `pc`).
fn parse_expression(
    text: &str,
    line: usize,
    labels: &HashMap<String, u32>,
    pc: u32,
) -> Result<u32, AsmError> {
    let env = Env {
        labels,
        pc,
        lenient: false,
    };
    Ok(env.eval(text, line)? as u32)
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
    labels: &HashMap<String, u32>,
    pc: u32,
) -> Result<Vec<u32>, AsmError> {
    let mut values = Vec::new();
    for part in text.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let value = parse_expression(p, line, labels, pc)?;
        if words {
            if value > 0xFFFF {
                return err(line, format!("FDB value out of range: {p}"));
            }
        } else if value > 0xFF {
            return err(line, format!("FCB value out of range: {p}"));
        }
        values.push(value);
    }
    Ok(values)
}

/// Number of items in an FCB/FDB list (pass 1 does not evaluate them, so
/// forward references work).
fn data_list_len(text: &str) -> u32 {
    text.split(',').filter(|p| !p.trim().is_empty()).count() as u32
}

// ── Two-pass assembly ────────────────────────────────────────────────────

fn advance_pc_for_statement(
    statement: &str,
    line_no: usize,
    scan_pc: &mut u32,
    origin: &mut u32,
    labels: &HashMap<String, u32>,
) -> Result<(), AsmError> {
    if statement.is_empty() {
        return Ok(());
    }

    let upper = statement.to_uppercase();
    let parts: Vec<&str> = upper.split_whitespace().collect();
    if lookup(parts[0]).is_some() {
        *scan_pc += statement_size(statement, &parts, *scan_pc, line_no, labels)?;
    } else if upper.starts_with("ORG ") {
        let addr = parse_number(statement[4..].trim(), line_no)?;
        *scan_pc = addr;
        *origin = addr;
    } else if upper.starts_with("FCB") {
        *scan_pc += data_list_len(&statement[3..]);
    } else if upper.starts_with("FDB") {
        *scan_pc += data_list_len(&statement[3..]) * 2;
    } else if upper.starts_with("RMB ") {
        let count = parse_expression(statement[4..].trim(), line_no, labels, *scan_pc)?;
        *scan_pc += count;
    } else if upper.starts_with("EQU ") || upper.starts_with("SET ") {
        // Label assignment only; does not advance PC.
    } else if upper == "END" {
        return Ok(());
    } else {
        return err(line_no, format!("unknown instruction for label pass: {statement}"));
    }
    Ok(())
}

/// Instruction length in pass 1. Branches have fixed sizes; everything else is
/// encoded with undefined labels read as 0, which never changes the size
/// (symbolic addresses always use the long forms).
fn statement_size(
    statement: &str,
    parts: &[&str],
    pc: u32,
    line_no: usize,
    labels: &HashMap<String, u32>,
) -> Result<u32, AsmError> {
    match lookup(parts[0]) {
        Some(Enc::Branch(_)) => Ok(2),
        Some(Enc::LongBranch(op)) => Ok(op.len() as u32 + 2),
        _ => {
            let env = Env {
                labels,
                pc,
                lenient: true,
            };
            Ok(encode_instruction(statement, &env, line_no, parts)?.len() as u32)
        }
    }
}

/// Assemble source text into machine code.
///
/// Supports common directives and instructions:
/// - `ORG $addr`, `FCB $01,2,3` / `FDB $1234,label`, `RMB n`, `EQU`/`SET`, `END`
/// - Labels: `start:` or inline `start LDA #$42`
/// - The complete MC6809 and HD6309 instruction set with Motorola syntax
///   (`#imm`, `<dp`, `>ext`, auto-sized `$addr`, all indexed modes incl. the
///   6309 W modes, `label,PCR`), labels and `label+n` in operands, `*` = PC.
pub fn assemble(source: &str) -> Result<AssembledProgram, AsmError> {
    let mut origin: u32 = 0x0100;
    let mut pc: u32 = origin;
    let mut output: Vec<(u32, u8)> = Vec::new();
    let mut labels: HashMap<String, u32> = HashMap::new();

    // (line number, text without comment, starts with blanks)
    let lines: Vec<(usize, String, bool)> = source
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let text = l.split(';').next().unwrap_or("");
            (i + 1, text.trim().to_string(), text.starts_with([' ', '\t']))
        })
        .filter(|(_, l, _)| !l.is_empty())
        .collect();

    // First pass: record labels.
    let mut scan_pc = origin;
    for (line_no, line, indented) in &lines {
        let parsed = parse_source_line(line, *indented);
        let upper = parsed.statement.to_uppercase();
        if let Some(name) = parsed.label {
            if upper.starts_with("EQU ") || upper.starts_with("SET ") {
                let value =
                    parse_expression(parsed.statement[4..].trim(), *line_no, &labels, scan_pc)?;
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
    let mut line_map: HashMap<usize, u16> = HashMap::new();
    for (line_no, line, indented) in lines {
        let parsed = parse_source_line(&line, indented);
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
        if lookup(parts[0]).is_none() {
            if upper.starts_with("FCB") {
                record_line(&mut line_map, line_no, pc);
                for value in parse_data_list(&parsed.statement[3..], line_no, false, &labels, pc)? {
                    emit(&mut output, pc, value as u8);
                    pc += 1;
                }
                continue;
            }
            if upper.starts_with("FDB") {
                record_line(&mut line_map, line_no, pc);
                for value in parse_data_list(&parsed.statement[3..], line_no, true, &labels, pc)? {
                    emit(&mut output, pc, (value >> 8) as u8);
                    pc += 1;
                    emit(&mut output, pc, value as u8);
                    pc += 1;
                }
                continue;
            }
            if upper.starts_with("RMB ") {
                record_line(&mut line_map, line_no, pc);
                let count = parse_expression(parsed.statement[4..].trim(), line_no, &labels, pc)?;
                for _ in 0..count {
                    emit(&mut output, pc, 0);
                    pc += 1;
                }
                continue;
            }
            if upper.starts_with("EQU ") || upper.starts_with("SET ") {
                continue;
            }
        }

        record_line(&mut line_map, line_no, pc);
        let env = Env {
            labels: &labels,
            pc,
            lenient: false,
        };
        let bytes = encode_instruction(&parsed.statement, &env, line_no, &parts)?;
        for b in bytes {
            emit(&mut output, pc, b);
            pc += 1;
        }
    }

    if output.is_empty() {
        return Ok(AssembledProgram {
            origin: origin as u16,
            bytes: Vec::new(),
            line_map: HashMap::new(),
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
        line_map,
    })
}

/// Records the address of the first byte emitted by a source line so that
/// breakpoints can be set on the original (1-based) source line number.
fn record_line(line_map: &mut HashMap<usize, u16>, line: usize, addr: u32) {
    line_map.entry(line).or_insert(addr as u16);
}

fn emit(out: &mut Vec<(u32, u8)>, addr: u32, byte: u8) {
    if let Some(existing) = out.iter_mut().find(|(a, _)| *a == addr) {
        existing.1 = byte;
    } else {
        out.push((addr, byte));
    }
}

// ── Instruction table ────────────────────────────────────────────────────

/// Immediate operand size of a general instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Imm {
    Byte,
    Word,
}

/// How a mnemonic is encoded. `prefix` is 0 (page 1), $10 or $11.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Enc {
    /// Fixed bytes, no operand.
    Inherent(&'static [u8]),
    /// Opcode bytes followed by `#imm8` (ORCC, ANDCC, CWAI, LDMD, BITMD).
    ImmediateByte(&'static [u8]),
    /// Row layout: `#` = base, direct = base+$10, indexed = base+$20,
    /// extended = base+$30.
    General { prefix: u8, base: u8, imm: Imm },
    /// Same layout without an immediate form (stores, JSR).
    Store { prefix: u8, base: u8 },
    /// Page-1 memory layout: direct = base, indexed = base+$60, extended = base+$70.
    Memory(u8),
    /// HD6309 OIM/AIM/EIM/TIM `#imm,address` (memory layout, immediate first).
    LogicImmediate(u8),
    /// LEAX/LEAY/LEAS/LEAU: indexed only.
    Lea(u8),
    /// PSHS/PULS/PSHU/PULU register list (or a `$xx` postbyte).
    Stack(u8),
    /// TFR/EXG and the HD6309 inter-register operations: `src,dst`.
    RegisterPair(&'static [u8]),
    /// HD6309 LDQ: `#imm32` is $CD, memory forms are $10DC/$10EC/$10FC.
    Ldq,
    /// HD6309 BAND..STBT ($11 $30-$37).
    BitTransfer(u8),
    /// HD6309 TFM; the TFM+ / TFM- / TFM+R / TFM+W aliases fix the opcode.
    Tfm(Option<u8>),
    /// 8-bit relative branch.
    Branch(u8),
    /// 16-bit relative branch (opcode bytes).
    LongBranch(&'static [u8]),
    /// A known mnemonic that must not be assembled (reason).
    Unsupported(&'static str),
}

/// Encoding of every supported mnemonic (MC6809 and HD6309, MAME hd6309.lst).
fn lookup(mnemonic: &str) -> Option<Enc> {
    use Enc::*;
    const P2: u8 = 0x10;
    const P3: u8 = 0x11;
    let general = |prefix, base, imm| General { prefix, base, imm };
    let store = |prefix, base| Store { prefix, base };
    Some(match mnemonic {
        // ── MC6809 inherent ──
        "NOP" => Inherent(&[0x12]),
        "SYNC" => Inherent(&[0x13]),
        "DAA" => Inherent(&[0x19]),
        "SEX" => Inherent(&[0x1D]),
        "RTS" => Inherent(&[0x39]),
        "ABX" => Inherent(&[0x3A]),
        "RTI" => Inherent(&[0x3B]),
        "MUL" => Inherent(&[0x3D]),
        "SWI" => Inherent(&[0x3F]),
        "SWI2" => Inherent(&[P2, 0x3F]),
        "SWI3" => Inherent(&[P3, 0x3F]),
        "NEGA" => Inherent(&[0x40]),
        "COMA" => Inherent(&[0x43]),
        "LSRA" => Inherent(&[0x44]),
        "RORA" => Inherent(&[0x46]),
        "ASRA" => Inherent(&[0x47]),
        "ASLA" | "LSLA" => Inherent(&[0x48]),
        "ROLA" => Inherent(&[0x49]),
        "DECA" => Inherent(&[0x4A]),
        "INCA" => Inherent(&[0x4C]),
        "TSTA" => Inherent(&[0x4D]),
        "CLRA" => Inherent(&[0x4F]),
        "NEGB" => Inherent(&[0x50]),
        "COMB" => Inherent(&[0x53]),
        "LSRB" => Inherent(&[0x54]),
        "RORB" => Inherent(&[0x56]),
        "ASRB" => Inherent(&[0x57]),
        "ASLB" | "LSLB" => Inherent(&[0x58]),
        "ROLB" => Inherent(&[0x59]),
        "DECB" => Inherent(&[0x5A]),
        "INCB" => Inherent(&[0x5C]),
        "TSTB" => Inherent(&[0x5D]),
        "CLRB" => Inherent(&[0x5F]),
        // LEAX 1,X / LEAX -1,X / LEAY 1,Y / LEAY -1,Y
        "INX" => Inherent(&[0x30, 0x01]),
        "DEX" => Inherent(&[0x30, 0x1F]),
        "INY" => Inherent(&[0x31, 0x21]),
        "DEY" => Inherent(&[0x31, 0x3F]),
        "ORCC" => ImmediateByte(&[0x1A]),
        "ANDCC" => ImmediateByte(&[0x1C]),
        "CWAI" => ImmediateByte(&[0x3C]),

        // ── MC6809 memory / immediate ──
        "SUBA" => general(0, 0x80, Imm::Byte),
        "CMPA" => general(0, 0x81, Imm::Byte),
        "SBCA" => general(0, 0x82, Imm::Byte),
        "ANDA" => general(0, 0x84, Imm::Byte),
        "BITA" => general(0, 0x85, Imm::Byte),
        "LDA" => general(0, 0x86, Imm::Byte),
        "EORA" => general(0, 0x88, Imm::Byte),
        "ADCA" => general(0, 0x89, Imm::Byte),
        "ORA" => general(0, 0x8A, Imm::Byte),
        "ADDA" => general(0, 0x8B, Imm::Byte),
        "SUBB" => general(0, 0xC0, Imm::Byte),
        "CMPB" => general(0, 0xC1, Imm::Byte),
        "SBCB" => general(0, 0xC2, Imm::Byte),
        "ANDB" => general(0, 0xC4, Imm::Byte),
        "BITB" => general(0, 0xC5, Imm::Byte),
        "LDB" => general(0, 0xC6, Imm::Byte),
        "EORB" => general(0, 0xC8, Imm::Byte),
        "ADCB" => general(0, 0xC9, Imm::Byte),
        "ORB" => general(0, 0xCA, Imm::Byte),
        "ADDB" => general(0, 0xCB, Imm::Byte),
        "SUBD" => general(0, 0x83, Imm::Word),
        "CMPX" => general(0, 0x8C, Imm::Word),
        "LDX" => general(0, 0x8E, Imm::Word),
        "ADDD" => general(0, 0xC3, Imm::Word),
        "LDD" => general(0, 0xCC, Imm::Word),
        "LDU" => general(0, 0xCE, Imm::Word),
        "CMPD" => general(P2, 0x83, Imm::Word),
        "CMPY" => general(P2, 0x8C, Imm::Word),
        "LDY" => general(P2, 0x8E, Imm::Word),
        "LDS" => general(P2, 0xCE, Imm::Word),
        "CMPU" => general(P3, 0x83, Imm::Word),
        "CMPS" => general(P3, 0x8C, Imm::Word),
        "STA" => store(0, 0x87),
        "STB" => store(0, 0xC7),
        "STX" => store(0, 0x8F),
        "STD" => store(0, 0xCD),
        "STU" => store(0, 0xCF),
        "STY" => store(P2, 0x8F),
        "STS" => store(P2, 0xCF),
        "JSR" => store(0, 0x8D),
        "NEG" => Memory(0x00),
        "COM" => Memory(0x03),
        "LSR" => Memory(0x04),
        "ROR" => Memory(0x06),
        "ASR" => Memory(0x07),
        "ASL" | "LSL" => Memory(0x08),
        "ROL" => Memory(0x09),
        "DEC" => Memory(0x0A),
        "INC" => Memory(0x0C),
        "TST" => Memory(0x0D),
        "JMP" => Memory(0x0E),
        "CLR" => Memory(0x0F),
        "LEAX" => Lea(0x30),
        "LEAY" => Lea(0x31),
        "LEAS" => Lea(0x32),
        "LEAU" => Lea(0x33),
        "PSHS" => Stack(0x34),
        "PULS" => Stack(0x35),
        "PSHU" => Stack(0x36),
        "PULU" => Stack(0x37),
        "EXG" => RegisterPair(&[0x1E]),
        "TFR" => RegisterPair(&[0x1F]),

        // ── Branches ──
        "BRA" => Branch(0x20),
        "BRN" => Branch(0x21),
        "BHI" => Branch(0x22),
        "BLS" => Branch(0x23),
        "BCC" | "BHS" => Branch(0x24),
        "BCS" | "BLO" => Branch(0x25),
        "BNE" => Branch(0x26),
        "BEQ" => Branch(0x27),
        "BVC" => Branch(0x28),
        "BVS" => Branch(0x29),
        "BPL" => Branch(0x2A),
        "BMI" => Branch(0x2B),
        "BGE" => Branch(0x2C),
        "BLT" => Branch(0x2D),
        "BGT" => Branch(0x2E),
        "BLE" => Branch(0x2F),
        "BSR" => Branch(0x8D),
        "LBRA" => LongBranch(&[0x16]),
        "LBSR" => LongBranch(&[0x17]),
        "LBRN" => LongBranch(&[P2, 0x21]),
        "LBHI" => LongBranch(&[P2, 0x22]),
        "LBLS" => LongBranch(&[P2, 0x23]),
        "LBCC" | "LBHS" => LongBranch(&[P2, 0x24]),
        "LBCS" | "LBLO" => LongBranch(&[P2, 0x25]),
        "LBNE" => LongBranch(&[P2, 0x26]),
        "LBEQ" => LongBranch(&[P2, 0x27]),
        "LBVC" => LongBranch(&[P2, 0x28]),
        "LBVS" => LongBranch(&[P2, 0x29]),
        "LBPL" => LongBranch(&[P2, 0x2A]),
        "LBMI" => LongBranch(&[P2, 0x2B]),
        "LBGE" => LongBranch(&[P2, 0x2C]),
        "LBLT" => LongBranch(&[P2, 0x2D]),
        "LBGT" => LongBranch(&[P2, 0x2E]),
        "LBLE" => LongBranch(&[P2, 0x2F]),

        // ── HD6309 page 1 ──
        "OIM" => LogicImmediate(0x01),
        "AIM" => LogicImmediate(0x02),
        "EIM" => LogicImmediate(0x05),
        "TIM" => LogicImmediate(0x0B),
        "SEXW" => Inherent(&[0x14]),
        "LDQ" => Ldq,

        // ── HD6309 page 2 ($10) ──
        "ADDR" => RegisterPair(&[P2, 0x30]),
        "ADCR" => RegisterPair(&[P2, 0x31]),
        "SUBR" => RegisterPair(&[P2, 0x32]),
        "SBCR" => RegisterPair(&[P2, 0x33]),
        "ANDR" => RegisterPair(&[P2, 0x34]),
        "ORR" => RegisterPair(&[P2, 0x35]),
        "EORR" => RegisterPair(&[P2, 0x36]),
        "CMPR" => RegisterPair(&[P2, 0x37]),
        "PSHSW" => Inherent(&[P2, 0x38]),
        "PULSW" => Inherent(&[P2, 0x39]),
        "PSHUW" => Inherent(&[P2, 0x3A]),
        "PULUW" => Inherent(&[P2, 0x3B]),
        "NEGD" => Inherent(&[P2, 0x40]),
        "COMD" => Inherent(&[P2, 0x43]),
        "LSRD" => Inherent(&[P2, 0x44]),
        "RORD" => Inherent(&[P2, 0x46]),
        "ASRD" => Inherent(&[P2, 0x47]),
        "ASLD" | "LSLD" => Inherent(&[P2, 0x48]),
        "ROLD" => Inherent(&[P2, 0x49]),
        "DECD" => Inherent(&[P2, 0x4A]),
        "INCD" => Inherent(&[P2, 0x4C]),
        "TSTD" => Inherent(&[P2, 0x4D]),
        "CLRD" => Inherent(&[P2, 0x4F]),
        // NEGW, ASRW and ASLW only exist in MAME's core (hd6309.lst); a real
        // HD6309 traps on $1050/$1057/$1058 (hoglet67 6809Decoder).
        "NEGW" | "ASRW" | "ASLW" | "LSLW" => Unsupported(
            "does not exist on the HD6309 ($1050/$1057/$1058 trap on the real chip)",
        ),
        "COMW" => Inherent(&[P2, 0x53]),
        "LSRW" => Inherent(&[P2, 0x54]),
        "RORW" => Inherent(&[P2, 0x56]),
        "ROLW" => Inherent(&[P2, 0x59]),
        "DECW" => Inherent(&[P2, 0x5A]),
        "INCW" => Inherent(&[P2, 0x5C]),
        "TSTW" => Inherent(&[P2, 0x5D]),
        "CLRW" => Inherent(&[P2, 0x5F]),
        "SUBW" => general(P2, 0x80, Imm::Word),
        "CMPW" => general(P2, 0x81, Imm::Word),
        "SBCD" => general(P2, 0x82, Imm::Word),
        "ANDD" => general(P2, 0x84, Imm::Word),
        "BITD" => general(P2, 0x85, Imm::Word),
        "LDW" => general(P2, 0x86, Imm::Word),
        "EORD" => general(P2, 0x88, Imm::Word),
        "ADCD" => general(P2, 0x89, Imm::Word),
        "ORD" => general(P2, 0x8A, Imm::Word),
        "ADDW" => general(P2, 0x8B, Imm::Word),
        "STW" => store(P2, 0x87),
        "STQ" => store(P2, 0xCD),

        // ── HD6309 page 3 ($11) ──
        "BAND" => BitTransfer(0x30),
        "BIAND" => BitTransfer(0x31),
        "BOR" => BitTransfer(0x32),
        "BIOR" => BitTransfer(0x33),
        "BEOR" => BitTransfer(0x34),
        "BIEOR" => BitTransfer(0x35),
        "LDBT" => BitTransfer(0x36),
        "STBT" => BitTransfer(0x37),
        "TFM" => Tfm(None),
        "TFM+" => Tfm(Some(0x38)),
        "TFM-" => Tfm(Some(0x39)),
        "TFM+R" => Tfm(Some(0x3A)),
        "TFM+W" => Tfm(Some(0x3B)),
        "BITMD" => ImmediateByte(&[P3, 0x3C]),
        "LDMD" => ImmediateByte(&[P3, 0x3D]),
        "COME" => Inherent(&[P3, 0x43]),
        "DECE" => Inherent(&[P3, 0x4A]),
        "INCE" => Inherent(&[P3, 0x4C]),
        "TSTE" => Inherent(&[P3, 0x4D]),
        "CLRE" => Inherent(&[P3, 0x4F]),
        "COMF" => Inherent(&[P3, 0x53]),
        "DECF" => Inherent(&[P3, 0x5A]),
        "INCF" => Inherent(&[P3, 0x5C]),
        "TSTF" => Inherent(&[P3, 0x5D]),
        "CLRF" => Inherent(&[P3, 0x5F]),
        "SUBE" => general(P3, 0x80, Imm::Byte),
        "CMPE" => general(P3, 0x81, Imm::Byte),
        "LDE" => general(P3, 0x86, Imm::Byte),
        "ADDE" => general(P3, 0x8B, Imm::Byte),
        "STE" => store(P3, 0x87),
        "SUBF" => general(P3, 0xC0, Imm::Byte),
        "CMPF" => general(P3, 0xC1, Imm::Byte),
        "LDF" => general(P3, 0xC6, Imm::Byte),
        "ADDF" => general(P3, 0xCB, Imm::Byte),
        "STF" => store(P3, 0xC7),
        // DIVD: D / signed 8-bit operand; DIVQ: Q / 16-bit; MULD: D * 16-bit.
        "DIVD" => general(P3, 0x8D, Imm::Byte),
        "DIVQ" => general(P3, 0x8E, Imm::Word),
        "MULD" => general(P3, 0x8F, Imm::Word),
        _ => return None,
    })
}

// ── Encoding ─────────────────────────────────────────────────────────────

fn encode_instruction(
    statement: &str,
    env: &Env,
    line_no: usize,
    parts: &[&str],
) -> Result<Vec<u8>, AsmError> {
    let Some(&mnemonic) = parts.first() else {
        return err(line_no, "empty instruction");
    };
    let Some(enc) = lookup(mnemonic) else {
        return err(line_no, format!("unsupported instruction: {mnemonic}"));
    };
    let operand = parts.get(1).copied();
    let need = || match operand {
        Some(op) => Ok(op),
        None => err(line_no, format!("missing operand: {statement}")),
    };

    match enc {
        Enc::Inherent(bytes) => Ok(bytes.to_vec()),
        Enc::Unsupported(reason) => err(line_no, format!("{mnemonic} {reason}")),
        Enc::ImmediateByte(opcode) => {
            let op = need()?;
            let Some(value) = op.strip_prefix('#') else {
                return err(line_no, format!("{mnemonic} requires an immediate operand: {op}"));
            };
            let mut bytes = opcode.to_vec();
            bytes.push(env.byte(value, line_no)?);
            Ok(bytes)
        }
        Enc::General { prefix, base, imm } => {
            let op = need()?;
            if let Some(value) = op.strip_prefix('#') {
                let mut bytes = with_prefix(prefix, base);
                match imm {
                    Imm::Byte => bytes.push(env.byte(value, line_no)?),
                    Imm::Word => bytes.extend(env.word(value, line_no)?.to_be_bytes()),
                }
                return Ok(bytes);
            }
            let ops = [base + 0x10, base + 0x20, base + 0x30];
            encode_memory(prefix, ops, &[], op, env, line_no)
        }
        Enc::Store { prefix, base } => {
            let op = need()?;
            if op.starts_with('#') {
                return err(line_no, format!("{mnemonic} has no immediate form: {op}"));
            }
            let ops = [base + 0x10, base + 0x20, base + 0x30];
            encode_memory(prefix, ops, &[], op, env, line_no)
        }
        Enc::Memory(base) => {
            let op = need()?;
            if op.starts_with('#') {
                return err(line_no, format!("{mnemonic} has no immediate form: {op}"));
            }
            encode_memory(0, [base, base + 0x60, base + 0x70], &[], op, env, line_no)
        }
        Enc::LogicImmediate(base) => {
            // AIM #$F0,<$20  AIM #$F0,$1234  AIM #$F0,5,X: the immediate
            // byte follows the opcode, then the address (MAME hd6309.lst).
            let op = need()?;
            let Some((imm, address)) = op
                .strip_prefix('#')
                .and_then(|rest| rest.split_once(','))
            else {
                return err(line_no, format!("{mnemonic} expects #imm,address: {op}"));
            };
            let imm = env.byte(imm, line_no)?;
            encode_memory(0, [base, base + 0x60, base + 0x70], &[imm], address, env, line_no)
        }
        Enc::Lea(opcode) => {
            let op = need()?;
            if !is_indexed_operand(op) {
                return err(line_no, format!("{op}: LEA requires indexed addressing"));
            }
            let mut bytes = vec![opcode];
            bytes.extend(encode_indexed(op, env.pc + 1, env, line_no)?);
            Ok(bytes)
        }
        Enc::Stack(opcode) => {
            let op = need()?;
            Ok(vec![opcode, parse_stack_postbyte(op, line_no)?])
        }
        Enc::RegisterPair(opcode) => {
            let op = need()?;
            let Some((src, dst)) = op.split_once(',') else {
                return err(line_no, format!("expected src,dst: {op}"));
            };
            let postbyte = (register_code(src, line_no)? << 4) | register_code(dst, line_no)?;
            let mut bytes = opcode.to_vec();
            bytes.push(postbyte);
            Ok(bytes)
        }
        Enc::Ldq => {
            let op = need()?;
            if let Some(value) = op.strip_prefix('#') {
                let value = env.eval(value, line_no)?;
                if !(-0x8000_0000..=0xFFFF_FFFF).contains(&value) {
                    return err(line_no, format!("32-bit value out of range: {op}"));
                }
                let mut bytes = vec![0xCD];
                bytes.extend((value as u32).to_be_bytes());
                return Ok(bytes);
            }
            encode_memory(0x10, [0xDC, 0xEC, 0xFC], &[], op, env, line_no)
        }
        Enc::BitTransfer(opcode) => encode_bit_transfer(opcode, need()?, env, line_no),
        Enc::Tfm(fixed) => encode_tfm(fixed, need()?, line_no),
        Enc::Branch(opcode) => {
            let target = env.eval(operand.unwrap_or("*"), line_no)?;
            let delta = target - (i64::from(env.pc) + 2);
            if !(-128..=127).contains(&delta) {
                return err(line_no, format!("branch out of range: delta={delta}"));
            }
            Ok(vec![opcode, delta as i8 as u8])
        }
        Enc::LongBranch(opcode) => {
            let target = env.eval(operand.unwrap_or("*"), line_no)?;
            let delta = target - (i64::from(env.pc) + opcode.len() as i64 + 2);
            if !(-32768..=32767).contains(&delta) {
                return err(line_no, format!("long branch out of range: delta={delta}"));
            }
            let mut bytes = opcode.to_vec();
            bytes.extend((delta as i16).to_be_bytes());
            Ok(bytes)
        }
    }
}

fn with_prefix(prefix: u8, opcode: u8) -> Vec<u8> {
    if prefix == 0 {
        vec![opcode]
    } else {
        vec![prefix, opcode]
    }
}

fn is_indexed_operand(operand: &str) -> bool {
    operand.contains(',') || operand.starts_with('[')
}

/// Direct / indexed / extended operand with opcodes `ops` = [dir, idx, ext].
/// `extra` bytes (the OIM/AIM/EIM/TIM immediate) follow the opcode.
/// `<addr` forces direct, `>addr` extended; a numeric address up to $FF is
/// direct, anything else (including every label) extended.
fn encode_memory(
    prefix: u8,
    ops: [u8; 3],
    extra: &[u8],
    operand: &str,
    env: &Env,
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let head = |opcode| {
        let mut bytes = with_prefix(prefix, opcode);
        bytes.extend_from_slice(extra);
        bytes
    };
    if is_indexed_operand(operand) {
        let mut bytes = head(ops[1]);
        let postbyte_addr = env.pc + bytes.len() as u32;
        bytes.extend(encode_indexed(operand, postbyte_addr, env, line_no)?);
        return Ok(bytes);
    }
    let (direct, value) = if let Some(rest) = operand.strip_prefix('<') {
        (true, env.eval(rest, line_no)?)
    } else if let Some(rest) = operand.strip_prefix('>') {
        (false, env.eval(rest, line_no)?)
    } else {
        let value = env.eval(operand, line_no)?;
        (is_numeric(operand) && (0..=0xFF).contains(&value), value)
    };
    if direct {
        let mut bytes = head(ops[0]);
        bytes.push(value as u8); // `<` keeps the low byte (offset in the direct page)
        Ok(bytes)
    } else {
        if !(0..=0xFFFF).contains(&value) {
            return err(line_no, format!("address out of range: {operand}"));
        }
        let mut bytes = head(ops[2]);
        bytes.extend((value as u16).to_be_bytes());
        Ok(bytes)
    }
}

/// Index register field (postbyte bits 6-5).
fn index_register(name: &str) -> Option<u8> {
    match name {
        "X" => Some(0x00),
        "Y" => Some(0x20),
        "U" => Some(0x40),
        "S" => Some(0x60),
        _ => None,
    }
}

/// Encode an indexed operand (postbyte + offset bytes). `postbyte_addr` is the
/// address of the postbyte; PC-relative offsets count from the end of the
/// instruction. `n,PCR` takes the *target address* (as the disassembler shows
/// it); a numeric target within reach uses the 8-bit form, labels always use
/// 16 bits so pass 1 and pass 2 agree on the size.
fn encode_indexed(
    operand: &str,
    postbyte_addr: u32,
    env: &Env,
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let op = operand.trim().to_uppercase();
    let indirect = op.starts_with('[') && op.ends_with(']');
    let inner = if indirect { op[1..op.len() - 1].trim() } else { op.as_str() };
    let ind = if indirect { 0x10 } else { 0x00 };
    let bad = || err(line_no, format!("unsupported indexed operand: {operand}"));

    let Some((offset, register)) = inner.split_once(',') else {
        // [address]: extended indirect
        if indirect {
            let address = env.word(inner, line_no)?;
            let mut bytes = vec![0x9F];
            bytes.extend(address.to_be_bytes());
            return Ok(bytes);
        }
        return bad();
    };
    let offset = offset.trim();
    let register = register.trim();

    // HD6309 W-relative modes: ,W  n,W  ,W++  ,--W and their indirect forms
    // (no 5/8-bit offsets and no ,W+ / ,-W).
    if matches!(register, "W" | "W++" | "--W") {
        let postbyte = match (offset.is_empty(), register, indirect) {
            (true, "W", false) => 0x8F,
            (true, "W", true) => 0x90,
            (true, "W++", false) => 0xCF,
            (true, "W++", true) => 0xD0,
            (true, "--W", false) => 0xEF,
            (true, "--W", true) => 0xF0,
            (false, "W", _) => {
                let value = env.word(offset, line_no)?;
                let mut bytes = vec![if indirect { 0xB0 } else { 0xAF }];
                bytes.extend(value.to_be_bytes());
                return Ok(bytes);
            }
            _ => return bad(),
        };
        return Ok(vec![postbyte]);
    }

    // Auto increment / decrement and zero offset: ,R  ,R+  ,R++  ,-R  ,--R
    if offset.is_empty() {
        let (name, mode) = if let Some(r) = register.strip_prefix("--") {
            (r, 0x83)
        } else if let Some(r) = register.strip_prefix('-') {
            (r, 0x82)
        } else if let Some(r) = register.strip_suffix("++") {
            (r, 0x81)
        } else if let Some(r) = register.strip_suffix('+') {
            (r, 0x80)
        } else {
            (register, 0x84)
        };
        if name == "PCR" || name == "PC" {
            return err(line_no, "PCR requires an offset: use 0,PCR");
        }
        let Some(reg) = index_register(name.trim()) else {
            return err(line_no, format!("unsupported index register: {name}"));
        };
        if indirect && (mode == 0x80 || mode == 0x82) {
            return err(line_no, format!("[,R+] and [,-R] do not exist: {operand}"));
        }
        return Ok(vec![mode | reg | ind]);
    }

    // PC relative: n,PCR (target address)
    if register == "PCR" || register == "PC" {
        let target = env.eval(offset, line_no)?;
        let short = target - (i64::from(postbyte_addr) + 2);
        if is_numeric(offset) && (-128..=127).contains(&short) {
            return Ok(vec![0x8C | ind, short as i8 as u8]);
        }
        let long = target - (i64::from(postbyte_addr) + 3);
        let long = if env.lenient { 0 } else { long };
        if !(-32768..=32767).contains(&long) {
            return err(line_no, format!("PCR offset out of range: {long}"));
        }
        let mut bytes = vec![0x8D | ind];
        bytes.extend((long as i16).to_be_bytes());
        return Ok(bytes);
    }

    let Some(reg) = index_register(register) else {
        return err(line_no, format!("unsupported index register: {register}"));
    };

    // Accumulator offsets: A,R B,R D,R and the HD6309 E,R F,R W,R
    let accumulator = match offset {
        "A" => Some(0x86),
        "B" => Some(0x85),
        "D" => Some(0x8B),
        "E" => Some(0x87),
        "F" => Some(0x8A),
        "W" => Some(0x8E),
        _ => None,
    };
    if let Some(mode) = accumulator {
        return Ok(vec![mode | reg | ind]);
    }

    // Constant offset: 5-bit (not indirect), 8-bit, 16-bit; labels use 16 bits.
    let value = env.eval(offset, line_no)?;
    let numeric = is_numeric(offset);
    if numeric && !indirect && (-16..=15).contains(&value) {
        return Ok(vec![reg | (value as u8 & 0x1F)]);
    }
    if numeric && (-128..=127).contains(&value) {
        return Ok(vec![0x88 | reg | ind, value as i8 as u8]);
    }
    if !(-32768..=0xFFFF).contains(&value) {
        return err(line_no, format!("indexed offset out of range: {operand}"));
    }
    let mut bytes = vec![0x89 | reg | ind];
    bytes.extend((value as u16).to_be_bytes());
    Ok(bytes)
}

fn stack_register_bit(reg: &str, line: usize) -> Result<u8, AsmError> {
    Ok(match reg {
        "CC" => 0x01,
        "A" => 0x02,
        "B" => 0x04,
        "D" => 0x06, // D = A|B
        "DP" => 0x08,
        "X" => 0x10,
        "Y" => 0x20,
        // bit 6 is the other stack pointer (U for PSHS/PULS, S for PSHU/PULU)
        "U" | "S" => 0x40,
        "PC" => 0x80,
        _ => return err(line, format!("unsupported stack register: {reg}")),
    })
}

fn parse_stack_postbyte(operand: &str, line: usize) -> Result<u8, AsmError> {
    let op = operand.trim().to_uppercase();
    if op.starts_with('$') {
        let value = parse_number(&op, line)?;
        if value > 0xFF {
            return err(line, format!("stack postbyte out of range: {operand}"));
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
        return err(line, "empty stack register list");
    }
    Ok(postbyte)
}

/// TFR/EXG/inter-register postbyte nibble: D0 X1 Y2 U3 S4 PC5 W6 V7 A8 B9
/// CC=A DP=B 0=C E=E F=F (HD6309; the 6809 has no W, V, 0, E, F).
fn register_code(reg: &str, line: usize) -> Result<u8, AsmError> {
    Ok(match reg.trim() {
        "D" => 0x0,
        "X" => 0x1,
        "Y" => 0x2,
        "U" => 0x3,
        "S" => 0x4,
        "PC" => 0x5,
        "W" => 0x6,
        "V" => 0x7,
        "A" => 0x8,
        "B" => 0x9,
        "CC" => 0xA,
        "DP" => 0xB,
        "0" | "00" | "Z" => 0xC,
        "E" => 0xE,
        "F" => 0xF,
        other => return err(line, format!("unsupported register: {other}")),
    })
}

/// BAND BIAND BOR BIOR BEOR BIEOR LDBT STBT `reg,sourceBit,destBit,address`
/// (Burke / MAME disassembler order; `address` is direct-page only).
/// `reg` is CC, A or B; postbyte = reg<<6 | sourceBit<<3 | destBit.
/// For BAND..LDBT the source bit is in memory and the destination bit in the
/// register (the result goes into the register); for STBT the source bit is
/// in the register and the destination bit in memory.
fn encode_bit_transfer(
    opcode: u8,
    operand: &str,
    env: &Env,
    line_no: usize,
) -> Result<Vec<u8>, AsmError> {
    let fields: Vec<&str> = operand.split(',').map(str::trim).collect();
    let [reg, src, dst, address] = fields.as_slice() else {
        return err(line_no, format!("expected reg,srcbit,dstbit,address: {operand}"));
    };
    let reg = match *reg {
        "CC" => 0u8,
        "A" => 1,
        "B" => 2,
        other => return err(line_no, format!("bit register must be CC, A or B: {other}")),
    };
    let bit = |text: &str| -> Result<u8, AsmError> {
        let value = env.eval(text, line_no)?;
        if !(0..=7).contains(&value) {
            return err(line_no, format!("bit number must be 0..7: {text}"));
        }
        Ok(value as u8)
    };
    let (src, dst) = (bit(src)?, bit(dst)?);
    let address = address.strip_prefix('<').unwrap_or(address);
    let value = env.eval(address, line_no)?;
    if !(0..=0xFF).contains(&value) && !env.lenient {
        return err(line_no, format!("bit operations use direct addressing: {address}"));
    }
    Ok(vec![0x11, opcode, (reg << 6) | (src << 3) | dst, value as u8])
}

/// TFM r0+,r1+ ($1138)  r0-,r1- ($1139)  r0+,r1 ($113A)  r0,r1+ ($113B);
/// registers D, X, Y, U, S.
fn encode_tfm(fixed: Option<u8>, operand: &str, line_no: usize) -> Result<Vec<u8>, AsmError> {
    let Some((src, dst)) = operand.split_once(',') else {
        return err(line_no, format!("TFM operand must be src,dst: {operand}"));
    };
    let split = |text: &str| -> Result<(u8, char), AsmError> {
        let text = text.trim();
        let (name, mark) = match text.chars().last() {
            Some(c @ ('+' | '-')) => (&text[..text.len() - 1], c),
            _ => (text, ' '),
        };
        let code = match name {
            "D" => 0,
            "X" => 1,
            "Y" => 2,
            "U" => 3,
            "S" => 4,
            other => return err(line_no, format!("TFM registers are D, X, Y, U, S: {other}")),
        };
        Ok((code, mark))
    };
    let (src, src_mark) = split(src)?;
    let (dst, dst_mark) = split(dst)?;
    let form = match (src_mark, dst_mark) {
        ('+', '+') => Some(0x38),
        ('-', '-') => Some(0x39),
        ('+', ' ') => Some(0x3A),
        (' ', '+') => Some(0x3B),
        (' ', ' ') => None,
        _ => return err(line_no, format!("TFM forms: r0+,r1+ r0-,r1- r0+,r1 r0,r1+: {operand}")),
    };
    let opcode = match (form, fixed) {
        (Some(form), Some(alias)) if form != alias => {
            return err(line_no, format!("operand does not match the TFM variant: {operand}"));
        }
        (Some(opcode), _) | (None, Some(opcode)) => opcode,
        (None, None) => {
            return err(line_no, format!("TFM needs r0+,r1+ / r0-,r1- / r0+,r1 / r0,r1+: {operand}"));
        }
    };
    Ok(vec![0x11, opcode, (src << 4) | dst])
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
        // MULD always has an operand ($118F #, $119F <, $11AF idx, $11BF ext);
        // $103E is not an HD6309 opcode.
        let bytes = asm_bytes("LDW #$000A\nMULD #$000A");
        assert_eq!(bytes, vec![0x10, 0x86, 0x00, 0x0A, 0x11, 0x8F, 0x00, 0x0A]);
        let error = assemble("MULD").unwrap_err();
        assert!(error.message.contains("missing operand"), "{error}");
    }

    #[test]
    fn assemble_6309_leax() {
        let bytes = asm_bytes("LEAX 5,X");
        assert_eq!(bytes, vec![0x30, 0x05]); // 5,X = 5-bit constant 5 = 0x05
    }

    #[test]
    fn assemble_6309_divd_tfm() {
        // DIVD takes an 8-bit immediate.
        let bytes = asm_bytes("DIVD #$0002\nTFM+ X+,Y+\nTFM X+,Y+");
        assert_eq!(bytes, vec![0x11, 0x8D, 0x02, 0x11, 0x38, 0x12, 0x11, 0x38, 0x12]);
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

    #[test]
    fn line_map_records_source_line_addresses() {
        let src = "\
; comment line (1)
ORG $0100             ; line 2
start LDA #$42        ; line 3
NOP                   ; line 4
COUNT EQU 5           ; line 5
BRA start             ; line 6
END"; // line 7
        let prog = assemble(src).unwrap();
        // line 2 (ORG) and 5 (EQU) produce no code -> absent
        assert!(!prog.line_map.contains_key(&2));
        assert!(!prog.line_map.contains_key(&5));
        // code-producing lines map to their emitted address
        assert_eq!(prog.line_map.get(&3), Some(&0x0100)); // LDA #$42
        assert_eq!(prog.line_map.get(&4), Some(&0x0102)); // NOP
        assert_eq!(prog.line_map.get(&6), Some(&0x0103)); // BRA start
    }

    #[test]
    fn line_map_rmb_marks_reserved_region() {
        let src = "ORG $0200\nBUF RMB 4\nLDA #$00\nEND";
        let prog = assemble(src).unwrap();
        assert_eq!(prog.line_map.get(&2), Some(&0x0200)); // RMB reserves here
        assert_eq!(prog.line_map.get(&3), Some(&0x0204)); // LDA after the buffer
    }

    // ── HD6309 encodings (MAME hd6309.lst) ───────────────────────────────

    fn check(cases: &[(&str, &[u8])]) {
        for (source, expected) in cases {
            let bytes = assemble(&format!("ORG $0100\n {source}\nEND"))
                .unwrap_or_else(|e| panic!("{source}: {e}"))
                .bytes;
            assert_eq!(bytes, *expected, "{source}");
        }
    }

    fn check_error(sources: &[&str]) {
        for source in sources {
            assert!(assemble(&format!(" {source}")).is_err(), "{source} must not assemble");
        }
    }

    #[test]
    fn aim_family_encodes_immediate_first_on_page_1() {
        check(&[
            ("AIM #$F0,<$20", &[0x02, 0xF0, 0x20]),
            ("AIM #$F0,$20", &[0x02, 0xF0, 0x20]),
            ("OIM #$0F,$1234", &[0x71, 0x0F, 0x12, 0x34]),
            ("OIM #1,>$0020", &[0x71, 0x01, 0x00, 0x20]),
            ("EIM #$FF,5,X", &[0x65, 0xFF, 0x05]),
            ("AIM #$01,,X", &[0x62, 0x01, 0x84]),
            ("TIM #$80,$20", &[0x0B, 0x80, 0x20]),
            ("TIM #$01,[,Y]", &[0x6B, 0x01, 0xB4]),
            ("TIM #%1010,1000,U", &[0x6B, 0x0A, 0xC9, 0x03, 0xE8]),
        ]);
        check_error(&["AIM $20,#$F0", "AIM #$F0", "OIM #$100,$20"]);
    }

    #[test]
    fn inter_register_postbyte_is_source_then_destination() {
        check(&[
            ("ADDR W,D", &[0x10, 0x30, 0x60]),
            ("ADCR CC,A", &[0x10, 0x31, 0xA8]),
            ("SUBR X,Y", &[0x10, 0x32, 0x12]),
            ("SBCR PC,X", &[0x10, 0x33, 0x51]),
            ("ANDR V,W", &[0x10, 0x34, 0x76]),
            ("ORR DP,B", &[0x10, 0x35, 0xB9]),
            ("EORR E,F", &[0x10, 0x36, 0xEF]),
            ("CMPR 0,D", &[0x10, 0x37, 0xC0]),
            ("ADDR A,B", &[0x10, 0x30, 0x89]),
            ("TFR 0,D", &[0x1F, 0xC0]),
            ("TFR W,Y", &[0x1F, 0x62]),
            ("EXG E,F", &[0x1E, 0xEF]),
            ("TFR V,X", &[0x1F, 0x71]),
        ]);
        check_error(&["ADDR W", "ADDR Q,D", "TFR X,MD"]);
    }

    #[test]
    fn divd_divq_muld_forms() {
        check(&[
            ("DIVD #2", &[0x11, 0x8D, 0x02]),
            ("DIVD #-2", &[0x11, 0x8D, 0xFE]),
            ("DIVD <$20", &[0x11, 0x9D, 0x20]),
            ("DIVD ,X", &[0x11, 0xAD, 0x84]),
            ("DIVD $1234", &[0x11, 0xBD, 0x12, 0x34]),
            ("DIVQ #$0100", &[0x11, 0x8E, 0x01, 0x00]),
            ("DIVQ $20", &[0x11, 0x9E, 0x20]),
            ("MULD #$000A", &[0x11, 0x8F, 0x00, 0x0A]),
            ("MULD ,Y++", &[0x11, 0xAF, 0xA1]),
            ("MULD >$0020", &[0x11, 0xBF, 0x00, 0x20]),
        ]);
        check_error(&["DIVD #$1234", "DIVD", "MULD"]);
    }

    #[test]
    fn bit_transfer_syntax_is_reg_source_destination_address() {
        check(&[
            ("BOR A,5,0,<$20", &[0x11, 0x32, 0x68, 0x20]),
            ("BAND CC,0,0,$20", &[0x11, 0x30, 0x00, 0x20]),
            ("BIAND B,1,2,$20", &[0x11, 0x31, 0x8A, 0x20]),
            ("BIOR CC,7,0,$FF", &[0x11, 0x33, 0x38, 0xFF]),
            ("BEOR A,0,7,$00", &[0x11, 0x34, 0x47, 0x00]),
            ("BIEOR B,3,3,$10", &[0x11, 0x35, 0x9B, 0x10]),
            ("LDBT A,1,7,32", &[0x11, 0x36, 0x4F, 0x20]),
            ("STBT B,7,2,$20", &[0x11, 0x37, 0xBA, 0x20]),
        ]);
        check_error(&["BAND X,0,0,$20", "BAND A,8,0,$20", "BAND A,0,0,$1234", "BAND A 1 7 $00"]);
    }

    #[test]
    fn hd6309_inherent_opcodes() {
        check(&[
            ("SEXW", &[0x14]),
            ("PSHSW", &[0x10, 0x38]),
            ("PULSW", &[0x10, 0x39]),
            ("PSHUW", &[0x10, 0x3A]),
            ("PULUW", &[0x10, 0x3B]),
            ("NEGD", &[0x10, 0x40]),
            ("COMD", &[0x10, 0x43]),
            ("LSRD", &[0x10, 0x44]),
            ("RORD", &[0x10, 0x46]),
            ("ASRD", &[0x10, 0x47]),
            ("ASLD", &[0x10, 0x48]),
            ("LSLD", &[0x10, 0x48]),
            ("ROLD", &[0x10, 0x49]),
            ("DECD", &[0x10, 0x4A]),
            ("INCD", &[0x10, 0x4C]),
            ("TSTD", &[0x10, 0x4D]),
            ("CLRD", &[0x10, 0x4F]),
            ("COMW", &[0x10, 0x53]),
            ("LSRW", &[0x10, 0x54]),
            ("RORW", &[0x10, 0x56]),
            ("ROLW", &[0x10, 0x59]),
            ("DECW", &[0x10, 0x5A]),
            ("INCW", &[0x10, 0x5C]),
            ("TSTW", &[0x10, 0x5D]),
            ("CLRW", &[0x10, 0x5F]),
            ("COME", &[0x11, 0x43]),
            ("DECE", &[0x11, 0x4A]),
            ("INCE", &[0x11, 0x4C]),
            ("TSTE", &[0x11, 0x4D]),
            ("CLRE", &[0x11, 0x4F]),
            ("COMF", &[0x11, 0x53]),
            ("DECF", &[0x11, 0x5A]),
            ("INCF", &[0x11, 0x5C]),
            ("TSTF", &[0x11, 0x5D]),
            ("CLRF", &[0x11, 0x5F]),
            ("LDMD #$01", &[0x11, 0x3D, 0x01]),
            ("BITMD #$80", &[0x11, 0x3C, 0x80]),
        ]);
        check_error(&["LDMD $01"]);
    }

    #[test]
    fn mame_only_w_opcodes_are_rejected() {
        // MAME's NEGW/ASRW/ASLW trap on a real HD6309.
        for mnemonic in ["NEGW", "ASRW", "ASLW", "LSLW"] {
            for source in [format!(" {mnemonic}"), format!("LOOP {mnemonic}")] {
                let error = assemble(&source).unwrap_err();
                assert!(error.message.contains("does not exist on the HD6309"), "{error}");
            }
        }
    }

    #[test]
    fn hd6309_memory_forms_on_pages_2_and_3() {
        check(&[
            ("SUBW #$1234", &[0x10, 0x80, 0x12, 0x34]),
            ("CMPW <$20", &[0x10, 0x91, 0x20]),
            ("SBCD ,X", &[0x10, 0xA2, 0x84]),
            ("ANDD $1234", &[0x10, 0xB4, 0x12, 0x34]),
            ("BITD #1", &[0x10, 0x85, 0x00, 0x01]),
            ("LDW 5,Y", &[0x10, 0xA6, 0x25]),
            ("STW <$20", &[0x10, 0x97, 0x20]),
            ("STW $1234", &[0x10, 0xB7, 0x12, 0x34]),
            ("EORD >$0012", &[0x10, 0xB8, 0x00, 0x12]),
            ("ADCD #$FFFF", &[0x10, 0x89, 0xFF, 0xFF]),
            ("ORD ,--X", &[0x10, 0xAA, 0x83]),
            ("ADDW [$1234]", &[0x10, 0xAB, 0x9F, 0x12, 0x34]),
            ("LDQ #$12345678", &[0xCD, 0x12, 0x34, 0x56, 0x78]),
            ("LDQ <$20", &[0x10, 0xDC, 0x20]),
            ("LDQ ,X", &[0x10, 0xEC, 0x84]),
            ("LDQ $1234", &[0x10, 0xFC, 0x12, 0x34]),
            ("STQ <$20", &[0x10, 0xDD, 0x20]),
            ("STQ ,X", &[0x10, 0xED, 0x84]),
            ("STQ $1234", &[0x10, 0xFD, 0x12, 0x34]),
            ("SUBE #1", &[0x11, 0x80, 0x01]),
            ("CMPE <$20", &[0x11, 0x91, 0x20]),
            ("LDE ,X", &[0x11, 0xA6, 0x84]),
            ("STE $1234", &[0x11, 0xB7, 0x12, 0x34]),
            ("ADDE #$FF", &[0x11, 0x8B, 0xFF]),
            ("SUBF #1", &[0x11, 0xC0, 0x01]),
            ("CMPF <$20", &[0x11, 0xD1, 0x20]),
            ("LDF ,X", &[0x11, 0xE6, 0x84]),
            ("STF $1234", &[0x11, 0xF7, 0x12, 0x34]),
            ("ADDF #1", &[0x11, 0xCB, 0x01]),
            ("CMPU #$1234", &[0x11, 0x83, 0x12, 0x34]),
            ("CMPS <$20", &[0x11, 0x9C, 0x20]),
        ]);
        check_error(&["STW #1", "STQ #1", "STE #1"]);
    }

    #[test]
    fn tfm_opcode_follows_the_operand_form() {
        check(&[
            ("TFM X+,Y+", &[0x11, 0x38, 0x12]),
            ("TFM X-,Y-", &[0x11, 0x39, 0x12]),
            ("TFM X+,Y", &[0x11, 0x3A, 0x12]),
            ("TFM X,Y+", &[0x11, 0x3B, 0x12]),
            ("TFM D+,S+", &[0x11, 0x38, 0x04]),
            ("TFM+ X+,Y+", &[0x11, 0x38, 0x12]),
            ("TFM+R U+,X", &[0x11, 0x3A, 0x31]),
            ("TFM+W X,U", &[0x11, 0x3B, 0x13]),
        ]);
        check_error(&["TFM PC+,X+", "TFM W+,X+", "TFM X,Y", "TFM- X+,Y+", "TFM X-,Y+"]);
    }

    #[test]
    fn hd6309_indexed_modes() {
        check(&[
            ("LDA ,W", &[0xA6, 0x8F]),
            ("LDA [,W]", &[0xA6, 0x90]),
            ("LDA 16,W", &[0xA6, 0xAF, 0x00, 0x10]),
            ("LDA [-2,W]", &[0xA6, 0xB0, 0xFF, 0xFE]),
            ("LDA ,W++", &[0xA6, 0xCF]),
            ("LDA [,W++]", &[0xA6, 0xD0]),
            ("LDA ,--W", &[0xA6, 0xEF]),
            ("LDA [,--W]", &[0xA6, 0xF0]),
            ("LDA E,X", &[0xA6, 0x87]),
            ("LDA F,Y", &[0xA6, 0xAA]),
            ("LDA W,U", &[0xA6, 0xCE]),
            ("LDA [W,S]", &[0xA6, 0xFE]),
            ("LEAX D,X", &[0x30, 0x8B]),
        ]);
        check_error(&["LDA ,W+", "LDA ,-W", "LDA [,X+]", "LDA 5,Q"]);
    }

    #[test]
    fn pcr_offsets_count_prefix_and_immediate_bytes() {
        check(&[
            // 10 A6 8C oo: PC after the instruction = $0104
            ("LDW $0110,PCR", &[0x10, 0xA6, 0x8C, 0x0C]),
            // 62 01 8C oo: PC after = $0104
            ("AIM #$01,$0110,PCR", &[0x62, 0x01, 0x8C, 0x0C]),
            ("LEAX $0100,PCR", &[0x30, 0x8C, 0xFD]),
            ("LDA $1000,PCR", &[0xA6, 0x8D, 0x0E, 0xFC]),
        ]);
        // Labels always use the 16-bit form so both passes agree on the size.
        let bytes = asm_bytes("ORG $0100\nLEAX MSG,PCR\nNOP\nMSG FCB 1\nEND");
        assert_eq!(bytes, vec![0x30, 0x8D, 0x00, 0x01, 0x12, 0x01]);
    }

    #[test]
    fn labels_and_expressions_in_operands() {
        let src = "\
COUNT   EQU 16
        ORG $0100
        LDA #COUNT
        LDX #TABLE+2
        JSR SUB
        BRA *
SUB     RTS
HERE    EQU *
TABLE   FCB 1,2
        FDB HERE
        LDA #-1
        END";
        let prog = assemble(src).unwrap();
        assert_eq!(
            prog.bytes,
            vec![
                0x86, 0x10, // LDA #16
                0x8E, 0x01, 0x0D, // LDX #$010D (TABLE+2, forward reference)
                0xBD, 0x01, 0x0A, // JSR $010A (forward label: extended)
                0x20, 0xFE, // BRA *
                0x39, // SUB RTS
                0x01, 0x02, // TABLE FCB 1,2
                0x01, 0x0B, // FDB HERE ($010B)
                0x86, 0xFF, // LDA #-1
            ]
        );
    }

    #[test]
    fn complete_mc6809_mnemonics() {
        check(&[
            ("ROLA", &[0x49]),
            ("INCB", &[0x5C]),
            ("ASLB", &[0x58]),
            ("LSLA", &[0x48]),
            ("NEGA", &[0x40]),
            ("TSTB", &[0x5D]),
            ("DAA", &[0x19]),
            ("SWI2", &[0x10, 0x3F]),
            ("SWI3", &[0x11, 0x3F]),
            ("CLR $0400", &[0x7F, 0x04, 0x00]),
            ("CLR <$20", &[0x0F, 0x20]),
            ("INC ,X", &[0x6C, 0x84]),
            ("TST $1234", &[0x7D, 0x12, 0x34]),
            ("NEG 5,X", &[0x60, 0x05]),
            ("ADCA #1", &[0x89, 0x01]),
            ("SBCB <$20", &[0xD2, 0x20]),
            ("BITA ,X", &[0xA5, 0x84]),
            ("CMPD #1", &[0x10, 0x83, 0x00, 0x01]),
            ("CMPY $1234", &[0x10, 0xBC, 0x12, 0x34]),
            ("CMPU ,X", &[0x11, 0xA3, 0x84]),
            ("STS <$20", &[0x10, 0xDF, 0x20]),
            ("LDY $1234", &[0x10, 0xBE, 0x12, 0x34]),
            ("LDS $20", &[0x10, 0xDE, 0x20]),
            ("JMP ,X", &[0x6E, 0x84]),
            ("JSR $20", &[0x9D, 0x20]),
            ("BHI $0100", &[0x22, 0xFE]),
            ("BLE $0100", &[0x2F, 0xFE]),
            ("BHS $0100", &[0x24, 0xFE]),
            ("LBLO $0100", &[0x10, 0x25, 0xFF, 0xFC]),
        ]);
        // Bare inherent lines used to be taken for labels (pia example).
        let bytes = asm_bytes("        INCB\n        ROLA\nNOWRAP\n        NOP\n");
        assert_eq!(bytes, vec![0x5C, 0x49, 0x12]);
    }

    #[test]
    fn labels_named_like_mnemonics() {
        // Column 1: label CLR; indented: BNE with the operand CLR (coco2 ROM).
        let bytes = asm_bytes("        ORG $0100\nCLR     STA ,X\n        BNE     CLR\n        CLR     <$20\n");
        assert_eq!(bytes, vec![0xA7, 0x84, 0x26, 0xFC, 0x0F, 0x20]);
    }

    #[test]
    fn hd6309_disassembly_round_trip() {
        use m6809_core::CpuVariant;
        let lines = [
            "AIM #$F0,<$20",
            "OIM #$0F,$1234",
            "EIM #$FF,5,X",
            "TIM #$80,[,Y]",
            "ADDR W,D",
            "CMPR 0,X",
            "SUBR A,B",
            "DIVD #$02",
            "DIVD <$20",
            "DIVQ #$0100",
            "MULD ,X",
            "MULD $1234",
            "BOR A,5,0,<$20",
            "STBT CC,0,7,<$10",
            "TFM X+,Y+",
            "TFM X-,Y-",
            "TFM D+,U",
            "TFM X,S+",
            "LDQ #$12345678",
            "STQ $1234",
            "LDW 16,W",
            "SUBW ,X",
            "LDMD #$01",
            "BITMD #$80",
            "SEXW",
            "INCW",
            "TSTD",
            "TFR 0,D",
            "EXG W,D",
            "LDE #$12",
            "STF <$20",
            "ADDE ,X",
            "CMPF $1234",
            "PSHSW",
            "LDA E,X",
            "LDA [,W++]",
            "LDA ,W",
        ];
        for line in lines {
            let bytes = asm_bytes(&format!("ORG $0100\n {line}\nEND"));
            let insns = disassemble_with_variant(&bytes, 0x0100, CpuVariant::Hd6309);
            assert_eq!(insns.len(), 1, "{line}: {insns:?}");
            assert_eq!(insns[0].text, line);
            assert_eq!(insns[0].bytes, bytes, "{line}");
        }
    }

    #[test]
    fn disassembler_handles_every_opcode() {
        use m6809_core::CpuVariant;
        for variant in [CpuVariant::Mc6809, CpuVariant::Hd6309] {
            for prefix in [None, Some(0x10u8), Some(0x11)] {
                for opcode in 0..=0xFFu8 {
                    let mut bytes: Vec<u8> = prefix.into_iter().collect();
                    bytes.extend([opcode, 0x9F, 0x12, 0x34, 0x56]);
                    let insns = disassemble_with_variant(&bytes, 0x0100, variant);
                    assert!(!insns.is_empty(), "{bytes:02X?}");
                    let covered: usize = insns.iter().map(|i| i.bytes.len()).sum();
                    assert!(covered <= bytes.len() + 4, "{bytes:02X?}");
                }
            }
        }
    }

    #[test]
    fn disassembly_is_not_confused_by_stores_into_the_code() {
        // STA $0103 writes over the following NOP while being decoded.
        let insns = disassemble(&[0xB7, 0x01, 0x03, 0x12], 0x0100);
        let texts: Vec<&str> = insns.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, vec!["STA $0103", "NOP"]);
    }

    /// Source of an example in src/lib/examples.ts (as the frontend loads it).
    fn example_sources() -> Vec<(String, String)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/examples.ts");
        let text = std::fs::read_to_string(path).expect("src/lib/examples.ts");
        text.split("id: \"")
            .skip(1)
            .map(|chunk| {
                let id = chunk.split('"').next().unwrap_or_default().to_string();
                let source = chunk
                    .split("source: `")
                    .nth(1)
                    .and_then(|s| s.split('`').next())
                    .unwrap_or_default()
                    .to_string();
                (id, source)
            })
            .collect()
    }

    #[test]
    fn all_frontend_examples_assemble() {
        let examples = example_sources();
        assert!(examples.len() >= 10);
        for (id, source) in examples {
            assert!(!source.is_empty(), "{id}");
            if let Err(error) = assemble(&source) {
                panic!("example {id}: {error}");
            }
        }
    }

    #[test]
    fn hd6309_example_runs_to_its_documented_results() {
        use m6809_core::CpuVariant;
        let (_, source) = example_sources()
            .into_iter()
            .find(|(id, _)| id == "hd6309")
            .expect("hd6309 example");
        let program = assemble(&source).expect("hd6309 example assembles");
        let mut mem = Memory::new();
        mem.load_binary(program.origin, &program.bytes).unwrap();
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.pc = 0x0100;
        let mut steps = 0;
        loop {
            let step = cpu.step(&mut mem);
            assert!(step.trap.is_none(), "{step:?}");
            steps += 1;
            assert!(steps < 10_000, "example must reach its idle loop");
            // IDLE BRA IDLE (a pending TFM also keeps PC, one byte per step)
            if step.pc_after == step.pc_before && cpu.tfm_pending.is_none() {
                break;
            }
        }
        assert_eq!(cpu.mode_reg & 0x01, 0x01, "native mode");
        let word = |addr: u16| mem.read16(addr);
        // TFM copy of $10..$1F
        assert_eq!(mem.export_range(0x0700, 16).unwrap(), (0x10..0x20).collect::<Vec<u8>>());
        assert_eq!((mem.read8(0x0800), mem.read8(0x0801)), (0x10, 0x1F));
        // MULD: -300 * 1000 = -300000 = $FFFB6C20
        assert_eq!((word(0x0802), word(0x0804)), (0xFFFB, 0x6C20));
        // DIVD: -1000 / 9 = -111 rem -1 -> A = remainder, B = quotient
        assert_eq!((mem.read8(0x0806), mem.read8(0x0807)), (0xFF, 0x91));
        // DIVQ: 100000 / 300 = 333 rem 100 -> W = quotient, D = remainder
        assert_eq!((word(0x0808), word(0x080A)), (333, 100));
        // ADDR W,D: 1000 + 234
        assert_eq!(word(0x080C), 1234);
        // AIM/OIM/EIM: $FF -> $F0 -> $F5 -> $5A
        assert_eq!(mem.read8(0x0810), 0x5A);
        // LDBT/STBT: bit 6 of $0810 -> bit 7 of $0811
        assert_eq!(mem.read8(0x0811), 0x80);
        assert_eq!(cpu.w, 0, "CLRW");
    }
}
