//! MC6809 undocumented opcode behaviour (MAME m6809.lst / hoglet67 hardware tests).

use crate::addressing::AddrMode;
use crate::alu::add16;
use crate::cpu::{Cpu, Reg16, Reg8, StepCtx};
use crate::flags::Flags;
use crate::memory::Memory;

impl Cpu {
    pub(crate) fn is_mc6809(&self) -> bool {
        !self.is_hd6309()
    }

    /// Handle MC6809-only page-1 opcodes. Returns true when dispatched.
    pub(crate) fn try_exec_page1_undoc(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_mc6809() {
            return false;
        }

        match opcode {
            // Alias opcodes (same behaviour as documented neighbours)
            0x01 => {
                self.op_mem_unary(mem, ctx, "NEG", AddrMode::Direct, 6, |cpu, v| cpu.op_neg8(v));
                true
            }
            0x05 => {
                self.op_mem_shift(mem, ctx, "LSR", AddrMode::Direct, 6, crate::alu::lsr8);
                true
            }
            0x41 => {
                self.op_reg_unary(ctx, "NEG", Reg8::A, 2, |cpu, v| cpu.op_neg8(v));
                true
            }
            0x45 => {
                self.op_reg_shift(ctx, "LSR", Reg8::A, 2, crate::alu::lsr8);
                true
            }
            0x51 => {
                self.op_reg_unary(ctx, "NEG", Reg8::B, 2, |cpu, v| cpu.op_neg8(v));
                true
            }
            0x55 => {
                self.op_reg_shift(ctx, "LSR", Reg8::B, 2, crate::alu::lsr8);
                true
            }
            0x61 => {
                self.op_mem_unary(mem, ctx, "NEG", AddrMode::Indexed, 6, |cpu, v| cpu.op_neg8(v));
                true
            }
            0x65 => {
                self.op_mem_shift(mem, ctx, "LSR", AddrMode::Indexed, 6, crate::alu::lsr8);
                true
            }
            0x71 => {
                self.op_mem_unary(mem, ctx, "NEG", AddrMode::Extended, 7, |cpu, v| cpu.op_neg8(v));
                true
            }
            0x75 => {
                self.op_mem_shift(mem, ctx, "LSR", AddrMode::Extended, 7, crate::alu::lsr8);
                true
            }

            // XNC8 — COM if C set, else NEG
            0x02 => {
                self.op_mem_unary(mem, ctx, "XNC", AddrMode::Direct, 6, |cpu, v| cpu.op_xnc8(v));
                true
            }
            0x42 => {
                self.op_reg_unary(ctx, "XNC", Reg8::A, 2, |cpu, v| cpu.op_xnc8(v));
                true
            }
            0x52 => {
                self.op_reg_unary(ctx, "XNC", Reg8::B, 2, |cpu, v| cpu.op_xnc8(v));
                true
            }
            0x62 => {
                self.op_mem_unary(mem, ctx, "XNC", AddrMode::Indexed, 6, |cpu, v| cpu.op_xnc8(v));
                true
            }
            0x72 => {
                self.op_mem_unary(mem, ctx, "XNC", AddrMode::Extended, 7, |cpu, v| cpu.op_xnc8(v));
                true
            }

            // XDEC8 — DEC with alternate carry semantics
            0x0B => {
                self.op_mem_unary(mem, ctx, "XDEC", AddrMode::Direct, 6, |cpu, v| cpu.op_xdec8(v));
                true
            }
            0x4B => {
                self.op_reg_unary(ctx, "XDEC", Reg8::A, 2, |cpu, v| cpu.op_xdec8(v));
                true
            }
            0x5B => {
                self.op_reg_unary(ctx, "XDEC", Reg8::B, 2, |cpu, v| cpu.op_xdec8(v));
                true
            }
            0x6B => {
                self.op_mem_unary(mem, ctx, "XDEC", AddrMode::Indexed, 6, |cpu, v| cpu.op_xdec8(v));
                true
            }
            0x7B => {
                self.op_mem_unary(mem, ctx, "XDEC", AddrMode::Extended, 7, |cpu, v| cpu.op_xdec8(v));
                true
            }

            // XCLR8 — CLR but preserves C
            0x4E => {
                self.op_reg_unary(ctx, "XCLR", Reg8::A, 2, |cpu, _| cpu.op_xclr8());
                true
            }
            0x5E => {
                self.op_reg_unary(ctx, "XCLR", Reg8::B, 2, |cpu, _| cpu.op_xclr8());
                true
            }

            // HCF / FREERUN test mode
            0x14 | 0x15 => {
                self.exec_freerun(ctx);
                true
            }

            // X18 — undocumented CC manipulation
            0x18 => {
                self.exec_x18(mem, ctx);
                true
            }

            // Undocumented NOP
            0x1B => {
                self.exec_nop(ctx);
                true
            }

            // XANDCC — ANDCC with extra cycle
            0x38 => {
                self.exec_xandcc(mem, ctx);
                true
            }

            // XRES — push state and vector through reset
            0x3E => {
                self.exec_xres(mem, ctx);
                true
            }

            // Store-immediate (consume operand, no memory write)
            0x87 => {
                self.op_xst8_imm(mem, ctx, Reg8::A);
                true
            }
            0xC7 => {
                self.op_xst8_imm(mem, ctx, Reg8::B);
                true
            }
            0x8F => {
                self.op_xst16_imm(mem, ctx, Reg16::X);
                true
            }
            0xCF => {
                self.op_xst16_imm(mem, ctx, Reg16::U);
                true
            }

            _ => false,
        }
    }

    pub(crate) fn exec_freerun(&mut self, ctx: &mut StepCtx) {
        self.free_run = true;
        ctx.cycles = 1;
        ctx.mnemonic = "HCF".into();
    }

    pub(crate) fn exec_freerun_step(&mut self) -> StepCtx {
        StepCtx {
            cycles: 1,
            bytes: vec![],
            mnemonic: "HCF".into(),
            operands: String::new(),
            trap: None,
        }
    }

    pub(crate) fn exec_x18(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let mask = self.fetch_imm8(mem, ctx);
        let cc_bits = self.cc.bits();
        let temp = (cc_bits & mask).wrapping_shl(1);
        self.cc = Flags::from_bits_retain(temp | ((cc_bits & 0x04) >> 1));
        ctx.cycles = 3;
        ctx.mnemonic = "X18".into();
        ctx.operands = format!("#${mask:02X}");
    }

    fn exec_xandcc(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        self.exec_andcc(mem, ctx);
        ctx.cycles += 1;
        ctx.mnemonic = "XANDCC".into();
    }

    fn exec_xres(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        ctx.cycles = 19;
        ctx.mnemonic = "XRES".into();
        let saved_i = self.cc.contains(Flags::I);
        let saved_f = self.cc.contains(Flags::F);
        let target = mem.read16(0xFFFE);
        self.push_interrupt_frame(mem, true);
        if !saved_i {
            self.cc.remove(Flags::I);
        }
        if !saved_f {
            self.cc.remove(Flags::F);
        }
        self.pc = target;
    }

    pub(crate) fn exec_xswi2(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        ctx.cycles = 19;
        ctx.mnemonic = "XSWI2".into();
        let saved_i = self.cc.contains(Flags::I);
        let saved_f = self.cc.contains(Flags::F);
        let target = mem.read16(0xFFF4);
        self.push_interrupt_frame(mem, true);
        if !saved_i {
            self.cc.remove(Flags::I);
        }
        if !saved_f {
            self.cc.remove(Flags::F);
        }
        self.pc = target;
    }

    pub(crate) fn exec_xfirq(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        ctx.cycles = 19;
        ctx.mnemonic = "XFIRQ".into();
        let saved_i = self.cc.contains(Flags::I);
        let saved_f = self.cc.contains(Flags::F);
        let target = mem.read16(0xFFF6);
        self.push_interrupt_frame(mem, true);
        if !saved_i {
            self.cc.remove(Flags::I);
        }
        if !saved_f {
            self.cc.remove(Flags::F);
        }
        self.pc = target;
    }

    pub(crate) fn op_xnc8(&mut self, value: u8) -> u8 {
        if self.cc.contains(Flags::C) {
            self.op_com8(value)
        } else {
            self.op_neg8(value)
        }
    }

    pub(crate) fn op_xdec8(&mut self, value: u8) -> u8 {
        if value != 0 {
            self.cc.insert(Flags::C);
        } else {
            self.cc.remove(Flags::C);
        }
        let result = value.wrapping_sub(1);
        self.cc.set(Flags::V, value == 0x80);
        self.cc.set_nz8(result);
        result
    }

    pub(crate) fn op_xclr8(&mut self) -> u8 {
        self.cc.remove(Flags::N | Flags::Z | Flags::V);
        self.cc.insert(Flags::Z);
        0
    }

    pub(crate) fn op_xst8_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg8) {
        let _operand = self.fetch_imm8(mem, ctx);
        let value = self.get_reg8(reg);
        self.cc.remove(Flags::V | Flags::C);
        self.cc.set_nz8(value);
        ctx.cycles = 2;
        ctx.mnemonic = "XST".into();
        ctx.operands = format!("#${_operand:02X}");
    }

    pub(crate) fn op_xst16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg16) {
        let hi = self.fetch_imm8(mem, ctx);
        let lo = self.fetch_imm8(mem, ctx);
        let value = self.get_reg16(reg);
        mem.write8(self.pc, value as u8);
        self.pc = self.pc.wrapping_add(1);
        self.cc.remove(Flags::V | Flags::C);
        self.cc.set_nz16(value);
        ctx.cycles = 3;
        ctx.mnemonic = "XST".into();
        ctx.operands = format!("#${hi:02X}{lo:02X}");
    }

    pub(crate) fn op_xadd16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg16) {
        let value = self.fetch_imm16(mem, ctx);
        let current = self.get_reg16(reg);
        add16(current, value, &mut self.cc);
        ctx.cycles = Self::add16_cycles(AddrMode::Immediate);
        ctx.mnemonic = "XADD".into();
        ctx.operands = format!("#${value:04X}");
    }

    pub(crate) fn op_xadd16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        add16(current, value, &mut self.cc);
        ctx.cycles = Self::add16_cycles(AddrMode::Direct);
        ctx.mnemonic = "XADD".into();
        ctx.operands = operand;
    }

    pub(crate) fn op_xadd16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        add16(current, value, &mut self.cc);
        ctx.cycles = Self::add16_cycles(AddrMode::Indexed) + extra as u32;
        ctx.mnemonic = "XADD".into();
        ctx.operands = operand;
    }

    pub(crate) fn op_xadd16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        add16(current, value, &mut self.cc);
        ctx.cycles = Self::add16_cycles(AddrMode::Extended);
        ctx.mnemonic = "XADD".into();
        ctx.operands = operand;
    }

    /// MC6809: log-only illegal opcode (MAME ILLEGAL — no trap).
    pub(crate) fn op_undoc_illegal(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 1;
        ctx.mnemonic = "???".into();
    }
}