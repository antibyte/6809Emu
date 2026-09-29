//! HD6309 extensions of the CPU core: the additional page-1/2/3 opcodes,
//! native-mode timing, the TFM block transfer and the $FFF0 error trap.
//!
//! The reference is the real chip: MAME (`src/devices/cpu/m6809/hd6309.cpp`,
//! `hd6309.lst`, `base6x09.lst`) as the baseline, corrected wherever hardware
//! measurements differ — Darren Atkinson's "6809/6309 Programming Reference"
//! and hoglet67's 6809Decoder, whose flag and cycle model is checked against
//! logic-analyser captures and exhaustive/random tests on real HD6309s.
//! Cycle counts follow Burke's "The 6309 Book" tables as verified there.

use crate::addressing::{decode_index_mode, index_extra_cycles, IndexMode};
use crate::alu::{
    adc16, add16, add8, and16, asl16, asr16, bit16, cmp16, com16, dec16, eor16, inc16, lsr16,
    neg16, or16, rol16, ror16, sbc16, sub16, sub8, tst16,
};
use crate::cpu::{Cpu, Reg16, StepCtx, TfmPending};
use crate::flags::Flags;
use crate::memory::Memory;
use crate::types::{CpuVariant, StepResult, Trap};

/// Bytes moved per `step()` during TFM. One byte per step, so that an
/// interrupt is recognised after every transferred byte, as on the chip.
const TFM_CHUNK_SIZE: u16 = 1;

/// MD bit 0: native mode.
pub(crate) const MD_NATIVE: u8 = 0x01;
/// MD bit 1: FIRQ stacks the entire state.
pub(crate) const MD_FIRQ_ENTIRE: u8 = 0x02;
/// MD bit 6: an illegal instruction was trapped.
pub(crate) const MD_ILLEGAL: u8 = 0x40;
/// MD bit 7: a division by zero was trapped.
pub(crate) const MD_DIV_ZERO: u8 = 0x80;
/// LDMD can only write NM and FM; bits 6/7 are status bits (cleared by BITMD).
const MD_WRITABLE: u8 = MD_NATIVE | MD_FIRQ_ENTIRE;

/// Trap reported in the `StepResult` when a division by zero vectors through $FFF0.
const DIV_ZERO_TRAP: Trap = Trap::DivideByZero;

// Emulation-mode cycle counts by addressing mode [immediate, direct, indexed
// (+ postbyte extras), extended] (Burke / hoglet67). `hd6309_native_cycles`
// converts them for native mode.
/// SUBW CMPW SBCD ANDD BITD EORD ADCD ORD ADDW.
const ALU16_CYCLES: [u32; 4] = [5, 7, 7, 8];
/// LDW, STW.
const LDW_CYCLES: [u32; 4] = [4, 6, 6, 7];
/// LDQ, STQ.
const LDQ_CYCLES: [u32; 4] = [5, 8, 8, 9];
/// SUBE/F CMPE/F LDE/F ADDE/F, STE/F.
const ALU8_EF_CYCLES: [u32; 4] = [3, 5, 5, 6];
const MULD_CYCLES: [u32; 4] = [28, 30, 30, 31];
/// DIVD/DIVQ: before the data-dependent adjustment of `exec_divd`/`exec_divq`.
const DIVD_CYCLES: [u32; 4] = [25, 27, 27, 28];
const DIVQ_CYCLES: [u32; 4] = [34, 36, 36, 37];

/// Operand addressing mode of a memory/immediate instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Imm = 0,
    Dir = 1,
    Idx = 2,
    Ext = 3,
}

impl Mode {
    /// Standard row layout: $8x/$Cx immediate, $9x/$Dx direct, $Ax/$Ex indexed,
    /// $Bx/$Fx extended.
    fn of(opcode: u8) -> Mode {
        match (opcode >> 4) & 0x03 {
            0 => Mode::Imm,
            1 => Mode::Dir,
            2 => Mode::Idx,
            _ => Mode::Ext,
        }
    }

    /// Page-1 memory layout: $0x direct, $6x indexed, $7x extended.
    fn of_memory_op(opcode: u8) -> Option<Mode> {
        match opcode >> 4 {
            0x0 => Some(Mode::Dir),
            0x6 => Some(Mode::Idx),
            0x7 => Some(Mode::Ext),
            _ => None,
        }
    }
}

/// OIM/AIM/EIM/TIM operation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LogicOp {
    Or,
    And,
    Eor,
    Test,
}

/// Register names of the TFR/EXG/inter-register postbyte nibbles.
pub(crate) fn hd6309_reg_name(code: u8) -> &'static str {
    match code & 0x0F {
        0x0 => "D",
        0x1 => "X",
        0x2 => "Y",
        0x3 => "U",
        0x4 => "S",
        0x5 => "PC",
        0x6 => "W",
        0x7 => "V",
        0x8 => "A",
        0x9 => "B",
        0xA => "CC",
        0xB => "DP",
        0xC | 0xD => "0",
        0xE => "E",
        _ => "F",
    }
}

fn tfm_reg_name(code: u8) -> &'static str {
    match code {
        0 => "D",
        1 => "X",
        2 => "Y",
        3 => "U",
        4 => "S",
        _ => "?",
    }
}

/// `r0+,r1+` ($38), `r0-,r1-` ($39), `r0+,r1` ($3A), `r0,r1+` ($3B).
fn tfm_operands(opcode: u8, postbyte: u8) -> String {
    let src = tfm_reg_name(postbyte >> 4);
    let dst = tfm_reg_name(postbyte & 0x0F);
    match opcode & 0x03 {
        0 => format!("{src}+,{dst}+"),
        1 => format!("{src}-,{dst}-"),
        2 => format!("{src}+,{dst}"),
        _ => format!("{src},{dst}+"),
    }
}

/// Pointer increments (source, destination) of the four TFM forms.
fn tfm_steps(opcode: u8) -> (u16, u16) {
    match opcode & 0x03 {
        0 => (1, 1),
        1 => (0xFFFF, 0xFFFF),
        2 => (1, 0),
        _ => (0, 1),
    }
}

/// Bit-manipulation register field (postbyte bits 7-6).
fn bit_reg_name(code: u8) -> &'static str {
    match code {
        0 => "CC",
        1 => "A",
        2 => "B",
        _ => "?",
    }
}

/// Emulation-mode indexed extra cycles exactly as `Cpu::addr_indexed` charges
/// them (`addressing::index_extra_cycles`), so native mode can replace them.
fn emulation_index_extra(postbyte: u8) -> i32 {
    let indirect = postbyte & 0x90 == 0x90;
    let w_mode = if postbyte & 0x80 != 0 {
        match postbyte & 0x7F {
            0x0F | 0x10 => Some(IndexMode::WBase),
            0x2F | 0x30 => Some(IndexMode::WOff16),
            0x4F | 0x50 => Some(IndexMode::WPostInc2),
            0x6F | 0x70 => Some(IndexMode::WPreDec2),
            _ => None,
        }
    } else {
        None
    };
    let extra = match w_mode {
        Some(mode) => index_extra_cycles(mode, indirect),
        None => {
            let (mode, indirect, _, _) = decode_index_mode(postbyte);
            index_extra_cycles(mode, indirect)
        }
    };
    i32::from(extra)
}

/// Native-mode indexed extra cycles ("Addendum to The 6309 Book", hoglet67).
fn native_index_extra(postbyte: u8) -> i32 {
    if postbyte & 0x80 == 0 {
        return 1; // n5,R
    }
    let indirect = postbyte & 0x10 != 0;
    let w_mode = match postbyte {
        0x8F | 0x90 => Some(0), // ,W   [,W]
        0xAF | 0xB0 => Some(2), // n16,W   [n16,W]
        0xCF | 0xD0 => Some(1), // ,W++   [,W++]
        0xEF | 0xF0 => Some(1), // ,--W   [,--W]
        _ => None,
    };
    if let Some(extra) = w_mode {
        return if indirect { extra + 3 } else { extra };
    }
    let direct = match postbyte & 0x0F {
        0x0 | 0x2 => 1,             // ,R+  ,-R
        0x1 | 0x3 => 2,             // ,R++  ,--R
        0x4 => 0,                   // ,R
        0x5 | 0x6 | 0x7 | 0xA => 1, // B,R  A,R  E,R  F,R
        0x8 | 0xC => 1,             // n8,R  n8,PCR
        0x9 | 0xD => 3,             // n16,R  n16,PCR
        0xB => 2,                   // D,R
        _ => 1,                     // W,R
    };
    match (indirect, postbyte & 0x0F) {
        (false, _) => direct,
        (true, 0x0F) => 4, // [n16]
        (true, _) => direct + 3,
    }
}

/// Cycles saved in native mode (emulation count minus native count) for every
/// documented HD6309 opcode (Burke's tables as verified by hoglet67).
/// `emulation` is the emulation-mode count of the executed instruction. RTI
/// and CWAI are adjusted in `cpu.rs`; SWI/SWI2/SWI3 are 2 cycles slower (W).
fn native_saving(page: u8, opcode: u8, emulation: u32) -> i32 {
    let row = opcode >> 4;
    let col = opcode & 0x0F;
    match page {
        1 => match opcode {
            0x00..=0x0F => match col {
                0x1 | 0x2 | 0x5 | 0xB => 0, // OIM AIM EIM TIM
                0xD => 2,                   // TST
                _ => 1,                     // RMW, JMP
            },
            0x12 | 0x13 | 0x16 | 0x19 | 0x1D => 1, // NOP SYNC LBRA DAA SEX
            0x17 => 2,                             // LBSR
            0x1E => 3,                             // EXG 8 -> 5
            0x1F => 2,                             // TFR 6 -> 4
            0x34..=0x37 | 0x39 | 0x3D => 1,        // PSHS..PULU RTS MUL
            0x3A => 2,                             // ABX
            0x3F => -2,                            // SWI stacks W
            0x40..=0x5F => 1,                      // inherent A/B
            0x60..=0x6F => i32::from(col == 0xD),  // TST ,R
            0x70..=0x7F => match col {
                0x1 | 0x2 | 0x5 | 0xB => 0,
                0xD => 2,
                _ => 1,
            },
            0x80..=0xFF => {
                // 16-bit arithmetic: SUBD/CMPX ($x3/$xC, rows 8-B), ADDD ($x3, rows C-F)
                let arith16 = col == 0x3 || (row < 0xC && col == 0xC);
                match (row & 0x03, arith16) {
                    (0, true) => 1,  // immediate
                    (0, false) => i32::from(opcode == 0x8D), // BSR
                    (1, true) => 2,  // direct
                    (1, false) => 1,
                    (2, true) => 1,  // indexed
                    (2, false) => i32::from(opcode == 0xAD), // JSR ,R
                    (_, true) => 2,  // extended
                    (_, false) => 1,
                }
            }
            _ => 0,
        },
        2 => match opcode {
            // LBcc: taken branches lose their extra cycle
            0x21..=0x2F => emulation.saturating_sub(5) as i32,
            0x3F => -2,                    // SWI2
            0x40..=0x5F => 1,              // D/W inherent
            0x80..=0xBF => {
                let load_store = matches!(col, 0x6 | 0x7 | 0xE | 0xF); // LDW STW LDY STY
                match (row & 0x03, load_store) {
                    (0, true) => 0,
                    (0, false) => 1,
                    (1, true) => 1,
                    (1, false) => 2,
                    (2, true) => 0,
                    (2, false) => 1,
                    (_, true) => 1,
                    (_, false) => 2,
                }
            }
            0xDC..=0xDF | 0xFC..=0xFF => 1, // LDQ/STQ/LDS/STS direct, extended
            _ => 0,
        },
        _ => match opcode {
            0x30..=0x37 => 1,     // bit operations 7 -> 6, STBT 8 -> 7
            0x3F => -2,           // SWI3
            0x40..=0x5F => 1,     // E/F inherent
            0x80..=0xFF => {
                let wide = matches!(col, 0x3 | 0xC); // CMPU CMPS
                match (row & 0x03, wide) {
                    (0, true) => 1,
                    (0, false) => 0,
                    (1, true) => 2,
                    (1, false) => 1,
                    (2, true) => 1,
                    (2, false) => 0,
                    (_, true) => 2,
                    (_, false) => 1,
                }
            }
            _ => 0,
        },
    }
}

impl Cpu {
    pub fn is_hd6309(&self) -> bool {
        self.variant == CpuVariant::Hd6309
    }

    /// HD6309 native-mode timing: called by `Cpu::step` after every executed
    /// instruction while MD bit 0 is set. `ctx.cycles` holds the emulation-mode
    /// count; this replaces it with the native count.
    pub(crate) fn hd6309_native_cycles(&mut self, ctx: &mut StepCtx) {
        if ctx.trap == Some(Trap::IllegalOpcode) && ctx.mnemonic == "TRAP" {
            // Illegal instruction: the trap stacks W as well (20/21 -> 22/23).
            ctx.cycles += 2;
            return;
        }
        let (page, opcode) = match ctx.bytes.as_slice() {
            [0x10, op, ..] => (2, *op),
            [0x11, op, ..] => (3, *op),
            [op, ..] => (1, *op),
            [] => return,
        };
        let mut cycles = ctx.cycles as i32 - native_saving(page, opcode, ctx.cycles);
        if let Some(postbyte) = ctx.index_postbyte {
            cycles += native_index_extra(postbyte) - emulation_index_extra(postbyte);
        }
        if ctx.trap == Some(DIV_ZERO_TRAP) {
            cycles += 2; // division by zero: the $FFF0 trap stacks W as well
        }
        ctx.cycles = cycles.max(1) as u32;
    }

    /// HD6309-only page-1 opcodes: OIM/AIM/EIM/TIM ($01/$02/$05/$0B direct,
    /// $6x indexed, $7x extended). The immediate byte precedes the address.
    pub(crate) fn try_hd6309_page1(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_hd6309() {
            return false;
        }
        let Some(mode) = Mode::of_memory_op(opcode) else {
            return false;
        };
        let op = match opcode & 0x0F {
            0x1 => LogicOp::Or,
            0x2 => LogicOp::And,
            0x5 => LogicOp::Eor,
            0xB => LogicOp::Test,
            _ => return false,
        };
        self.op_logic_imm(mem, ctx, op, mode);
        true
    }

    pub(crate) fn try_hd6309_page2(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_hd6309() {
            return false;
        }

        match opcode {
            // $1020 (LBRA) and $108D (LBSR) only work on the 6809.
            0x20 | 0x8D => self.hd6309_illegal(mem, ctx),
            0x30..=0x37 => self.op_inter_reg(mem, ctx, opcode),
            0x38 => self.op_push_w(mem, ctx, true),
            0x39 => self.op_pull_w(mem, ctx, true),
            0x3A => self.op_push_w(mem, ctx, false),
            0x3B => self.op_pull_w(mem, ctx, false),

            // D inherent ($1040-$104F)
            0x40 => self.op_d_unary(ctx, "NEGD", neg16),
            0x43 => self.op_d_unary(ctx, "COMD", com16),
            0x44 => self.op_d_unary(ctx, "LSRD", lsr16),
            0x46 => self.op_d_unary(ctx, "RORD", ror16),
            0x47 => self.op_d_unary(ctx, "ASRD", asr16),
            0x48 => self.op_d_unary(ctx, "ASLD", asl16),
            0x49 => self.op_d_unary(ctx, "ROLD", rol16),
            0x4A => self.op_d_unary(ctx, "DECD", dec16),
            0x4C => self.op_d_unary(ctx, "INCD", inc16),
            0x4D => {
                tst16(self.get_reg16(Reg16::D), &mut self.cc);
                self.inherent(ctx, "TSTD", 3);
            }
            0x4F => {
                self.set_reg16(Reg16::D, 0);
                self.clear_flags();
                self.inherent(ctx, "CLRD", 3);
            }

            // W inherent ($1053-$105F). $1050/$1057/$1058 (MAME's NEGW, ASRW,
            // ASLW) trap on real HD6309s (hoglet67), like every unlisted opcode.
            0x53 => self.op_w_unary(ctx, "COMW", com16),
            0x54 => self.op_w_unary(ctx, "LSRW", lsr16),
            0x56 => self.op_w_unary(ctx, "RORW", ror16),
            0x59 => self.op_w_unary(ctx, "ROLW", rol16),
            0x5A => self.op_w_unary(ctx, "DECW", dec16),
            0x5C => self.op_w_unary(ctx, "INCW", inc16),
            0x5D => {
                tst16(self.w, &mut self.cc);
                self.inherent(ctx, "TSTW", 3);
            }
            0x5F => {
                self.w = 0;
                self.clear_flags();
                self.inherent(ctx, "CLRW", 3);
            }

            0x80..=0xBF => match opcode & 0x0F {
                0x0 => self.op_word(mem, ctx, opcode, "SUBW", ALU16_CYCLES, |c, v| {
                    c.w = sub16(c.w, v, &mut c.cc);
                }),
                0x1 => self.op_word(mem, ctx, opcode, "CMPW", ALU16_CYCLES, |c, v| {
                    cmp16(c.w, v, &mut c.cc);
                }),
                0x2 => self.op_word(mem, ctx, opcode, "SBCD", ALU16_CYCLES, |c, v| {
                    let d = sbc16(c.get_reg16(Reg16::D), v, &mut c.cc);
                    c.set_reg16(Reg16::D, d);
                }),
                0x4 => self.op_word(mem, ctx, opcode, "ANDD", ALU16_CYCLES, |c, v| {
                    let d = and16(c.get_reg16(Reg16::D), v, &mut c.cc);
                    c.set_reg16(Reg16::D, d);
                }),
                0x5 => self.op_word(mem, ctx, opcode, "BITD", ALU16_CYCLES, |c, v| {
                    bit16(c.get_reg16(Reg16::D), v, &mut c.cc);
                }),
                0x6 => self.op_word(mem, ctx, opcode, "LDW", LDW_CYCLES, |c, v| {
                    c.w = v;
                    c.cc.remove(Flags::V);
                    c.cc.set_nz16(v);
                }),
                0x7 if Mode::of(opcode) != Mode::Imm => {
                    let w = self.w;
                    self.op_store_word(mem, ctx, opcode, "STW", LDW_CYCLES, w);
                }
                0x8 => self.op_word(mem, ctx, opcode, "EORD", ALU16_CYCLES, |c, v| {
                    let d = eor16(c.get_reg16(Reg16::D), v, &mut c.cc);
                    c.set_reg16(Reg16::D, d);
                }),
                0x9 => self.op_word(mem, ctx, opcode, "ADCD", ALU16_CYCLES, |c, v| {
                    let d = adc16(c.get_reg16(Reg16::D), v, &mut c.cc);
                    c.set_reg16(Reg16::D, d);
                }),
                0xA => self.op_word(mem, ctx, opcode, "ORD", ALU16_CYCLES, |c, v| {
                    let d = or16(c.get_reg16(Reg16::D), v, &mut c.cc);
                    c.set_reg16(Reg16::D, d);
                }),
                0xB => self.op_word(mem, ctx, opcode, "ADDW", ALU16_CYCLES, |c, v| {
                    c.w = add16(c.w, v, &mut c.cc);
                }),
                // CMPD, CMPY, LDY, STY: shared with the 6809 (exec_page2)
                _ => return false,
            },
            0xDC | 0xEC | 0xFC => self.op_ldq_mem(mem, ctx, opcode),
            0xDD | 0xED | 0xFD => self.op_stq(mem, ctx, opcode),
            _ => return false,
        }
        true
    }

    pub(crate) fn try_hd6309_page3(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_hd6309() {
            return false;
        }

        match opcode {
            0x30..=0x37 => self.op_bit_transfer(mem, ctx, opcode),
            0x38..=0x3B => self.op_tfm(mem, ctx, opcode),
            0x3C => self.op_bitmd(mem, ctx),
            0x3D => self.op_ldmd(mem, ctx),

            // E inherent ($1143-$114F)
            0x43 => self.op_e_unary(ctx, "COME", Cpu::op_com8),
            0x4A => self.op_e_unary(ctx, "DECE", Cpu::op_dec8),
            0x4C => self.op_e_unary(ctx, "INCE", Cpu::op_inc8),
            0x4D => {
                let e = self.get_e();
                self.test8(e);
                self.inherent(ctx, "TSTE", 3);
            }
            0x4F => {
                self.set_e(0);
                self.clear_flags();
                self.inherent(ctx, "CLRE", 3);
            }

            // F inherent ($1153-$115F)
            0x53 => self.op_f_unary(ctx, "COMF", Cpu::op_com8),
            0x5A => self.op_f_unary(ctx, "DECF", Cpu::op_dec8),
            0x5C => self.op_f_unary(ctx, "INCF", Cpu::op_inc8),
            0x5D => {
                let f = self.get_f();
                self.test8(f);
                self.inherent(ctx, "TSTF", 3);
            }
            0x5F => {
                self.set_f(0);
                self.clear_flags();
                self.inherent(ctx, "CLRF", 3);
            }

            0x80..=0xFF => {
                let use_e = opcode < 0xC0;
                match (opcode & 0x0F, use_e) {
                    (0x0, _) => self.op_byte_ef(mem, ctx, opcode, "SUB", |c, r, v| {
                        let h = c.cc.contains(Flags::H);
                        sub8(r, v, false, &mut c.cc);
                        c.cc.set(Flags::H, h); // SUB8 leaves H alone (MAME)
                        Some(r.wrapping_sub(v))
                    }),
                    (0x1, _) => self.op_byte_ef(mem, ctx, opcode, "CMP", |c, r, v| {
                        let h = c.cc.contains(Flags::H);
                        sub8(r, v, false, &mut c.cc);
                        c.cc.set(Flags::H, h);
                        None
                    }),
                    (0x6, _) => self.op_byte_ef(mem, ctx, opcode, "LD", |c, _, v| {
                        c.cc.remove(Flags::V);
                        c.cc.set_nz8(v);
                        Some(v)
                    }),
                    (0x7, _) if Mode::of(opcode) != Mode::Imm => self.op_store_ef(mem, ctx, opcode),
                    (0xB, _) => self.op_byte_ef(mem, ctx, opcode, "ADD", |c, r, v| {
                        add8(r, v, false, &mut c.cc);
                        Some(r.wrapping_add(v))
                    }),
                    (0xD, true) => self.op_divd(mem, ctx, opcode),
                    (0xE, true) => self.op_divq(mem, ctx, opcode),
                    (0xF, true) => self.op_muld(mem, ctx, opcode),
                    // CMPU, CMPS: shared with the 6809 (exec_page3)
                    _ => return false,
                }
            }
            _ => return false,
        }
        true
    }

    // ── Register helpers ─────────────────────────────────────────────────

    pub(crate) fn get_hd6309_reg16(&self, code: u8) -> u16 {
        match code {
            0x0 => self.get_reg16(Reg16::D),
            0x1 => self.x,
            0x2 => self.y,
            0x3 => self.u,
            0x4 => self.s,
            0x5 => self.pc,
            0x6 => self.w,
            0x7 => self.v,
            _ => 0,
        }
    }

    pub(crate) fn set_hd6309_reg16(&mut self, code: u8, value: u16) {
        match code {
            0x0 => self.set_reg16(Reg16::D, value),
            0x1 => self.x = value,
            0x2 => self.y = value,
            0x3 => self.u = value,
            0x4 => self.s = value,
            0x5 => self.pc = value,
            0x6 => self.w = value,
            0x7 => self.v = value,
            _ => {}
        }
    }

    fn get_q(&self) -> u32 {
        (u32::from(self.get_reg16(Reg16::D)) << 16) | u32::from(self.w)
    }

    fn set_q(&mut self, value: u32) {
        self.set_reg16(Reg16::D, (value >> 16) as u16);
        self.w = value as u16;
    }

    fn get_e(&self) -> u8 {
        (self.w >> 8) as u8
    }

    fn get_f(&self) -> u8 {
        self.w as u8
    }

    fn set_e(&mut self, value: u8) {
        self.w = (self.w & 0x00FF) | (u16::from(value) << 8);
    }

    fn set_f(&mut self, value: u8) {
        self.w = (self.w & 0xFF00) | u16::from(value);
    }

    /// CLR: N=0 Z=1 V=0 C=0.
    fn clear_flags(&mut self) {
        self.cc.remove(Flags::N | Flags::V | Flags::C);
        self.cc.insert(Flags::Z);
    }

    /// TST: N, Z from the value, V cleared, C unchanged.
    fn test8(&mut self, value: u8) {
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
    }

    fn inherent(&mut self, ctx: &mut StepCtx, name: &str, cycles: u32) {
        ctx.cycles = cycles;
        ctx.mnemonic = name.into();
        ctx.operands.clear();
    }

    /// Effective address of a direct/indexed/extended operand:
    /// (address, indexed extra cycles, operand text).
    fn ea(&mut self, mem: &Memory, ctx: &mut StepCtx, mode: Mode) -> (u16, u32, String) {
        match mode {
            Mode::Dir => {
                let (addr, text) = self.addr_direct(mem, ctx);
                (addr, 0, text)
            }
            Mode::Idx => {
                let (addr, extra, text) = self.addr_indexed(mem, ctx);
                (addr, u32::from(extra), text)
            }
            Mode::Imm | Mode::Ext => {
                debug_assert!(mode == Mode::Ext, "immediate operand has no address");
                let (addr, text) = self.addr_extended(mem, ctx);
                (addr, 0, text)
            }
        }
    }

    fn operand8(&mut self, mem: &Memory, ctx: &mut StepCtx, mode: Mode) -> (u8, u32, String) {
        if mode == Mode::Imm {
            let value = self.fetch_imm8(mem, ctx);
            return (value, 0, format!("#${value:02X}"));
        }
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        (mem.read8(addr), extra, text)
    }

    fn operand16(&mut self, mem: &Memory, ctx: &mut StepCtx, mode: Mode) -> (u16, u32, String) {
        if mode == Mode::Imm {
            let value = self.fetch_imm16(mem, ctx);
            return (value, 0, format!("#${value:04X}"));
        }
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        (mem.read16(addr), extra, text)
    }

    // ── $FFF0 trap ───────────────────────────────────────────────────────

    /// Illegal-instruction trap (MD bit 6), for an illegal opcode or (called by
    /// `Cpu::step`) an illegal indexed postbyte. The trap takes 19 cycles plus
    /// one per byte fetched so far: 20 for a page-1 opcode, 21 with a $10/$11
    /// prefix or for `op postbyte`, 22 for `$10 op postbyte` or
    /// `AIM #i,postbyte` (hoglet67). Native mode adds 2 for W.
    pub(crate) fn hd6309_illegal(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let cycles = 19 + ctx.bytes.len() as u32;
        self.trap_illegal(mem, ctx, cycles);
    }

    fn trap_illegal(&mut self, mem: &mut Memory, ctx: &mut StepCtx, cycles: u32) {
        self.enter_hw_trap(mem, MD_ILLEGAL);
        ctx.cycles = cycles;
        ctx.mnemonic = "TRAP".into();
        ctx.operands = "$FFF0".into();
        ctx.trap = Some(Trap::IllegalOpcode);
    }

    /// Division by zero (MD bit 7). `cycles` already includes the stacking.
    /// The chip sets Z and clears N and V before stacking CC (hoglet67).
    fn trap_div_zero(&mut self, mem: &mut Memory, ctx: &mut StepCtx, cycles: u32) {
        self.cc.insert(Flags::Z);
        self.cc.remove(Flags::N | Flags::V);
        self.enter_hw_trap(mem, MD_DIV_ZERO);
        ctx.cycles = cycles;
        ctx.trap = Some(DIV_ZERO_TRAP);
    }

    /// Flags of LDQ, STQ and MULD on real HD6309s (hoglet67 random testing):
    /// N from bit 31 but Z from the upper 16 bits (D) only; V and C unchanged.
    fn set_q_flags(&mut self, q: u32) {
        self.cc.set(Flags::N, q & 0x8000_0000 != 0);
        self.cc.set(Flags::Z, q >> 16 == 0);
    }

    // ── Page 1: OIM/AIM/EIM/TIM, SEXW, LDQ # ────────────────────────────

    fn op_logic_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, op: LogicOp, mode: Mode) {
        let imm = self.fetch_imm8(mem, ctx);
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        let value = mem.read8(addr);
        let result = match op {
            LogicOp::Or => value | imm,
            LogicOp::And | LogicOp::Test => value & imm,
            LogicOp::Eor => value ^ imm,
        };
        // N, Z from the result, V cleared, C unchanged (also for TIM).
        self.test8(result);
        let base = if op == LogicOp::Test {
            mem_cycles(mode, 4, 5, 5)
        } else {
            mem.write8(addr, result);
            mem_cycles(mode, 6, 7, 7)
        };
        ctx.cycles = base + extra;
        ctx.mnemonic = match op {
            LogicOp::Or => "OIM",
            LogicOp::And => "AIM",
            LogicOp::Eor => "EIM",
            LogicOp::Test => "TIM",
        }
        .into();
        ctx.operands = format!("#${imm:02X},{text}");
    }

    /// SEXW: sign-extend W into D. N from D, Z from Q (D:W); V unchanged.
    pub(crate) fn op_sexw(&mut self, ctx: &mut StepCtx) {
        let d = if self.w & 0x8000 != 0 { 0xFFFF } else { 0x0000 };
        self.set_reg16(Reg16::D, d);
        self.cc.set(Flags::N, d & 0x8000 != 0);
        self.cc.set(Flags::Z, d == 0 && self.w == 0);
        self.inherent(ctx, "SEXW", 4);
    }

    /// LDQ #imm32 ($CD).
    pub(crate) fn op_ldq_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let high = self.fetch_imm16(mem, ctx);
        let low = self.fetch_imm16(mem, ctx);
        let value = (u32::from(high) << 16) | u32::from(low);
        self.load_q(value);
        ctx.cycles = LDQ_CYCLES[Mode::Imm as usize];
        ctx.mnemonic = "LDQ".into();
        ctx.operands = format!("#${value:08X}");
    }

    fn load_q(&mut self, value: u32) {
        self.set_q(value);
        self.set_q_flags(value);
    }

    fn op_ldq_mem(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let mode = Mode::of(opcode);
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        let high = mem.read16(addr);
        let low = mem.read16(addr.wrapping_add(2));
        self.load_q((u32::from(high) << 16) | u32::from(low));
        ctx.cycles = LDQ_CYCLES[mode as usize] + extra;
        ctx.mnemonic = "LDQ".into();
        ctx.operands = text;
    }

    fn op_stq(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let mode = Mode::of(opcode);
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        mem.write16(addr, self.get_reg16(Reg16::D));
        mem.write16(addr.wrapping_add(2), self.w);
        let q = self.get_q();
        self.set_q_flags(q);
        ctx.cycles = LDQ_CYCLES[mode as usize] + extra;
        ctx.mnemonic = "STQ".into();
        ctx.operands = text;
    }

    // ── Page 2 ───────────────────────────────────────────────────────────

    /// ADDR ADCR SUBR SBCR ANDR ORR EORR CMPR ($1030-$1037) as the chip does
    /// them (Atkinson, "6309 Inter-Register Operations"; hoglet67): postbyte
    /// high nibble = source, low nibble = destination, dst = dst OP src.
    /// The destination's size decides the operation size. An 8-bit destination
    /// uses the low byte of a 16-bit source; a 16-bit destination promotes A/B
    /// to D, E/F to W, CC to $00:CC, DP to DP:$00 and 0 to zero. PC reads one
    /// ahead of the next instruction (pipelining). H is never changed. With CC
    /// as destination the result is stored in CC, then the new N/Z/V/C (N/Z
    /// for the logical operations) conditions are set on top of it.
    fn op_inter_reg(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src = postbyte >> 4;
        let dst = postbyte & 0x0F;
        let mut flags = self.cc;

        let result = if dst < 8 {
            let s = self.inter_reg_source16(src);
            let d = self.inter_reg_value16(dst);
            match opcode {
                0x30 => add16(d, s, &mut flags),
                0x31 => adc16(d, s, &mut flags),
                0x32 | 0x37 => sub16(d, s, &mut flags),
                0x33 => sbc16(d, s, &mut flags),
                0x34 => and16(d, s, &mut flags),
                0x35 => or16(d, s, &mut flags),
                _ => eor16(d, s, &mut flags),
            }
        } else {
            let s = self.inter_reg_source8(src);
            let d = self.inter_reg_value8(dst);
            let carry = flags.contains(Flags::C);
            let result = match opcode {
                0x30 | 0x31 => {
                    let c = opcode == 0x31 && carry;
                    add8(d, s, c, &mut flags);
                    d.wrapping_add(s).wrapping_add(u8::from(c))
                }
                0x32 | 0x33 | 0x37 => {
                    let c = opcode == 0x33 && carry;
                    sub8(d, s, c, &mut flags);
                    d.wrapping_sub(s).wrapping_sub(u8::from(c))
                }
                0x34 => d & s,
                0x35 => d | s,
                _ => d ^ s,
            };
            if matches!(opcode, 0x34..=0x36) {
                flags.remove(Flags::V);
                flags.set_nz8(result);
            }
            u16::from(result)
        };
        flags.set(Flags::H, self.cc.contains(Flags::H));

        if opcode == 0x37 {
            self.cc = flags; // CMPR only compares
        } else if dst == 0xA {
            let conditions = if opcode <= 0x33 {
                Flags::N | Flags::Z | Flags::V | Flags::C
            } else {
                Flags::N | Flags::Z
            };
            self.cc = Flags::from_byte(result as u8) | (flags & conditions);
        } else {
            self.cc = flags;
            self.inter_reg_write(dst, result);
        }

        ctx.cycles = 4;
        ctx.mnemonic = match opcode {
            0x30 => "ADDR",
            0x31 => "ADCR",
            0x32 => "SUBR",
            0x33 => "SBCR",
            0x34 => "ANDR",
            0x35 => "ORR",
            0x36 => "EORR",
            _ => "CMPR",
        }
        .into();
        ctx.operands = format!("{},{}", hd6309_reg_name(src), hd6309_reg_name(dst));
    }

    /// Current value of a 16-bit register in an inter-register operation.
    /// PC reads as the address of the next instruction + 1 (hoglet67).
    fn inter_reg_value16(&self, code: u8) -> u16 {
        if code == 0x5 {
            self.pc.wrapping_add(1)
        } else {
            self.get_hd6309_reg16(code)
        }
    }

    /// Source operand for a 16-bit destination (Atkinson's promotion table).
    fn inter_reg_source16(&self, code: u8) -> u16 {
        match code {
            0x0..=0x7 => self.inter_reg_value16(code),
            0x8 | 0x9 => self.get_reg16(Reg16::D),
            0xA => u16::from(self.cc.bits()), // $00:CC
            0xB => u16::from(self.dp) << 8,   // DP:$00
            0xE | 0xF => self.w,
            _ => 0, // the zero register
        }
    }

    /// Current value of an 8-bit register (the zero register reads 0).
    fn inter_reg_value8(&self, code: u8) -> u8 {
        match code {
            0x8 => self.a,
            0x9 => self.b,
            0xA => self.cc.bits(),
            0xB => self.dp,
            0xE => self.get_e(),
            0xF => self.get_f(),
            _ => 0,
        }
    }

    /// Source operand for an 8-bit destination: 16-bit registers are demoted
    /// to their low byte (D -> B, W -> F).
    fn inter_reg_source8(&self, code: u8) -> u8 {
        if code < 8 {
            self.inter_reg_value16(code) as u8
        } else {
            self.inter_reg_value8(code)
        }
    }

    /// Store an inter-register result (CC is handled by the caller; writes to
    /// the zero register are dropped). Writing PC jumps; on the chip such a
    /// write lands in the middle of the next instruction fetch (Atkinson:
    /// "unpredictable"), which is not modelled.
    fn inter_reg_write(&mut self, code: u8, value: u16) {
        match code {
            0x0..=0x7 => {
                self.set_hd6309_reg16(code, value);
                if code == 0x4 {
                    self.lds_encountered = true;
                }
            }
            0x8 => self.a = value as u8,
            0x9 => self.b = value as u8,
            0xB => self.dp = value as u8,
            0xE => self.set_e(value as u8),
            0xF => self.set_f(value as u8),
            _ => {}
        }
    }

    /// PSHSW/PSHUW: 6 cycles.
    fn op_push_w(&mut self, mem: &mut Memory, ctx: &mut StepCtx, system: bool) {
        if system {
            self.push16(mem, self.w);
        } else {
            self.push16_u(mem, self.w);
        }
        self.inherent(ctx, if system { "PSHSW" } else { "PSHUW" }, 6);
    }

    /// PULSW/PULUW: 6 cycles.
    fn op_pull_w(&mut self, mem: &mut Memory, ctx: &mut StepCtx, system: bool) {
        self.w = if system {
            self.pull16(mem)
        } else {
            self.pull16_u(mem)
        };
        self.inherent(ctx, if system { "PULSW" } else { "PULUW" }, 6);
    }

    fn op_d_unary(&mut self, ctx: &mut StepCtx, name: &str, op: fn(u16, &mut Flags) -> u16) {
        let d = op(self.get_reg16(Reg16::D), &mut self.cc);
        self.set_reg16(Reg16::D, d);
        self.inherent(ctx, name, 3);
    }

    fn op_w_unary(&mut self, ctx: &mut StepCtx, name: &str, op: fn(u16, &mut Flags) -> u16) {
        self.w = op(self.w, &mut self.cc);
        self.inherent(ctx, name, 3);
    }

    /// 16-bit ALU or load with an immediate/memory operand.
    fn op_word(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        opcode: u8,
        name: &str,
        cycles: [u32; 4],
        op: fn(&mut Cpu, u16),
    ) {
        let mode = Mode::of(opcode);
        let (value, extra, text) = self.operand16(mem, ctx, mode);
        op(self, value);
        ctx.cycles = cycles[mode as usize] + extra;
        ctx.mnemonic = name.into();
        ctx.operands = text;
    }

    fn op_store_word(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        opcode: u8,
        name: &str,
        cycles: [u32; 4],
        value: u16,
    ) {
        let mode = Mode::of(opcode);
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        mem.write16(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = cycles[mode as usize] + extra;
        ctx.mnemonic = name.into();
        ctx.operands = text;
    }

    // ── Page 3 ───────────────────────────────────────────────────────────

    fn op_e_unary(&mut self, ctx: &mut StepCtx, name: &str, op: fn(&mut Cpu, u8) -> u8) {
        let e = self.get_e();
        let result = op(self, e);
        self.set_e(result);
        self.inherent(ctx, name, 3);
    }

    fn op_f_unary(&mut self, ctx: &mut StepCtx, name: &str, op: fn(&mut Cpu, u8) -> u8) {
        let f = self.get_f();
        let result = op(self, f);
        self.set_f(result);
        self.inherent(ctx, name, 3);
    }

    /// SUBE/CMPE/LDE/ADDE ($118x-$11Bx) and the F forms ($11Cx-$11Fx).
    /// `op` returns the new register value (None: compare only).
    fn op_byte_ef(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        opcode: u8,
        stem: &str,
        op: fn(&mut Cpu, u8, u8) -> Option<u8>,
    ) {
        let use_e = opcode < 0xC0;
        let mode = Mode::of(opcode);
        let (value, extra, text) = self.operand8(mem, ctx, mode);
        let reg = if use_e { self.get_e() } else { self.get_f() };
        if let Some(result) = op(self, reg, value) {
            if use_e {
                self.set_e(result);
            } else {
                self.set_f(result);
            }
        }
        ctx.cycles = ALU8_EF_CYCLES[mode as usize] + extra;
        ctx.mnemonic = format!("{stem}{}", if use_e { 'E' } else { 'F' });
        ctx.operands = text;
    }

    /// STE ($1197/$11A7/$11B7), STF ($11D7/$11E7/$11F7).
    fn op_store_ef(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let use_e = opcode < 0xC0;
        let mode = Mode::of(opcode);
        let (addr, extra, text) = self.ea(mem, ctx, mode);
        let value = if use_e { self.get_e() } else { self.get_f() };
        mem.write8(addr, value);
        self.test8(value);
        ctx.cycles = ALU8_EF_CYCLES[mode as usize] + extra;
        ctx.mnemonic = if use_e { "STE" } else { "STF" }.into();
        ctx.operands = text;
    }

    /// BITMD #imm ($113C): only Z changes — set when none of the tested status
    /// bits (6/7) is set (hoglet67, Atkinson). The tested status bits are
    /// cleared; NM/FM (bits 0/1) can not be tested and are never touched.
    fn op_bitmd(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let imm = self.fetch_imm8(mem, ctx);
        let hit = self.mode_reg & imm & (MD_ILLEGAL | MD_DIV_ZERO);
        self.cc.set(Flags::Z, hit == 0);
        self.mode_reg &= !(imm & (MD_ILLEGAL | MD_DIV_ZERO));
        ctx.cycles = 4;
        ctx.mnemonic = "BITMD".into();
        ctx.operands = format!("#${imm:02X}");
    }

    /// LDMD #imm ($113D): only NM (bit 0) and FM (bit 1) are writable.
    fn op_ldmd(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let imm = self.fetch_imm8(mem, ctx);
        self.mode_reg = (self.mode_reg & !MD_WRITABLE) | (imm & MD_WRITABLE);
        ctx.cycles = 5;
        ctx.mnemonic = "LDMD".into();
        ctx.operands = format!("#${imm:02X}");
    }

    /// BAND BIAND BOR BIOR BEOR BIEOR LDBT STBT ($1130-$1137), direct only.
    /// Postbyte: bits 7-6 register (00 CC, 01 A, 10 B, 11 none), bits 5-3
    /// source bit, bits 2-0 destination bit. The logic operations and LDBT
    /// combine memory bit (5-3) into register bit (2-0) and change no flags
    /// (except the CC bit itself); STBT copies register bit (5-3) into memory
    /// bit (2-0) and sets N/Z from the stored byte (hoglet67).
    fn op_bit_transfer(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let (addr, text) = self.addr_direct(mem, ctx);
        let reg = postbyte >> 6;
        let src_bit = (postbyte >> 3) & 0x07;
        let dst_bit = postbyte & 0x07;
        let reg_value = match reg {
            0 => Some(self.cc.bits()),
            1 => Some(self.a),
            2 => Some(self.b),
            _ => None,
        };
        let mem_value = mem.read8(addr);

        if opcode == 0x37 {
            // STBT: register code 3 has no register; its source bit reads 0.
            let bit = reg_value.is_some_and(|r| (r >> src_bit) & 1 != 0);
            let mask = 1u8 << dst_bit;
            let new = if bit { mem_value | mask } else { mem_value & !mask };
            mem.write8(addr, new);
            self.cc.set_nz8(new);
            ctx.cycles = 8;
        } else {
            // Register code 3 is a no-op (MAME operates on a scratch byte).
            if let Some(r) = reg_value {
                let m = (mem_value >> src_bit) & 1 != 0;
                let d = (r >> dst_bit) & 1 != 0;
                let result = match opcode {
                    0x30 => d && m,
                    0x31 => d && !m,
                    0x32 => d || m,
                    0x33 => d || !m,
                    0x34 => d != m,
                    0x35 => d == m,
                    _ => m, // LDBT
                };
                let mask = 1u8 << dst_bit;
                let new = if result { r | mask } else { r & !mask };
                match reg {
                    0 => self.cc = Flags::from_byte(new),
                    1 => self.a = new,
                    _ => self.b = new,
                }
            }
            ctx.cycles = 7;
        }

        ctx.mnemonic = match opcode {
            0x30 => "BAND",
            0x31 => "BIAND",
            0x32 => "BOR",
            0x33 => "BIOR",
            0x34 => "BEOR",
            0x35 => "BIEOR",
            0x36 => "LDBT",
            _ => "STBT",
        }
        .into();
        ctx.operands = format!("{},{src_bit},{dst_bit},{text}", bit_reg_name(reg));
    }

    /// MULD: Q = D * operand (signed 16 x 16). N from bit 31, Z from the upper
    /// word, V and C unchanged; +1 cycle per negative operand and +1 when the
    /// product is negative (hoglet67's exhaustive MULD test on a real HD6309).
    fn op_muld(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let mode = Mode::of(opcode);
        let (value, extra, text) = self.operand16(mem, ctx, mode);
        let d = self.get_reg16(Reg16::D) as i16;
        let m = value as i16;
        let product = i32::from(d).wrapping_mul(i32::from(m)) as u32;
        self.set_q(product);
        self.set_q_flags(product);
        let sign_penalty = u32::from(d < 0) + u32::from(m < 0) + u32::from((d < 0) != (m < 0));
        ctx.cycles = MULD_CYCLES[mode as usize] + extra + sign_penalty;
        ctx.mnemonic = "MULD".into();
        ctx.operands = text;
    }

    /// DIVD: D / signed 8-bit operand -> B = quotient, A = remainder.
    fn op_divd(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let mode = Mode::of(opcode);
        let (divisor, extra, text) = self.operand8(mem, ctx, mode);
        ctx.mnemonic = "DIVD".into();
        ctx.operands = text;
        let base = DIVD_CYCLES[mode as usize] + extra;
        match self.exec_divd(divisor) {
            Some(adjust) => ctx.cycles = (base as i32 + adjust) as u32,
            // The zero test ends the division early; the count includes the
            // stacking of the trap (hoglet67: 2 cycles less than a division).
            None => self.trap_div_zero(mem, ctx, base - 2),
        }
    }

    /// DIVQ: Q / signed 16-bit operand -> W = quotient, D = remainder.
    fn op_divq(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let mode = Mode::of(opcode);
        let (divisor, extra, text) = self.operand16(mem, ctx, mode);
        ctx.mnemonic = "DIVQ".into();
        ctx.operands = text;
        let base = DIVQ_CYCLES[mode as usize] + extra;
        match self.exec_divq(divisor) {
            Some(adjust) => ctx.cycles = (base as i32 + adjust) as u32,
            None => self.trap_div_zero(mem, ctx, base - 10),
        }
    }

    /// MAME `divd()`. Returns the data-dependent cycle adjustment (hoglet67:
    /// +1 negative dividend, +1 negative divisor, -1 two's-complement
    /// overflow, -13 range overflow) or None for a division by zero.
    fn exec_divd(&mut self, divisor: u8) -> Option<i32> {
        if divisor == 0 {
            return None;
        }
        let mut adjust = 0;
        let mut dividend = self.get_reg16(Reg16::D);
        let dividend_negative = dividend & 0x8000 != 0;
        if dividend_negative {
            dividend = dividend.wrapping_neg();
            self.set_reg16(Reg16::D, dividend); // stays in D on range overflow
            adjust += 1;
        }
        let divisor_negative = divisor & 0x80 != 0;
        let magnitude = if divisor_negative {
            adjust += 1;
            divisor.wrapping_neg()
        } else {
            divisor
        };
        let mut quotient = dividend / u16::from(magnitude);
        let mut remainder = (dividend % u16::from(magnitude)) as u8;

        self.cc.remove(Flags::N | Flags::Z | Flags::V | Flags::C);
        if quotient >> 8 != 0 {
            // Range overflow: D keeps the dividend's magnitude.
            self.cc.insert(Flags::V);
            self.cc.set(Flags::N, dividend_negative);
            return Some(adjust - 13);
        }
        if dividend_negative {
            remainder = remainder.wrapping_neg();
        }
        if dividend_negative != divisor_negative {
            let negated = quotient.wrapping_neg();
            if negated & 0x80 != 0 && quotient & 0x80 == 0 {
                quotient = negated;
            }
        }
        self.a = remainder;
        self.b = quotient as u8;
        self.cc.set(Flags::C, self.b & 0x01 != 0);
        if ((quotient >> 15) ^ (u16::from(self.b) >> 7)) & 1 == 0 {
            self.cc.set_nz8(self.b);
        } else {
            // Two's-complement overflow: quotient does not fit in 8 signed bits.
            self.cc.insert(Flags::N | Flags::V);
            adjust -= 1;
        }
        Some(adjust)
    }

    /// MAME `divq()`. Cycle adjustment (hoglet67): +1 negative dividend,
    /// +1 negative divisor, -21 range overflow; None for a division by zero.
    fn exec_divq(&mut self, divisor: u16) -> Option<i32> {
        if divisor == 0 {
            return None;
        }
        let mut adjust = 0;
        let mut dividend = self.get_q();
        let dividend_negative = dividend & 0x8000_0000 != 0;
        if dividend_negative {
            dividend = dividend.wrapping_neg();
            self.set_q(dividend); // stays in Q on range overflow
            adjust += 1;
        }
        let divisor_negative = divisor & 0x8000 != 0;
        let magnitude = if divisor_negative {
            adjust += 1;
            divisor.wrapping_neg()
        } else {
            divisor
        };
        let mut quotient = dividend / u32::from(magnitude);
        let mut remainder = (dividend % u32::from(magnitude)) as u16;

        self.cc.remove(Flags::N | Flags::Z | Flags::V | Flags::C);
        if quotient >> 16 != 0 {
            self.cc.insert(Flags::V);
            self.cc.set(Flags::N, dividend_negative);
            return Some(adjust - 21);
        }
        if dividend_negative {
            remainder = remainder.wrapping_neg();
        }
        if dividend_negative != divisor_negative {
            let negated = quotient.wrapping_neg();
            if negated & 0x8000 != 0 && quotient & 0x8000 == 0 {
                quotient = negated;
            }
        }
        self.set_reg16(Reg16::D, remainder);
        self.w = quotient as u16;
        self.cc.set(Flags::C, self.w & 0x0001 != 0);
        if ((quotient >> 31) ^ (u32::from(self.w) >> 15)) & 1 == 0 {
            self.cc.set_nz16(self.w);
        } else {
            self.cc.insert(Flags::N | Flags::V);
        }
        Some(adjust)
    }

    // ── TFM ──────────────────────────────────────────────────────────────

    /// TFM ($1138-$113B): moves W bytes, 6 + 3n cycles, one byte per step.
    /// Only D, X, Y, U and S can be pointers; anything else traps (23 cycles).
    /// The registers and W are updated after every byte and PC stays on the
    /// TFM until the transfer is complete, so an interrupt between two bytes
    /// returns to the TFM, which resumes with the remaining count (MAME:
    /// PC -= 3). The chip sets Z = (W == 0), also before the trap (hoglet67).
    fn op_tfm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src_code = postbyte >> 4;
        let dst_code = postbyte & 0x0F;
        if src_code > 4 || dst_code > 4 {
            self.cc.set(Flags::Z, self.w == 0);
            self.trap_illegal(mem, ctx, 23);
            return;
        }
        let pc_before = self.pc.wrapping_sub(ctx.bytes.len() as u16);
        self.tfm_pending = Some(TfmPending {
            opcode,
            src: self.get_hd6309_reg16(src_code),
            dst: self.get_hd6309_reg16(dst_code),
            src_code,
            dst_code,
            postbyte,
            remaining: self.w,
            pc_before,
            bytes: ctx.bytes.clone(),
            first_chunk: true,
        });
        self.run_tfm_chunk(mem, ctx);
    }

    /// Continue a pending TFM (called by `Cpu::step`).
    pub(crate) fn run_tfm_chunk_step(&mut self, mem: &mut Memory) -> StepResult {
        let (pc_before, bytes) = match &self.tfm_pending {
            Some(t) => (t.pc_before, t.bytes.clone()),
            None => {
                return StepResult {
                    cycles: 0,
                    pc_before: self.pc,
                    pc_after: self.pc,
                    opcode: 0,
                    bytes: vec![],
                    mnemonic: String::new(),
                    operands: String::new(),
                    trap: None,
                };
            }
        };
        if self.pc != pc_before {
            // The debugger moved PC: abandon the transfer (W keeps the rest).
            self.tfm_pending = None;
            return self.step(mem);
        }

        mem.clear_watchpoint_trigger();
        let mut ctx = StepCtx::new(bytes.first().copied().unwrap_or(0x11));
        ctx.bytes = bytes;
        self.run_tfm_chunk(mem, &mut ctx);
        self.total_cycles += u64::from(ctx.cycles);

        let trap = if mem.take_watchpoint_trigger().is_some() {
            Some(Trap::Watchpoint)
        } else {
            None
        };
        StepResult {
            cycles: ctx.cycles,
            pc_before,
            pc_after: self.pc,
            opcode: ctx.bytes.first().copied().unwrap_or(0x11),
            bytes: ctx.bytes,
            mnemonic: ctx.mnemonic,
            operands: ctx.operands,
            trap,
        }
    }

    /// Move up to `TFM_CHUNK_SIZE` bytes. Per byte (MAME tfr_read/tfr_write):
    /// read [src], step src, write [dst], step dst, W -= 1 — so a register used
    /// as both source and destination sees both steps. Z follows W.
    fn run_tfm_chunk(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let Some(mut pending) = self.tfm_pending.take() else {
            ctx.cycles = 0;
            return;
        };
        let (src_step, dst_step) = tfm_steps(pending.opcode);
        let mut moved = 0u32;
        while self.w != 0 && moved < u32::from(TFM_CHUNK_SIZE) {
            let src = self.get_hd6309_reg16(pending.src_code);
            let byte = mem.read8(src);
            self.set_hd6309_reg16(pending.src_code, src.wrapping_add(src_step));
            let dst = self.get_hd6309_reg16(pending.dst_code);
            mem.write8(dst, byte);
            self.set_hd6309_reg16(pending.dst_code, dst.wrapping_add(dst_step));
            self.w -= 1;
            moved += 1;
        }
        pending.src = self.get_hd6309_reg16(pending.src_code);
        pending.dst = self.get_hd6309_reg16(pending.dst_code);
        pending.remaining = self.w;
        self.cc.set(Flags::Z, self.w == 0);

        ctx.cycles = 3 * moved + if pending.first_chunk { 6 } else { 0 };
        pending.first_chunk = false;
        ctx.mnemonic = "TFM".into();
        ctx.operands = tfm_operands(pending.opcode, pending.postbyte);

        if self.w == 0 {
            self.pc = pending
                .pc_before
                .wrapping_add(pending.bytes.len() as u16);
        } else {
            self.pc = pending.pc_before;
            self.tfm_pending = Some(pending);
        }
    }
}

/// Base cycles of a page-1 memory instruction by mode (direct, indexed, extended).
fn mem_cycles(mode: Mode, direct: u32, indexed: u32, extended: u32) -> u32 {
    match mode {
        Mode::Dir => direct,
        Mode::Idx => indexed,
        _ => extended,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::Reg16;
    use crate::memory::Memory;

    fn cpu6309() -> Cpu {
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.pc = 0x0100;
        cpu.s = 0x0F00;
        cpu.dp = 0;
        cpu
    }

    fn native6309() -> Cpu {
        let mut cpu = cpu6309();
        cpu.mode_reg = MD_NATIVE;
        cpu
    }

    fn program(bytes: &[u8]) -> Memory {
        let mut mem = Memory::new();
        mem.load_binary(0x0100, bytes).unwrap();
        mem
    }

    // ── OIM/AIM/EIM/TIM ──────────────────────────────────────────────────

    #[test]
    fn aim_on_6309() {
        // AIM #$F0,<$20 = 02 F0 20: immediate first, page 1.
        let mut cpu = cpu6309();
        cpu.cc = Flags::C | Flags::V;
        let mut mem = program(&[0x02, 0xF0, 0x20]);
        mem.write8(0x0020, 0xFF);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "AIM");
        assert_eq!(step.operands, "#$F0,<$20");
        assert_eq!(step.bytes, vec![0x02, 0xF0, 0x20]);
        assert_eq!(mem.read8(0x0020), 0xF0);
        assert_eq!(cpu.pc, 0x0103);
        assert_eq!(step.cycles, 6);
        assert!(cpu.cc.contains(Flags::N));
        assert!(!cpu.cc.contains(Flags::V));
        assert!(cpu.cc.contains(Flags::C), "C is not affected");
    }

    #[test]
    fn oim_indexed_and_eim_extended() {
        let mut cpu = cpu6309();
        cpu.x = 0x0300;
        // OIM #$0F,,X ; EIM #$FF,$1234 ; OIM #$01,<$34 (DP=$12)
        let mut mem = program(&[0x61, 0x0F, 0x84, 0x75, 0xFF, 0x12, 0x34, 0x01, 0x01, 0x34]);
        mem.write8(0x0300, 0xA0);
        mem.write8(0x1234, 0x0F);
        let step = cpu.step(&mut mem);
        assert_eq!((step.mnemonic.as_str(), step.operands.as_str()), ("OIM", "#$0F,,X"));
        assert_eq!(mem.read8(0x0300), 0xAF);
        assert_eq!(step.cycles, 7);
        let step = cpu.step(&mut mem);
        assert_eq!((step.mnemonic.as_str(), step.operands.as_str()), ("EIM", "#$FF,$1234"));
        assert_eq!(mem.read8(0x1234), 0xF0);
        assert_eq!(step.cycles, 7);
        cpu.dp = 0x12;
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x1234), 0xF1, "direct page $12");
        assert_eq!(cpu.pc, 0x010A);
    }

    #[test]
    fn tim_tests_without_writing_and_keeps_carry() {
        let mut cpu = cpu6309();
        cpu.cc = Flags::C | Flags::V;
        let mut mem = program(&[0x0B, 0x80, 0x20, 0x0B, 0x01, 0x20, 0x7B, 0x80, 0x00, 0x20]);
        mem.write8(0x0020, 0x80);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "TIM");
        assert_eq!(mem.read8(0x0020), 0x80);
        assert!(cpu.cc.contains(Flags::N));
        assert!(!cpu.cc.contains(Flags::Z));
        assert!(!cpu.cc.contains(Flags::V));
        assert!(cpu.cc.contains(Flags::C), "TIM leaves C alone");
        assert_eq!(step.cycles, 4);
        cpu.step(&mut mem);
        assert!(cpu.cc.contains(Flags::Z));
        let step = cpu.step(&mut mem);
        assert_eq!(step.operands, "#$80,$0020");
        assert_eq!(step.cycles, 5);
    }

    #[test]
    fn page2_aim_family_aliases_trap() {
        for op in [0x01u8, 0x02, 0x05, 0x0B, 0x61, 0x62, 0x65, 0x6B, 0x71, 0x72, 0x75, 0x7B] {
            let mut cpu = cpu6309();
            let mut mem = program(&[0x10, op, 0x20, 0xF0]);
            mem.write16(0xFFF0, 0x0600);
            mem.write8(0x0020, 0xFF);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode), "$10{op:02X}");
            assert_eq!(cpu.pc, 0x0600);
            assert_ne!(cpu.mode_reg & MD_ILLEGAL, 0);
            assert_eq!(mem.read8(0x0020), 0xFF);
        }
    }

    #[test]
    fn opcodes_missing_on_the_6309_trap() {
        // $1020 (LBRA), $108D (LBSR), $103E ("MULD inherent" / XSWI2)
        for bytes in [[0x10u8, 0x20, 0x00, 0x10], [0x10, 0x8D, 0x00, 0x10], [0x10, 0x3E, 0x12, 0x12]] {
            let mut cpu = cpu6309();
            let mut mem = program(&bytes);
            mem.write16(0xFFF0, 0x0600);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode), "{bytes:02X?}");
            assert_eq!(step.cycles, 21);
            assert_eq!(cpu.pc, 0x0600);
            assert_eq!(mem.read16(0x0F00 - 2), 0x0102, "return address after the opcode");
        }
        // The MC6809 keeps executing $1020 as LBRA.
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        let mut mem = program(&[0x10, 0x20, 0x00, 0x10]);
        assert_eq!(cpu.step(&mut mem).mnemonic, "LBRA");
        assert_eq!(cpu.pc, 0x0114);
    }

    // ── Inter-register operations ────────────────────────────────────────

    #[test]
    fn addr_w_d_adds_source_into_destination() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 1000);
        cpu.w = 234;
        let mut mem = program(&[0x10, 0x30, 0x60]);
        let step = cpu.step(&mut mem);
        assert_eq!((step.mnemonic.as_str(), step.operands.as_str()), ("ADDR", "W,D"));
        assert_eq!(cpu.get_reg16(Reg16::D), 1234);
        assert_eq!(cpu.w, 234);
        assert_eq!(step.cycles, 4);
    }

    #[test]
    fn subr_sbcr_cmpr_compute_destination_minus_source() {
        let mut cpu = cpu6309();
        cpu.x = 5;
        cpu.y = 3;
        // SUBR X,Y ; CMPR X,Y ; SBCR X,Y (C=1)
        let mut mem = program(&[0x10, 0x32, 0x12, 0x10, 0x37, 0x12, 0x10, 0x33, 0x12]);
        cpu.step(&mut mem);
        assert_eq!(cpu.y, 0xFFFE);
        assert!(cpu.cc.contains(Flags::C | Flags::N));
        assert_eq!(cpu.x, 5);
        cpu.y = 5;
        cpu.step(&mut mem);
        assert!(cpu.cc.contains(Flags::Z));
        assert_eq!(cpu.y, 5, "CMPR does not write");
        cpu.cc.insert(Flags::C);
        cpu.y = 10;
        cpu.step(&mut mem);
        assert_eq!(cpu.y, 4);
    }

    #[test]
    fn eight_bit_register_arithmetic_leaves_h_alone() {
        let mut cpu = cpu6309();
        cpu.a = 0x0F;
        cpu.b = 0x01;
        // ADDR A,B ; ADCR A,B ; SUBR A,B
        let mut mem = program(&[0x10, 0x30, 0x89, 0x10, 0x31, 0x89, 0x10, 0x32, 0x89]);
        cpu.step(&mut mem);
        assert_eq!(cpu.b, 0x10);
        assert_eq!(cpu.a, 0x0F);
        assert!(!cpu.cc.contains(Flags::H));
        cpu.cc.insert(Flags::H | Flags::C);
        cpu.step(&mut mem);
        assert_eq!(cpu.b, 0x20, "0x10 + 0x0F + C");
        assert!(cpu.cc.contains(Flags::H));
        cpu.step(&mut mem);
        assert_eq!(cpu.b, 0x11);
        assert!(cpu.cc.contains(Flags::H));
    }

    #[test]
    fn zero_register_reads_zero_and_discards_writes() {
        let mut cpu = cpu6309();
        cpu.a = 0x42;
        cpu.x = 0x1234;
        let s = cpu.s;
        // ADDR A,0 ; ADDR 0,X ; ADDR X,0 ; ANDR 0,D
        let mut mem = program(&[0x10, 0x30, 0x8C, 0x10, 0x30, 0xC1, 0x10, 0x30, 0x1D, 0x10, 0x34, 0xC0]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.operands, "A,0");
        assert_eq!(cpu.s, s, "the zero register is never S");
        assert_eq!(cpu.a, 0x42);
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x1234);
        assert_eq!(cpu.pc, 0x0106, "the zero register is never PC");
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x1234);
        assert!(!cpu.cc.contains(Flags::Z), "flags from 0 + X");
        cpu.set_reg16(Reg16::D, 0xFFFF);
        cpu.step(&mut mem);
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
        assert!(cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn mixed_size_register_operations_follow_the_destination() {
        // Atkinson, "6309 Inter-Register Operations" (hardware).
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x0102);
        cpu.x = 0x1034;
        // ADDR A,X: a 16-bit destination promotes A to D.
        let mut mem = program(&[
            0x10, 0x30, 0x81, // ADDR A,X
            0x10, 0x30, 0x18, // ADDR X,A
            0x10, 0x30, 0xB2, // ADDR DP,Y
            0x10, 0x30, 0xA3, // ADDR CC,U
            0x10, 0x30, 0xE1, // ADDR E,X
            0x10, 0x30, 0x6F, // ADDR W,F
        ]);
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x1136);
        // ADDR X,A: an 8-bit destination takes the low byte of X; B unchanged.
        cpu.step(&mut mem);
        assert_eq!((cpu.a, cpu.b), (0x37, 0x02));
        // DP promotes to DP:$00, CC to $00:CC.
        cpu.dp = 0x12;
        cpu.y = 0x0001;
        cpu.step(&mut mem);
        assert_eq!(cpu.y, 0x1201);
        cpu.u = 0x1000;
        cpu.cc = Flags::from_byte(0x05);
        cpu.step(&mut mem);
        assert_eq!(cpu.u, 0x1005);
        // E promotes to W.
        cpu.w = 0x0102;
        cpu.x = 0x1000;
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x1102);
        // W into F: F = F + low(W).
        cpu.w = 0x0101;
        let step = cpu.step(&mut mem);
        assert_eq!(step.operands, "W,F");
        assert_eq!(cpu.w, 0x0102);
    }

    #[test]
    fn cc_destination_stores_the_result_then_sets_the_conditions() {
        let mut cpu = cpu6309();
        cpu.a = 0x01;
        cpu.cc = Flags::empty();
        // ORR A,CC ; ANDR B,CC ; ADDR A,CC ; ADDR X,CC (low byte of X)
        let mut mem = program(&[0x10, 0x35, 0x8A, 0x10, 0x34, 0x9A, 0x10, 0x30, 0x8A, 0x10, 0x30, 0x1A]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.operands, "A,CC");
        assert_eq!(cpu.cc.bits(), 0x01);
        // ANDR: result 0 goes to CC, then Z is set on top of it.
        cpu.b = 0x00;
        cpu.cc = Flags::from_byte(0xFF);
        cpu.step(&mut mem);
        assert_eq!(cpu.cc.bits(), 0x04);
        // ADDR $80 + $80 = $00 with carry and overflow: CC = $00 | Z | V | C.
        cpu.a = 0x80;
        cpu.cc = Flags::from_byte(0x80);
        cpu.step(&mut mem);
        assert_eq!(cpu.cc.bits(), 0x07);
        // ADDR X,CC: 8-bit operation with the low byte of X ($01 + $08).
        cpu.x = 0x1208;
        cpu.cc = Flags::from_byte(0x01);
        cpu.step(&mut mem);
        assert_eq!(cpu.cc.bits(), 0x09);
    }

    #[test]
    fn inter_register_pc_reads_one_ahead() {
        // The pipelined PC: next instruction + 1 (hoglet67).
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x0010);
        let mut mem = program(&[0x10, 0x30, 0x51, 0x10, 0x30, 0x58, 0x10, 0x30, 0x05]);
        cpu.x = 0x0000;
        cpu.step(&mut mem); // ADDR PC,X at $0100: next = $0103
        assert_eq!(cpu.x, 0x0104);
        cpu.a = 0x00;
        cpu.step(&mut mem); // ADDR PC,A: low byte of $0107
        assert_eq!(cpu.a, 0x07);
        cpu.step(&mut mem); // ADDR D,PC: $0109 + 1 + D
        assert_eq!(cpu.pc, 0x010A + 0x0710);
    }

    // ── DIVD / DIVQ / MULD ───────────────────────────────────────────────

    #[test]
    fn divd_stores_quotient_in_b_and_remainder_in_a() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 23);
        cpu.w = 0xBEEF;
        cpu.cc = Flags::V | Flags::C | Flags::Z | Flags::N;
        let mut mem = program(&[0x11, 0x8D, 0x02, 0x12]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVD");
        assert_eq!(step.operands, "#$02");
        assert_eq!(step.bytes.len(), 3, "8-bit immediate");
        assert_eq!(cpu.pc, 0x0103);
        assert_eq!(cpu.b, 11);
        assert_eq!(cpu.a, 1);
        assert_eq!(cpu.w, 0xBEEF, "W is untouched");
        assert!(cpu.cc.contains(Flags::C), "C = quotient odd");
        assert!(!cpu.cc.intersects(Flags::N | Flags::Z | Flags::V));
        assert_eq!(step.cycles, 25);
    }

    #[test]
    fn divd_is_signed_with_data_dependent_timing() {
        // (dividend, divisor, A, B, N, V, C, cycles)
        type Case = (u16, u8, u8, u8, bool, bool, bool, u32);
        let cases: [Case; 7] = [
            (0xFFF9, 0x02, 0xFF, 0xFD, true, false, true, 26), // -7 / 2 = -3 r -1
            (0x0007, 0xFE, 0x01, 0xFD, true, false, true, 26), // 7 / -2 = -3 r 1
            (0xFFF9, 0xFE, 0xFF, 0x03, false, false, true, 27), // -7 / -2 = 3 r -1
            (200, 0x01, 0x00, 0xC8, true, true, false, 24),    // two's-complement overflow
            // |quotient| > 127 always overflows, even for -128 (MAME, hoglet67)
            (0x0080, 0xFF, 0x00, 0x80, true, true, false, 25), // 128 / -1
            (0x0000, 0x05, 0x00, 0x00, false, false, false, 25), // Z
            (0xFF80, 0x01, 0x00, 0x80, true, true, false, 25), // -128 / 1
        ];
        for (dividend, divisor, a, b, n, v, c, cycles) in cases {
            let mut cpu = cpu6309();
            cpu.set_reg16(Reg16::D, dividend);
            let mut mem = program(&[0x11, 0x8D, divisor]);
            let step = cpu.step(&mut mem);
            let what = format!("{dividend:04X}/{divisor:02X}");
            assert_eq!((cpu.a, cpu.b), (a, b), "{what}");
            assert_eq!(cpu.cc.contains(Flags::N), n, "N {what}");
            assert_eq!(cpu.cc.contains(Flags::V), v, "V {what}");
            assert_eq!(cpu.cc.contains(Flags::C), c, "C {what}");
            assert_eq!(cpu.cc.contains(Flags::Z), b == 0 && !v, "Z {what}");
            assert_eq!(step.cycles, cycles, "cycles {what}");
        }
    }

    #[test]
    fn divd_range_overflow_keeps_dividend_magnitude() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x1000);
        let mut mem = program(&[0x11, 0x8D, 0x01]);
        let step = cpu.step(&mut mem);
        assert_eq!(cpu.get_reg16(Reg16::D), 0x1000);
        assert!(cpu.cc.contains(Flags::V));
        assert!(!cpu.cc.intersects(Flags::N | Flags::Z | Flags::C));
        assert_eq!(step.cycles, 12);

        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0xF000);
        let mut mem = program(&[0x11, 0x8D, 0x01]);
        let step = cpu.step(&mut mem);
        assert_eq!(cpu.get_reg16(Reg16::D), 0x1000, "magnitude of -4096");
        assert!(cpu.cc.contains(Flags::V | Flags::N));
        assert_eq!(step.cycles, 13);
    }

    #[test]
    fn divd_memory_forms_read_one_byte() {
        let mut cpu = cpu6309();
        cpu.x = 0x0300;
        // DIVD <$20 ; DIVD ,X ; DIVD $1234
        let mut mem = program(&[0x11, 0x9D, 0x20, 0x11, 0xAD, 0x84, 0x11, 0xBD, 0x12, 0x34]);
        mem.write8(0x0020, 3);
        mem.write8(0x0300, 4);
        mem.write8(0x1234, 5);
        cpu.set_reg16(Reg16::D, 30);
        let step = cpu.step(&mut mem);
        assert_eq!((cpu.a, cpu.b, step.cycles), (0, 10, 27));
        cpu.set_reg16(Reg16::D, 30);
        let step = cpu.step(&mut mem);
        assert_eq!((cpu.a, cpu.b, step.cycles), (2, 7, 27));
        cpu.set_reg16(Reg16::D, 30);
        let step = cpu.step(&mut mem);
        assert_eq!((cpu.a, cpu.b, step.cycles), (0, 6, 28));
    }

    #[test]
    fn divd_zero_traps_to_fff0() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x1234);
        cpu.cc = Flags::N | Flags::V | Flags::C;
        let mut mem = program(&[0x11, 0x8D, 0x00]);
        mem.write16(0xFFF0, 0x0600);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVD");
        assert_eq!(step.trap, Some(Trap::DivideByZero));
        assert_eq!(cpu.pc, 0x0600);
        assert_ne!(cpu.mode_reg & MD_DIV_ZERO, 0);
        assert_eq!(cpu.mode_reg & MD_ILLEGAL, 0);
        assert_eq!(mem.read16(0x0F00 - 2), 0x0103, "return address after the instruction");
        assert_eq!(cpu.get_reg16(Reg16::D), 0x1234);
        assert!(!cpu.cc.intersects(Flags::I | Flags::F));
        // Z=1 N=0 V=0, C unchanged, before CC is stacked (hoglet67).
        assert_eq!(cpu.cc, Flags::E | Flags::Z | Flags::C);
        assert_eq!(mem.read8(cpu.s), (Flags::E | Flags::Z | Flags::C).bits());
        assert_eq!(step.cycles, 23, "stacking included (hoglet67)");
        assert_eq!(cpu.total_cycles, 23);
    }

    #[test]
    fn divd_zero_in_native_mode() {
        let mut cpu = native6309();
        let mut mem = program(&[0x11, 0x9D, 0x20]);
        mem.write16(0xFFF0, 0x0600);
        let step = cpu.step(&mut mem);
        assert_eq!(step.cycles, 26);
        assert_eq!(cpu.s, 0x0F00 - 14, "W stacked in native mode");
    }

    #[test]
    fn divq_divides_q_register() {
        let mut cpu = cpu6309();
        cpu.set_q(100);
        cpu.cc = Flags::V;
        let mut mem = program(&[0x11, 0x8E, 0x00, 0x07]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVQ");
        assert_eq!(cpu.w, 14);
        assert_eq!(cpu.get_reg16(Reg16::D), 2);
        assert!(!cpu.cc.intersects(Flags::V | Flags::C | Flags::N | Flags::Z), "success clears V");
        assert_eq!(step.cycles, 34);
    }

    #[test]
    fn divq_signs_and_overflows() {
        // (Q, divisor, W, D, N, V, C, cycles)
        type Case = (u32, u16, u16, u16, bool, bool, bool, u32);
        let cases: [Case; 5] = [
            (0xFFFF_FF9C, 7, 0xFFF2, 0xFFFE, true, false, false, 35), // -100 / 7 = -14 r -2
            (100, 0xFFF9, 0xFFF2, 2, true, false, false, 35),         // 100 / -7
            (0x0001_0000, 2, 0x8000, 0, true, true, false, 34),       // soft overflow
            (0x0000_0003, 2, 1, 1, false, false, true, 34),           // C = quotient odd
            (0, 3, 0, 0, false, false, false, 34),
        ];
        for (q, divisor, w, d, n, v, c, cycles) in cases {
            let mut cpu = cpu6309();
            cpu.set_q(q);
            let mut mem = program(&[0x11, 0x8E, (divisor >> 8) as u8, divisor as u8]);
            let step = cpu.step(&mut mem);
            let what = format!("{q:08X}/{divisor:04X}");
            assert_eq!((cpu.w, cpu.get_reg16(Reg16::D)), (w, d), "{what}");
            assert_eq!(cpu.cc.contains(Flags::N), n, "N {what}");
            assert_eq!(cpu.cc.contains(Flags::V), v, "V {what}");
            assert_eq!(cpu.cc.contains(Flags::C), c, "C {what}");
            assert_eq!(cpu.cc.contains(Flags::Z), w == 0 && !v, "Z {what}");
            assert_eq!(step.cycles, cycles, "cycles {what}");
        }

        // Range overflow: V (never C), Q unchanged, 21 cycles saved.
        let mut cpu = cpu6309();
        cpu.set_q(0x7FFF_FFFF);
        let mut mem = program(&[0x11, 0x8E, 0x00, 0x01]);
        let step = cpu.step(&mut mem);
        assert_eq!(cpu.get_q(), 0x7FFF_FFFF);
        assert!(cpu.cc.contains(Flags::V));
        assert!(!cpu.cc.intersects(Flags::C | Flags::N | Flags::Z));
        assert_eq!(step.cycles, 13);
    }

    #[test]
    fn divq_zero_traps_and_direct_form_reads_a_word() {
        let mut cpu = cpu6309();
        let mut mem = program(&[0x11, 0x8E, 0x00, 0x00]);
        mem.write16(0xFFF0, 0x0600);
        let step = cpu.step(&mut mem);
        assert_eq!(step.trap, Some(DIV_ZERO_TRAP));
        assert_ne!(cpu.mode_reg & MD_DIV_ZERO, 0);
        assert_eq!(step.cycles, 24);

        let mut cpu = cpu6309();
        cpu.set_q(1000);
        let mut mem = program(&[0x11, 0x9E, 0x20]);
        mem.write16(0x0020, 0x0100);
        let step = cpu.step(&mut mem);
        assert_eq!((cpu.w, cpu.get_reg16(Reg16::D)), (3, 232));
        assert_eq!(step.cycles, 36);
    }

    #[test]
    fn muld_on_6309() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0xFFFE); // -2
        cpu.cc = Flags::V | Flags::C;
        let mut mem = program(&[0x11, 0x8F, 0x00, 0x03]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "MULD");
        assert_eq!(cpu.get_q(), 0xFFFF_FFFA, "signed -2 * 3");
        assert!(cpu.cc.contains(Flags::N));
        assert!(!cpu.cc.contains(Flags::Z));
        assert!(cpu.cc.contains(Flags::V | Flags::C), "V and C unchanged (hoglet67)");
        assert_eq!(step.cycles, 30, "28 + negative operand + negative product");

        for (d, m, q) in [(0x7FFFu16, 0x7FFFu16, 0x3FFF_0001u32), (0x8000, 0x8000, 0x4000_0000), (10, 10, 100)] {
            let mut cpu = cpu6309();
            cpu.set_reg16(Reg16::D, d);
            let mut mem = program(&[0x11, 0x8F, (m >> 8) as u8, m as u8]);
            cpu.step(&mut mem);
            assert_eq!(cpu.get_q(), q);
        }
    }

    #[test]
    fn muld_timing_depends_on_the_signs() {
        // (D, operand, cycles): +1 per negative operand, +1 for a negative product.
        let cases = [(5u16, 3u16, 28), (0xFFFB, 3, 30), (5, 0xFFFD, 30), (0xFFFB, 0xFFFD, 30), (0, 0xFFFF, 30)];
        for (d, m, cycles) in cases {
            let mut cpu = cpu6309();
            cpu.set_reg16(Reg16::D, d);
            let mut mem = program(&[0x11, 0x8F, (m >> 8) as u8, m as u8]);
            assert_eq!(cpu.step(&mut mem).cycles, cycles, "{d:04X} * {m:04X}");
        }
        let mut cpu = native6309();
        cpu.set_reg16(Reg16::D, 0xFFFB);
        let mut mem = program(&[0x11, 0x9F, 0x20]);
        mem.write16(0x0020, 0x0003);
        assert_eq!(cpu.step(&mut mem).cycles, 31, "native 29 + 2");
    }

    #[test]
    fn muld_memory_forms_and_z_from_the_upper_word() {
        let mut cpu = cpu6309();
        cpu.x = 0x0300;
        let mut mem = program(&[0x11, 0x9F, 0x20, 0x11, 0xAF, 0x84, 0x11, 0xBF, 0x12, 0x34]);
        mem.write16(0x0020, 0x0003);
        mem.write16(0x0300, 0x0000);
        mem.write16(0x1234, 0xFFFF);
        cpu.set_reg16(Reg16::D, 7);
        assert_eq!(cpu.step(&mut mem).cycles, 30);
        assert_eq!(cpu.get_q(), 21);
        assert!(cpu.cc.contains(Flags::Z), "Z reflects the upper 16 bits only");
        cpu.set_reg16(Reg16::D, 0x7000);
        assert_eq!(cpu.step(&mut mem).cycles, 30);
        assert!(cpu.cc.contains(Flags::Z));
        cpu.set_reg16(Reg16::D, 7);
        assert_eq!(cpu.step(&mut mem).cycles, 33, "31 + negative operand + negative product");
        assert_eq!(cpu.get_q(), 0xFFFF_FFF9);
        assert!(cpu.cc.contains(Flags::N));
        assert!(!cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn muld_with_immediate_operand() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 10);
        let mut mem = program(&[0x11, 0x8F, 0x00, 0x0A]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "MULD");
        assert_eq!(cpu.w, 100);
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
    }

    // ── Bit transfers ────────────────────────────────────────────────────

    #[test]
    fn bor_bit_transfer() {
        // BOR A,5,0,<$20: A.bit0 |= mem.bit5 (postbyte 01 101 000).
        let mut cpu = cpu6309();
        cpu.a = 0x00;
        let mut mem = program(&[0x11, 0x32, 0x68, 0x20]);
        mem.write8(0x0020, 0x20);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "BOR");
        assert_eq!(step.operands, "A,5,0,<$20");
        assert_eq!(cpu.a, 0x01);
        assert_eq!(mem.read8(0x0020), 0x20, "only STBT writes memory");
        assert_eq!(step.cycles, 7);
    }

    #[test]
    fn bior_changes_exactly_one_register_bit() {
        // BIOR CC,0,0,<$20: C |= !mem.bit0.
        let mut cpu = cpu6309();
        cpu.cc = Flags::from_byte(0x50);
        let mut mem = program(&[0x11, 0x33, 0x00, 0x20]);
        mem.write8(0x0020, 0x00);
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0020), 0x00);
        assert_eq!(cpu.cc.bits(), 0x51);
    }

    #[test]
    fn bit_operations_combine_memory_into_register_bit() {
        // (opcode, register bit, memory bit, result)
        let table: [(u8, bool, bool, bool); 14] = [
            (0x30, true, true, true), // BAND
            (0x30, true, false, false),
            (0x31, true, false, true), // BIAND
            (0x31, true, true, false),
            (0x32, false, true, true), // BOR
            (0x32, false, false, false),
            (0x33, false, false, true), // BIOR
            (0x33, false, true, false),
            (0x34, true, true, false), // BEOR
            (0x34, false, true, true),
            (0x35, true, true, true), // BIEOR
            (0x35, true, false, false),
            (0x36, true, false, false), // LDBT
            (0x36, false, true, true),
        ];
        for (opcode, reg_bit, mem_bit, result) in table {
            let mut cpu = cpu6309();
            // register B (10), memory bit 6, register bit 3
            cpu.b = if reg_bit { 0x08 | 0x81 } else { 0x81 };
            let mut mem = program(&[0x11, opcode, 0b10_110_011, 0x20]);
            mem.write8(0x0020, if mem_bit { 0x40 } else { 0xBF });
            cpu.step(&mut mem);
            assert_eq!(cpu.b & 0x08 != 0, result, "$11{opcode:02X} {reg_bit} {mem_bit}");
            assert_eq!(cpu.b & !0x08, 0x81, "other bits untouched");
        }
    }

    #[test]
    fn stbt_stores_register_bit_into_memory_bit() {
        // STBT B,7,2,<$20: mem.bit2 = B.bit7 (postbyte 10 111 010).
        let mut cpu = cpu6309();
        cpu.b = 0x80;
        cpu.cc = Flags::Z | Flags::N | Flags::V | Flags::C;
        let mut mem = program(&[0x11, 0x37, 0xBA, 0x20, 0x11, 0x37, 0xBA, 0x20, 0x11, 0x37, 0xBA, 0x20]);
        mem.write8(0x0020, 0x00);
        let step = cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0020), 0x04);
        assert_eq!(cpu.b, 0x80);
        assert_eq!(step.cycles, 8);
        assert_eq!(step.operands, "B,7,2,<$20");
        // N and Z follow the stored byte; V and C are not affected (hoglet67).
        assert_eq!(cpu.cc, Flags::V | Flags::C);
        cpu.b = 0x00;
        mem.write8(0x0020, 0xFF);
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0020), 0xFB);
        assert_eq!(cpu.cc, Flags::N | Flags::V | Flags::C);
        mem.write8(0x0020, 0x04);
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0020), 0x00);
        assert_eq!(cpu.cc, Flags::Z | Flags::V | Flags::C);
    }

    #[test]
    fn bit_register_field_3_does_not_panic() {
        let mut cpu = cpu6309();
        cpu.a = 0x12;
        cpu.b = 0x34;
        let cc = cpu.cc;
        let mut mem = program(&[0x11, 0x33, 0xC0, 0x20, 0x11, 0x37, 0xFF, 0x20]);
        mem.write8(0x0020, 0xFF);
        cpu.step(&mut mem);
        assert_eq!((cpu.a, cpu.b, cpu.cc), (0x12, 0x34, cc));
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0020), 0x7F, "STBT from register 3 stores 0");
    }

    // ── MD register ──────────────────────────────────────────────────────

    #[test]
    fn bitmd_tests_and_clears_only_status_bits() {
        let mut cpu = cpu6309();
        cpu.mode_reg = MD_ILLEGAL | MD_DIV_ZERO | MD_NATIVE;
        cpu.cc = Flags::V | Flags::C | Flags::N;
        let mut mem = program(&[0x11, 0x3C, 0x40, 0x11, 0x3C, 0x40, 0x11, 0x3C, 0x80, 0x11, 0x3C, 0x03]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "BITMD");
        assert!(!cpu.cc.contains(Flags::Z));
        assert_eq!(cpu.cc, Flags::V | Flags::C | Flags::N, "only Z is affected");
        assert_eq!(cpu.mode_reg, MD_DIV_ZERO | MD_NATIVE);
        cpu.step(&mut mem);
        assert!(cpu.cc.contains(Flags::Z), "bit 6 was cleared by the first BITMD");
        cpu.cc = Flags::empty();
        cpu.step(&mut mem);
        assert_eq!(cpu.cc, Flags::empty(), "bit 7 set: Z clear, N not affected");
        assert_eq!(cpu.mode_reg, MD_NATIVE, "native mode survives");
        cpu.step(&mut mem);
        assert!(cpu.cc.contains(Flags::Z), "NM/FM are not tested");
        assert_eq!(cpu.mode_reg, MD_NATIVE);
    }

    #[test]
    fn bitmd_clears_trap_bits() {
        let mut cpu = cpu6309();
        cpu.mode_reg = 0xC0;
        let mut mem = program(&[0x11, 0x3C, 0xFF]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "BITMD");
        assert_eq!(cpu.mode_reg, 0);
        assert_eq!(step.cycles, 4);
    }

    #[test]
    fn ldmd_writes_only_nm_and_fm() {
        let mut cpu = cpu6309();
        cpu.mode_reg = MD_ILLEGAL | MD_DIV_ZERO;
        cpu.cc = Flags::from_byte(0x5A);
        let mut mem = program(&[0x11, 0x3D, 0xFF, 0x11, 0x3D, 0x00]);
        let step = cpu.step(&mut mem);
        assert_eq!(cpu.mode_reg, 0xC3);
        assert_eq!(step.cycles, 5);
        assert_eq!(cpu.cc.bits(), 0x5A, "flags not affected");
        let step = cpu.step(&mut mem);
        assert_eq!(cpu.mode_reg, 0xC0);
        assert_eq!(step.cycles, 5);
    }

    #[test]
    fn standard_trap_handler_returns_in_native_mode() {
        // Illegal opcode in native mode; handler: BITMD #$40 / RTI.
        let mut cpu = native6309();
        cpu.w = 0xBEEF;
        cpu.x = 0x1111;
        cpu.u = 0x2222;
        let mut mem = program(&[0x87, 0x12]);
        mem.load_binary(0x0500, &[0x11, 0x3C, 0x40, 0x3B]).unwrap();
        mem.write16(0xFFF0, 0x0500);
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0500);
        cpu.w = 0;
        cpu.x = 0;
        cpu.u = 0;
        cpu.step(&mut mem);
        assert!(!cpu.cc.contains(Flags::Z), "illegal instruction flagged");
        assert_eq!(cpu.mode_reg, MD_NATIVE);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "RTI");
        assert_eq!(cpu.pc, 0x0101);
        assert_eq!(cpu.s, 0x0F00);
        assert_eq!((cpu.w, cpu.x, cpu.u), (0xBEEF, 0x1111, 0x2222));
        assert_eq!(cpu.step(&mut mem).mnemonic, "NOP");
    }

    // ── Loads, stores, SEXW, E/F ─────────────────────────────────────────

    #[test]
    fn loads_clear_v_except_ldq() {
        let mut cpu = cpu6309();
        // LDW #$1234 ; LDE #$80 ; LDF #$00 ; LDQ #0 ; LDW <$20
        let mut mem = program(&[
            0x10, 0x86, 0x12, 0x34, 0x11, 0x86, 0x80, 0x11, 0xC6, 0x00, 0xCD, 0, 0, 0, 0, 0x10, 0x96,
            0x20,
        ]);
        // LDQ leaves V alone on the chip (hoglet67 random testing).
        let expect = [(4, "LDW", false), (3, "LDE", false), (3, "LDF", false), (5, "LDQ", true), (6, "LDW", false)];
        for (cycles, name, v) in expect {
            cpu.cc.insert(Flags::V);
            let step = cpu.step(&mut mem);
            assert_eq!(step.mnemonic, name);
            assert_eq!(step.cycles, cycles, "{name}");
            assert_eq!(cpu.cc.contains(Flags::V), v, "{name} V");
        }
        assert!(cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn ldq_stq_set_z_from_the_upper_word_and_keep_v() {
        // (Q, N, Z)
        for (q, n, z) in [(0x0000_0001u32, false, true), (0x0001_0000, false, false), (0x8000_0000, true, false), (0, false, true)] {
            let mut cpu = cpu6309();
            cpu.cc = Flags::V | Flags::C;
            let mut mem = program(&[0xCD, (q >> 24) as u8, (q >> 16) as u8, (q >> 8) as u8, q as u8]);
            cpu.step(&mut mem);
            assert_eq!(cpu.get_q(), q);
            assert_eq!((cpu.cc.contains(Flags::N), cpu.cc.contains(Flags::Z)), (n, z), "LDQ #${q:08X}");
            assert!(cpu.cc.contains(Flags::V | Flags::C), "LDQ keeps V and C");

            let mut mem = program(&[0x10, 0xDD, 0x20]);
            cpu.pc = 0x0100;
            cpu.cc = Flags::V | Flags::C | if z { Flags::empty() } else { Flags::Z };
            cpu.step(&mut mem);
            assert_eq!(mem.read16(0x0020), (q >> 16) as u16);
            assert_eq!((cpu.cc.contains(Flags::N), cpu.cc.contains(Flags::Z)), (n, z), "STQ ${q:08X}");
            assert!(cpu.cc.contains(Flags::V | Flags::C), "STQ keeps V and C");
        }
    }

    #[test]
    fn stw_stq_ldq_emulation_cycles() {
        let mut cpu = cpu6309();
        cpu.w = 0xCAFE;
        cpu.set_reg16(Reg16::D, 0x1234);
        cpu.x = 0x0300;
        // STW <$20 ; STW $0400 ; STQ ,X ; LDQ <$20 ; LDQ $0300 ; STQ $0500
        let mut mem = program(&[
            0x10, 0x97, 0x20, 0x10, 0xB7, 0x04, 0x00, 0x10, 0xED, 0x84, 0x10, 0xDC, 0x20, 0x10, 0xFC,
            0x03, 0x00, 0x10, 0xFD, 0x05, 0x00,
        ]);
        let cycles: Vec<u32> = (0..6).map(|_| cpu.step(&mut mem).cycles).collect();
        assert_eq!(cycles, vec![6, 7, 8, 8, 9, 9]);
        assert_eq!(mem.read16(0x0020), 0xCAFE);
        assert_eq!(mem.read16(0x0400), 0xCAFE);
        assert_eq!(mem.read16(0x0500), 0x1234);
        assert_eq!(mem.read16(0x0502), 0xCAFE);
    }

    #[test]
    fn sexw_sets_z_from_q() {
        for (w, d, n, z) in [(0x0000u16, 0x0000u16, false, true), (0x0001, 0, false, false), (0x8000, 0xFFFF, true, false)] {
            let mut cpu = cpu6309();
            cpu.w = w;
            cpu.cc = Flags::V;
            let mut mem = program(&[0x14]);
            let step = cpu.step(&mut mem);
            assert_eq!(step.mnemonic, "SEXW");
            assert_eq!(cpu.get_reg16(Reg16::D), d);
            assert_eq!(cpu.w, w);
            assert_eq!(cpu.cc.contains(Flags::N), n);
            assert_eq!(cpu.cc.contains(Flags::Z), z, "W={w:04X}");
            assert!(cpu.cc.contains(Flags::V), "V unchanged");
            assert_eq!(step.cycles, 4);
        }
    }

    #[test]
    fn sube_cmpf_leave_h_alone() {
        let mut cpu = cpu6309();
        cpu.w = 0x1000;
        cpu.cc = Flags::H;
        let mut mem = program(&[0x11, 0x80, 0x01, 0x11, 0xC1, 0x01]);
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0x0F00);
        assert!(cpu.cc.contains(Flags::H));
        cpu.cc = Flags::empty();
        cpu.step(&mut mem);
        assert!(!cpu.cc.contains(Flags::H));
        assert!(cpu.cc.contains(Flags::C), "F=0 minus 1 borrows");
    }

    #[test]
    fn word_alu_indexed_operand_text() {
        let mut cpu = cpu6309();
        cpu.x = 0x0300;
        // SUBW ,X ; ADDW 16,X ; CMPW $010B,PCR
        let mut mem = program(&[0x10, 0xA0, 0x84, 0x10, 0xAB, 0x88, 0x10, 0x10, 0xA1, 0x8C, 0x00]);
        let texts: Vec<String> = (0..3)
            .map(|_| {
                let s = cpu.step(&mut mem);
                format!("{} {}", s.mnemonic, s.operands)
            })
            .collect();
        assert_eq!(texts, vec!["SUBW ,X", "ADDW 16,X", "CMPW $010B,PCR"]);
    }

    #[test]
    fn negd_operates_on_d_not_w() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x0005);
        cpu.w = 0x1234;
        let mut mem = program(&[0x10, 0x40]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NEGD");
        assert_eq!(cpu.get_reg16(Reg16::D), 0xFFFB);
        assert_eq!(cpu.w, 0x1234);
    }

    #[test]
    fn w_inherent_opcodes() {
        let mut cpu = cpu6309();
        // COMW ; INCW ; DECW ; LSRW ; RORW ; ROLW ; TSTW ; CLRW
        let mut mem = program(&[
            0x10, 0x53, 0x10, 0x5C, 0x10, 0x5A, 0x10, 0x54, 0x10, 0x56, 0x10, 0x59, 0x10, 0x5D, 0x10,
            0x5F,
        ]);
        cpu.w = 0x0006;
        cpu.cc = Flags::empty();
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0xFFF9);
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0xFFFA);
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0xFFF9);
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0x7FFC);
        assert!(cpu.cc.contains(Flags::C));
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0xBFFE);
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0x7FFC);
        assert!(cpu.cc.contains(Flags::C));
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "TSTW");
        assert!(!cpu.cc.contains(Flags::N));
        cpu.step(&mut mem);
        assert_eq!(cpu.w, 0);
        assert!(cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn negw_asrw_aslw_trap_like_the_chip() {
        // MAME implements $1050/$1057/$1058; real HD6309s trap (hoglet67).
        for op in [0x50u8, 0x57, 0x58] {
            let mut cpu = cpu6309();
            cpu.w = 0x0006;
            let mut mem = program(&[0x10, op]);
            mem.write16(0xFFF0, 0x0600);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode), "$10{op:02X}");
            assert_eq!(cpu.w, 0x0006);
            assert_eq!(cpu.pc, 0x0600);
        }
    }

    #[test]
    fn andd_imm_masks_d_register() {
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0xFF0F);
        let mut mem = program(&[0x10, 0x84, 0x00, 0xF0]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "ANDD");
        assert_eq!(cpu.get_reg16(Reg16::D), 0x0000);
        assert_eq!(step.cycles, 5);
    }

    #[test]
    fn e_f_loads_and_come() {
        let mut cpu = cpu6309();
        let mut mem = program(&[0x11, 0x86, 0xAB, 0x11, 0xC6, 0xCD, 0x11, 0x43]);
        assert_eq!(cpu.step(&mut mem).mnemonic, "LDE");
        assert_eq!(cpu.w, 0xAB00);
        assert_eq!(cpu.step(&mut mem).mnemonic, "LDF");
        assert_eq!(cpu.w, 0xABCD);
        assert_eq!(cpu.step(&mut mem).mnemonic, "COME");
        assert_eq!(cpu.w, 0x54CD);
    }

    #[test]
    fn leax_works_on_both_cpus() {
        for variant in [CpuVariant::Mc6809, CpuVariant::Hd6309] {
            let mut cpu = Cpu::new();
            cpu.variant = variant;
            cpu.pc = 0x0100;
            cpu.x = 0x0200;
            let mut mem = program(&[0x30, 0x05]);
            let step = cpu.step(&mut mem);
            assert_eq!(step.mnemonic, "LEAX");
            assert_eq!(cpu.x, 0x0205);
            assert_eq!(step.trap, None);
        }
    }

    #[test]
    fn pshsw_pulsw_pshuw_puluw_roundtrip() {
        let mut cpu = cpu6309();
        cpu.w = 0xBEEF;
        cpu.s = 0x0200;
        cpu.u = 0x0300;
        let mut mem = program(&[0x10, 0x38, 0x10, 0x39, 0x10, 0x3A, 0x10, 0x3B]);
        let push = cpu.step(&mut mem);
        assert_eq!(push.mnemonic, "PSHSW");
        assert_eq!(push.cycles, 6);
        assert_eq!(mem.read16(0x01FE), 0xBEEF);
        cpu.w = 0;
        assert_eq!(cpu.step(&mut mem).mnemonic, "PULSW");
        assert_eq!(cpu.w, 0xBEEF);
        cpu.w = 0xCAFE;
        assert_eq!(cpu.step(&mut mem).mnemonic, "PSHUW");
        assert_eq!(mem.read16(0x02FE), 0xCAFE);
        cpu.w = 0;
        assert_eq!(cpu.step(&mut mem).mnemonic, "PULUW");
        assert_eq!(cpu.w, 0xCAFE);
        assert_eq!((cpu.s, cpu.u), (0x0200, 0x0300));
    }

    #[test]
    fn ldq_stq_roundtrip() {
        let mut cpu = cpu6309();
        let mut mem = program(&[0xCD, 0x00, 0x01, 0x00, 0x02, 0x10, 0xDD, 0x20]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LDQ");
        assert_eq!(step.operands, "#$00010002");
        assert_eq!((cpu.get_reg16(Reg16::D), cpu.w), (0x0001, 0x0002));
        assert_eq!(cpu.step(&mut mem).mnemonic, "STQ");
        assert_eq!(mem.read16(0x0020), 0x0001);
        assert_eq!(mem.read16(0x0022), 0x0002);
    }

    // ── TFM ──────────────────────────────────────────────────────────────

    /// Step a TFM until it has finished: (steps, cycles).
    fn finish(cpu: &mut Cpu, mem: &mut Memory) -> (u32, u32) {
        let mut steps = 0;
        let mut cycles = 0;
        loop {
            let step = cpu.step(mem);
            steps += 1;
            cycles += step.cycles;
            if cpu.tfm_pending.is_none() {
                return (steps, cycles);
            }
            assert_eq!(step.mnemonic, "TFM");
            assert!(steps < 100_000);
        }
    }

    #[test]
    fn tfm_on_page3() {
        let mut cpu = cpu6309();
        cpu.x = 0x600;
        cpu.y = 0x700;
        cpu.w = 2;
        let mut mem = program(&[0x11, 0x38, 0x12]);
        mem.write8(0x600, 0x41);
        mem.write8(0x601, 0x42);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "TFM");
        assert_eq!(step.operands, "X+,Y+");
        assert_eq!(step.cycles, 9, "6 + one byte");
        assert!(cpu.tfm_pending.is_some());
        assert_eq!((cpu.x, cpu.y, cpu.w, cpu.pc), (0x601, 0x701, 1, 0x0100));
        let step = cpu.step(&mut mem);
        assert_eq!(step.cycles, 3);
        assert_eq!(mem.read8(0x700), 0x41);
        assert_eq!(mem.read8(0x701), 0x42);
        assert_eq!((cpu.x, cpu.y, cpu.w), (0x602, 0x702, 0));
        assert_eq!(cpu.pc, 0x0103);
        assert!(cpu.tfm_pending.is_none());
        assert_eq!(cpu.total_cycles, 12);
    }

    #[test]
    fn tfm_forms() {
        // TFM X-,Y-
        let mut cpu = cpu6309();
        cpu.x = 0x0601;
        cpu.y = 0x0701;
        cpu.w = 2;
        let mut mem = program(&[0x11, 0x39, 0x12]);
        mem.write8(0x0600, 0xA0);
        mem.write8(0x0601, 0xA1);
        assert_eq!(finish(&mut cpu, &mut mem), (2, 12));
        assert_eq!((mem.read8(0x0700), mem.read8(0x0701)), (0xA0, 0xA1));
        assert_eq!((cpu.x, cpu.y), (0x05FF, 0x06FF));

        // TFM X+,Y: read a block into one address
        let mut cpu = cpu6309();
        cpu.x = 0x0600;
        cpu.y = 0x0700;
        cpu.w = 3;
        let mut mem = program(&[0x11, 0x3A, 0x12]);
        mem.load_binary(0x0600, &[1, 2, 3]).unwrap();
        assert_eq!(cpu.step(&mut mem).operands, "X+,Y");
        finish(&mut cpu, &mut mem);
        assert_eq!(mem.read8(0x0700), 3);
        assert_eq!((cpu.x, cpu.y), (0x0603, 0x0700));

        // TFM X,Y+: fill
        let mut cpu = cpu6309();
        cpu.x = 0x0600;
        cpu.y = 0x0700;
        cpu.w = 3;
        let mut mem = program(&[0x11, 0x3B, 0x12]);
        mem.write8(0x0600, 0x55);
        assert_eq!(cpu.step(&mut mem).operands, "X,Y+");
        finish(&mut cpu, &mut mem);
        assert_eq!(mem.export_range(0x0700, 3).unwrap(), vec![0x55; 3]);
        assert_eq!((cpu.x, cpu.y), (0x0600, 0x0703));

        // TFM D+,U+: D is a legal pointer
        let mut cpu = cpu6309();
        cpu.set_reg16(Reg16::D, 0x0600);
        cpu.u = 0x0800;
        cpu.w = 2;
        let mut mem = program(&[0x11, 0x38, 0x03]);
        mem.load_binary(0x0600, &[7, 8]).unwrap();
        finish(&mut cpu, &mut mem);
        assert_eq!(mem.export_range(0x0800, 2).unwrap(), vec![7, 8]);
        assert_eq!((cpu.get_reg16(Reg16::D), cpu.u), (0x0602, 0x0802));
    }

    #[test]
    fn tfm_same_register_steps_twice_per_byte() {
        // TFM X+,X+ (MAME): read [X], X++, write [X], X++.
        let mut cpu = cpu6309();
        cpu.x = 0x0600;
        cpu.w = 2;
        let mut mem = program(&[0x11, 0x38, 0x11]);
        mem.load_binary(0x0600, &[0xAA, 0x11, 0x22, 0x33]).unwrap();
        finish(&mut cpu, &mut mem);
        assert_eq!(mem.export_range(0x0600, 4).unwrap(), vec![0xAA, 0xAA, 0x22, 0x22]);
        assert_eq!(cpu.x, 0x0604);
    }

    #[test]
    fn tfm_with_zero_count_does_nothing() {
        let mut cpu = cpu6309();
        cpu.x = 0x0600;
        cpu.y = 0x0700;
        cpu.w = 0;
        cpu.cc = Flags::empty();
        let mut mem = program(&[0x11, 0x38, 0x12]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.cycles, 6);
        assert_eq!((cpu.x, cpu.y, cpu.pc), (0x0600, 0x0700, 0x0103));
        assert!(cpu.tfm_pending.is_none());
        assert!(cpu.cc.contains(Flags::Z), "Z = (W == 0)");
    }

    #[test]
    fn tfm_sets_z_from_the_remaining_count() {
        let mut cpu = cpu6309();
        cpu.x = 0x0600;
        cpu.y = 0x0700;
        cpu.w = 3;
        cpu.cc = Flags::Z | Flags::C;
        let mut mem = program(&[0x11, 0x38, 0x12]);
        cpu.step(&mut mem);
        assert!(!cpu.cc.contains(Flags::Z), "2 bytes left");
        finish(&mut cpu, &mut mem);
        assert_eq!(cpu.cc, Flags::Z | Flags::C, "only Z is affected");
    }

    #[test]
    fn tfm_rejects_other_registers() {
        for postbyte in [0x51u8, 0x15, 0x16, 0x61, 0x81, 0xC1] {
            let mut cpu = cpu6309();
            cpu.w = 0; // traps even without a transfer
            cpu.x = 0x0600;
            cpu.cc = Flags::empty();
            let mut mem = program(&[0x11, 0x38, postbyte]);
            mem.write16(0xFFF0, 0x0600);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode), "postbyte {postbyte:02X}");
            assert_eq!(step.cycles, 23);
            assert_eq!(cpu.pc, 0x0600);
            assert_ne!(cpu.mode_reg & MD_ILLEGAL, 0);
            assert!(cpu.tfm_pending.is_none());
            // Z = (W == 0) is set before CC is stacked (hoglet67).
            assert_ne!(mem.read8(cpu.s) & 0x04, 0);
        }
        let mut cpu = native6309();
        cpu.w = 5;
        cpu.cc = Flags::Z;
        let mut mem = program(&[0x11, 0x38, 0x51]);
        assert_eq!(cpu.step(&mut mem).cycles, 25);
        assert_eq!(mem.read8(cpu.s) & 0x04, 0, "W != 0: Z clear in the stacked CC");
    }

    #[test]
    fn tfm_large_transfer_takes_six_plus_three_per_byte() {
        let mut cpu = cpu6309();
        cpu.x = 0x1000;
        cpu.y = 0x2000;
        cpu.w = 600;
        let mut mem = program(&[0x11, 0x38, 0x12]);
        for i in 0..600u16 {
            mem.write8(0x1000 + i, (i & 0xFF) as u8);
        }

        let step1 = cpu.step(&mut mem);
        assert_eq!(step1.mnemonic, "TFM");
        assert_eq!(step1.cycles, 6 + 3);
        assert!(cpu.tfm_pending.is_some());
        // Registers and W are current after every byte; PC stays on the TFM.
        assert_eq!((cpu.w, cpu.x, cpu.y), (599, 0x1001, 0x2001));
        assert_eq!(cpu.pc, 0x0100);
        assert_eq!(step1.pc_after, 0x0100);

        let (steps, cycles) = finish(&mut cpu, &mut mem);
        assert_eq!((steps, cycles), (599, 3 * 599));
        assert_eq!(cpu.w, 0);
        assert_eq!(cpu.x, 0x1000 + 600);
        assert_eq!(cpu.y, 0x2000 + 600);
        assert_eq!(cpu.pc, 0x0103);
        assert_eq!(mem.read8(0x2000), 0);
        assert_eq!(mem.read8(0x2000 + 599), (599 & 0xFF) as u8);
        assert_eq!(cpu.total_cycles, 6 + 3 * 600);
    }

    #[test]
    fn tfm_takes_an_interrupt_after_any_byte_and_resumes() {
        let mut cpu = cpu6309();
        cpu.cc = Flags::empty();
        cpu.x = 0x1000;
        cpu.y = 0x2000;
        cpu.w = 600;
        let mut mem = program(&[0x11, 0x38, 0x12, 0x12]);
        mem.write8(0x0400, 0x3B); // IRQ handler: RTI
        mem.write16(0xFFF8, 0x0400);
        for i in 0..600u16 {
            mem.write8(0x1000 + i, (i % 251) as u8);
        }
        cpu.step(&mut mem);
        cpu.step(&mut mem);
        cpu.irq_pending = true;
        let irq = cpu.step(&mut mem);
        assert_eq!(irq.mnemonic, "IRQ");
        assert_eq!(mem.read16(cpu.s + 10), 0x0100, "stacked PC is the TFM");
        assert_eq!(cpu.w, 598, "interrupted after the second byte");
        assert_eq!(mem.read8(cpu.s) & 0x04, 0, "stacked Z = (W == 0)");
        assert_eq!(cpu.step(&mut mem).mnemonic, "RTI");
        assert_eq!(cpu.pc, 0x0100);
        let resumed = cpu.step(&mut mem);
        assert_eq!(resumed.mnemonic, "TFM");
        assert_eq!(resumed.cycles, 6 + 3, "the restarted TFM fetches its 3 bytes again");
        finish(&mut cpu, &mut mem);
        assert_eq!(cpu.w, 0);
        assert_eq!(cpu.pc, 0x0103);
        assert_eq!((cpu.x, cpu.y), (0x1000 + 600, 0x2000 + 600));
        for i in 0..600u16 {
            assert_eq!(mem.read8(0x2000 + i), (i % 251) as u8);
        }
    }

    #[test]
    fn tfm_is_abandoned_when_the_debugger_moves_pc() {
        let mut cpu = cpu6309();
        cpu.x = 0x1000;
        cpu.y = 0x2000;
        cpu.w = 1000;
        let mut mem = program(&[0x11, 0x38, 0x12]);
        mem.write8(0x0200, 0x12);
        cpu.step(&mut mem);
        assert!(cpu.tfm_pending.is_some());
        cpu.pc = 0x0200;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NOP");
        assert!(cpu.tfm_pending.is_none());
        assert_eq!(cpu.w, 999);
    }

    // ── Native-mode timing ───────────────────────────────────────────────

    /// Cycle counts of the HD6309 in native mode (Burke / hoglet67).
    #[test]
    fn native_mode_cycle_table() {
        let cases: &[(&[u8], u32, &str)] = &[
            (&[0x12], 1, "NOP"),
            (&[0x3A], 1, "ABX"),
            (&[0x3D], 10, "MUL"),
            (&[0x1D], 1, "SEX"),
            (&[0x19], 1, "DAA"),
            (&[0x4C], 1, "INCA"),
            (&[0x96, 0x20], 3, "LDA <"),
            (&[0x97, 0x20], 3, "STA <"),
            (&[0xB6, 0x12, 0x34], 4, "LDA ext"),
            (&[0x86, 0x01], 2, "LDA #"),
            (&[0xA6, 0x84], 4, "LDA ,X"),
            (&[0xA6, 0x05], 5, "LDA 5,X"),
            (&[0xA6, 0x81], 6, "LDA ,X++"),
            (&[0xA6, 0x80], 5, "LDA ,X+"),
            (&[0xA6, 0x8B], 6, "LDA D,X"),
            (&[0xA6, 0x89, 0x01, 0x00], 7, "LDA n16,X"),
            (&[0xA6, 0x8D, 0x01, 0x00], 7, "LDA n16,PCR"),
            (&[0xA6, 0x94], 7, "LDA [,X]"),
            (&[0xA6, 0x9F, 0x01, 0x00], 8, "LDA [n16]"),
            (&[0xA6, 0x8F], 4, "LDA ,W"),
            (&[0xA6, 0xAF, 0x00, 0x10], 6, "LDA n16,W"),
            (&[0xA6, 0xCF], 5, "LDA ,W++"),
            (&[0xA6, 0x8E], 5, "LDA W,X"),
            (&[0xCC, 0x12, 0x34], 3, "LDD #"),
            (&[0xDC, 0x20], 4, "LDD <"),
            (&[0xFC, 0x12, 0x34], 5, "LDD ext"),
            (&[0xEC, 0x84], 5, "LDD ,X"),
            (&[0xF3, 0x12, 0x34], 5, "ADDD ext"),
            (&[0xC3, 0x00, 0x01], 3, "ADDD #"),
            (&[0x93, 0x20], 4, "SUBD <"),
            (&[0xAC, 0x84], 5, "CMPX ,X"),
            (&[0xBF, 0x12, 0x34], 5, "STX ext"),
            (&[0x34, 0x06], 6, "PSHS A,B"),
            (&[0x35, 0x06], 6, "PULS A,B"),
            (&[0x39], 4, "RTS"),
            (&[0xBD, 0x12, 0x34], 7, "JSR ext"),
            (&[0x9D, 0x20], 6, "JSR <"),
            (&[0xAD, 0x84], 6, "JSR ,X"),
            (&[0x0E, 0x20], 2, "JMP <"),
            (&[0x7E, 0x12, 0x34], 3, "JMP ext"),
            (&[0x6E, 0x84], 3, "JMP ,X"),
            (&[0x8D, 0x00], 6, "BSR"),
            (&[0x16, 0x00, 0x00], 4, "LBRA"),
            (&[0x17, 0x00, 0x00], 7, "LBSR"),
            (&[0x20, 0x00], 3, "BRA"),
            (&[0x00, 0x20], 5, "NEG <"),
            (&[0x60, 0x84], 6, "NEG ,X"),
            (&[0x70, 0x12, 0x34], 6, "NEG ext"),
            (&[0x0D, 0x20], 4, "TST <"),
            (&[0x6D, 0x84], 5, "TST ,X"),
            (&[0x7D, 0x12, 0x34], 5, "TST ext"),
            (&[0x0F, 0x20], 5, "CLR <"),
            (&[0x30, 0x05], 5, "LEAX 5,X"),
            (&[0x1F, 0x12], 4, "TFR"),
            (&[0x1E, 0x12], 5, "EXG"),
            (&[0x1A, 0x00], 3, "ORCC"),
            (&[0x3F], 21, "SWI"),
            (&[0x02, 0xF0, 0x20], 6, "AIM <"),
            (&[0x62, 0xF0, 0x84], 7, "AIM ,X"),
            (&[0x0B, 0xF0, 0x20], 4, "TIM <"),
            (&[0x7B, 0xF0, 0x12, 0x34], 5, "TIM ext"),
            (&[0x14], 4, "SEXW"),
            (&[0xCD, 0, 0, 0, 0], 5, "LDQ #"),
            (&[0x10, 0x83, 0x00, 0x01], 4, "CMPD #"),
            (&[0x10, 0x93, 0x20], 5, "CMPD <"),
            (&[0x10, 0xA3, 0x84], 6, "CMPD ,X"),
            (&[0x10, 0xB3, 0x12, 0x34], 6, "CMPD ext"),
            (&[0x10, 0x8E, 0x12, 0x34], 4, "LDY #"),
            (&[0x10, 0x9E, 0x20], 5, "LDY <"),
            (&[0x10, 0xBE, 0x12, 0x34], 6, "LDY ext"),
            (&[0x10, 0x9F, 0x20], 5, "STY <"),
            (&[0x10, 0xDE, 0x20], 5, "LDS <"),
            (&[0x10, 0xEE, 0x84], 6, "LDS ,X"),
            (&[0x10, 0x80, 0x00, 0x01], 4, "SUBW #"),
            (&[0x10, 0x86, 0x00, 0x01], 4, "LDW #"),
            (&[0x10, 0x96, 0x20], 5, "LDW <"),
            (&[0x10, 0xB7, 0x12, 0x34], 6, "STW ext"),
            (&[0x10, 0x99, 0x20], 5, "ADCD <"),
            (&[0x10, 0xA4, 0x84], 6, "ANDD ,X"),
            (&[0x10, 0x40], 2, "NEGD"),
            (&[0x10, 0x5D], 2, "TSTW"),
            (&[0x10, 0xDC, 0x20], 7, "LDQ <"),
            (&[0x10, 0xFC, 0x12, 0x34], 8, "LDQ ext"),
            (&[0x10, 0xED, 0x84], 8, "STQ ,X"),
            (&[0x10, 0x30, 0x12], 4, "ADDR"),
            (&[0x10, 0x38], 6, "PSHSW"),
            (&[0x10, 0x3F], 22, "SWI2"),
            (&[0x10, 0x27, 0x00, 0x00], 5, "LBEQ not taken"),
            (&[0x10, 0x26, 0x00, 0x00], 5, "LBNE taken"),
            (&[0x11, 0x83, 0x00, 0x01], 4, "CMPU #"),
            (&[0x11, 0x9C, 0x20], 5, "CMPS <"),
            (&[0x11, 0xB3, 0x12, 0x34], 6, "CMPU ext"),
            (&[0x11, 0x86, 0x01], 3, "LDE #"),
            (&[0x11, 0x96, 0x20], 4, "LDE <"),
            (&[0x11, 0xB7, 0x12, 0x34], 5, "STE ext"),
            (&[0x11, 0xE0, 0x84], 5, "SUBF ,X"),
            (&[0x11, 0x43], 2, "COME"),
            (&[0x11, 0x5F], 2, "CLRF"),
            (&[0x11, 0x30, 0x40, 0x20], 6, "BAND"),
            (&[0x11, 0x37, 0x40, 0x20], 7, "STBT"),
            (&[0x11, 0x3C, 0x40], 4, "BITMD"),
            (&[0x11, 0x3D, 0x01], 5, "LDMD"),
            (&[0x11, 0x8F, 0x00, 0x01], 28, "MULD #"),
            (&[0x11, 0x9F, 0x20], 29, "MULD <"),
            (&[0x11, 0xAF, 0x84], 30, "MULD ,X"),
            (&[0x11, 0xBF, 0x12, 0x34], 30, "MULD ext"),
            (&[0x11, 0x8D, 0x01], 25, "DIVD #"),
            (&[0x11, 0x9D, 0x20], 26, "DIVD <"),
            (&[0x11, 0xBD, 0x12, 0x34], 27, "DIVD ext"),
            (&[0x11, 0x9E, 0x20], 35, "DIVQ <"),
            (&[0x11, 0x3F], 22, "SWI3"),
            (&[0x87], 22, "illegal"),
            (&[0x11, 0x87], 23, "illegal page 3"),
        ];
        for (bytes, cycles, name) in cases {
            let mut cpu = native6309();
            cpu.x = 0x0300;
            cpu.w = 0x0400;
            cpu.cc = Flags::empty();
            cpu.set_reg16(Reg16::D, 0x0010);
            let mut mem = program(bytes);
            mem.write16(0x0020, 0x0101);
            mem.write16(0x0300, 0x0101);
            mem.write16(0x1234, 0x0101);
            let step = cpu.step(&mut mem);
            assert_eq!(step.cycles, *cycles, "{name}: {}", step.mnemonic);
            assert_eq!(cpu.total_cycles, u64::from(*cycles), "{name}");
        }
    }

    /// Indexed extra cycles of postbytes $80-$FF on a real HD6309 in emulation
    /// and native mode (hoglet67 6809Decoder, "Addendum to The 6309 Book");
    /// -1 marks the postbytes that trap. $00-$7F (n5,R) cost 1 in both modes.
    const HW_INDEX_EXTRA: [[[i8; 16]; 8]; 2] = [
        [
            [2, 3, 2, 3, 0, 1, 1, 1, 1, 4, 1, 4, 1, 5, 1, 0],
            [3, 6, -1, 6, 3, 4, 4, 4, 4, 7, 4, 7, 4, 8, 4, 5],
            [2, 3, 2, 3, 0, 1, 1, 1, 1, 4, 1, 4, 1, 5, 1, 2],
            [5, 6, -1, 6, 3, 4, 4, 4, 4, 7, 4, 7, 4, 8, 4, -1],
            [2, 3, 2, 3, 0, 1, 1, 1, 1, 4, 1, 4, 1, 5, 1, 1],
            [4, 6, -1, 6, 3, 4, 4, 4, 4, 7, 4, 7, 4, 8, 4, -1],
            [2, 3, 2, 3, 0, 1, 1, 1, 1, 4, 1, 4, 1, 5, 1, 1],
            [4, 6, -1, 6, 3, 4, 4, 4, 4, 7, 4, 7, 4, 8, 4, -1],
        ],
        [
            [1, 2, 1, 2, 0, 1, 1, 1, 1, 3, 1, 2, 1, 3, 1, 0],
            [3, 5, -1, 5, 3, 4, 4, 4, 4, 6, 4, 5, 4, 6, 4, 4],
            [1, 2, 1, 2, 0, 1, 1, 1, 1, 3, 1, 2, 1, 3, 1, 2],
            [5, 5, -1, 5, 3, 4, 4, 4, 4, 6, 4, 5, 4, 6, 4, -1],
            [1, 2, 1, 2, 0, 1, 1, 1, 1, 3, 1, 2, 1, 3, 1, 1],
            [4, 5, -1, 5, 3, 4, 4, 4, 4, 6, 4, 5, 4, 6, 4, -1],
            [1, 2, 1, 2, 0, 1, 1, 1, 1, 3, 1, 2, 1, 3, 1, 1],
            [4, 5, -1, 5, 3, 4, 4, 4, 4, 6, 4, 5, 4, 6, 4, -1],
        ],
    ];

    /// LDA indexed (4 cycles + postbyte extra) for every postbyte in both
    /// modes against the hardware table; illegal postbytes trap (21 / 23).
    #[test]
    fn indexed_extra_cycles_match_the_hardware_tables() {
        for postbyte in 0..=0xFFu8 {
            for native in [false, true] {
                let mut cpu = if native { native6309() } else { cpu6309() };
                cpu.x = 0x0300;
                cpu.y = 0x0300;
                cpu.u = 0x0300;
                cpu.s = 0x0300;
                cpu.w = 0x0300;
                let mut mem = program(&[0xA6, postbyte, 0x00, 0x10]);
                let step = cpu.step(&mut mem);
                let expected = if postbyte < 0x80 {
                    1
                } else {
                    HW_INDEX_EXTRA[usize::from(native)][usize::from((postbyte >> 4) & 7)]
                        [usize::from(postbyte & 0x0F)]
                };
                let what = format!("LDA with postbyte ${postbyte:02X} (native: {native})");
                if expected < 0 {
                    assert!(crate::addressing::is_illegal_hd6309_postbyte(postbyte), "{what}");
                    assert_eq!(step.trap, Some(Trap::IllegalOpcode), "{what}");
                    assert_eq!(step.cycles, if native { 23 } else { 21 }, "{what}");
                    continue;
                }
                assert_eq!(step.trap, None, "{what}");
                assert_eq!(step.cycles, 4 + expected as u32, "{what}");
                let helper = if native {
                    native_index_extra(postbyte)
                } else {
                    emulation_index_extra(postbyte)
                };
                assert_eq!(helper, i32::from(expected), "{what}");
            }
        }
    }

    #[test]
    fn illegal_indexed_postbyte_traps_without_side_effects() {
        for native in [false, true] {
            // STA [,-X]: [,-R] does not exist on the 6309.
            let mut cpu = if native { native6309() } else { cpu6309() };
            cpu.a = 0x42;
            cpu.x = 0x0400;
            cpu.cc = Flags::empty();
            let mut mem = program(&[0xA7, 0x92, 0x12]);
            mem.write16(0x03FF, 0x0500); // where [,-X] would point
            mem.write16(0xFFF0, 0x0600);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode));
            assert_eq!(step.mnemonic, "TRAP");
            assert_eq!(cpu.pc, 0x0600);
            assert_ne!(cpu.mode_reg & MD_ILLEGAL, 0);
            let frame = if native { 14 } else { 12 };
            assert_eq!(cpu.s, 0x0F00 - frame);
            assert_eq!(mem.read16(0x0F00 - 2), 0x0102, "stacked PC is past the postbyte");
            assert_eq!((cpu.a, cpu.x), (0x42, 0x0400), "A and X untouched");
            assert_eq!(mem.read8(0x0500), 0x00, "no store");
            assert_eq!(mem.read16(0x03FF), 0x0500);
            assert_eq!(step.cycles, if native { 23 } else { 21 });
            assert_eq!(cpu.total_cycles, u64::from(step.cycles));
        }

        // AIM #$01,[,-Y] and LDW [,-X]: one more byte fetched, one more cycle.
        for bytes in [[0x62u8, 0x01, 0xB2], [0x10, 0xA6, 0x92]] {
            let mut cpu = cpu6309();
            cpu.w = 0x1234;
            let mut mem = program(&bytes);
            let step = cpu.step(&mut mem);
            assert_eq!(step.trap, Some(Trap::IllegalOpcode), "{bytes:02X?}");
            assert_eq!(step.cycles, 22, "{bytes:02X?}");
            assert_eq!(mem.read16(0x0F00 - 2), 0x0103, "{bytes:02X?}");
            assert_eq!(cpu.w, 0x1234);
        }
    }

    /// Every opcode on every page with assorted operand bytes executes without
    /// panicking in both modes (a panic poisons the emulator mutex of the app).
    #[test]
    fn every_opcode_executes_without_panicking() {
        let operands: [[u8; 4]; 4] = [[0x00; 4], [0xFF; 4], [0xC0, 0x12, 0x34, 0x56], [0x3F, 0x80, 0x01, 0x10]];
        for prefix in [None, Some(0x10u8), Some(0x11)] {
            for opcode in 0..=0xFFu8 {
                for operand in operands {
                    for native in [false, true] {
                        let mut cpu = if native { native6309() } else { cpu6309() };
                        cpu.w = 0x0004;
                        cpu.set_reg16(Reg16::D, 0x8000);
                        let mut bytes: Vec<u8> = prefix.into_iter().collect();
                        bytes.push(opcode);
                        bytes.extend(operand);
                        let mut mem = program(&bytes);
                        mem.write16(0xFFF0, 0x0400);
                        let step = cpu.step(&mut mem);
                        assert!(step.cycles >= 1, "{bytes:02X?}");
                        assert!(!step.bytes.is_empty(), "{bytes:02X?}");
                        // finish a pending TFM
                        while cpu.tfm_pending.is_some() {
                            cpu.step(&mut mem);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn emulation_mode_keeps_datasheet_timing() {
        let cases: &[(&[u8], u32)] = &[
            (&[0x12], 2),
            (&[0x1F, 0x12], 6),
            (&[0x1E, 0x12], 8),
            (&[0x3F], 19),
            (&[0x10, 0x3F], 20),
            (&[0x34, 0x06], 7),
            (&[0x10, 0x86, 0x00, 0x01], 4),
        ];
        for (bytes, cycles) in cases {
            let mut cpu = cpu6309();
            let mut mem = program(bytes);
            assert_eq!(cpu.step(&mut mem).cycles, *cycles, "{bytes:02X?}");
        }
    }
}
