use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::addressing::{indexed_addr, AddrMode};
use crate::alu::{
    add16, add8, asl8, asr8, cmp16, cmp8, lsr8, rol8, ror8, sub16, sub8,
};
use crate::flags::Flags;
use crate::memory::Memory;
use crate::types::{CpuState, CpuVariant, FlagState, StepResult, Trap};

/// Partial HD6309 TFM block transfer (continued across multiple `step()` calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TfmPending {
    pub opcode: u8,
    pub src: u16,
    pub dst: u16,
    pub src_code: u8,
    pub dst_code: u8,
    pub postbyte: u8,
    pub remaining: u16,
    pub pc_before: u16,
    pub bytes: Vec<u8>,
    #[serde(default = "default_true")]
    pub first_chunk: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub x: u16,
    pub y: u16,
    pub u: u16,
    pub s: u16,
    pub pc: u16,
    pub dp: u8,
    pub cc: Flags,
    pub total_cycles: u64,
    pub halted: bool,
    pub irq_pending: bool,
    pub firq_pending: bool,
    pub nmi_pending: bool,
    pub breakpoints: HashSet<u16>,
    pub variant: CpuVariant,
    pub w: u16,
    pub v: u16,
    pub mode_reg: u8,
    /// MC6809 HCF/FREERUN test mode (0x14/0x15/0xCD).
    #[serde(default)]
    pub free_run: bool,
    /// SYNC waiting for any interrupt line (MAME: line-triggered, mask ignored).
    #[serde(default)]
    pub sync_waiting: bool,
    /// CWAI pushed stack frame and waits for a serviceable interrupt.
    #[serde(default)]
    pub cwai_waiting: bool,
    /// MAME: NMI is not taken until the first LDS instruction has executed.
    #[serde(default)]
    pub lds_encountered: bool,
    /// HD6309 TFM in progress (chunked to keep the UI responsive).
    #[serde(default)]
    pub tfm_pending: Option<TfmPending>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Reg8 {
    A,
    B,
    Dp,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Reg16 {
    D,
    X,
    Y,
    U,
    S,
    Pc,
}

pub(crate) struct StepCtx {
    pub cycles: u32,
    pub bytes: Vec<u8>,
    pub mnemonic: String,
    pub operands: String,
    pub trap: Option<Trap>,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            b: 0,
            x: 0xFFFF,
            y: 0xFFFF,
            u: 0xFFFF,
            s: 0xFFFF,
            pc: 0,
            dp: 0,
            cc: Flags::from_bits_truncate(0x50),
            total_cycles: 0,
            halted: false,
            irq_pending: false,
            firq_pending: false,
            nmi_pending: false,
            breakpoints: HashSet::new(),
            variant: CpuVariant::Mc6809,
            w: 0,
            v: 0,
            mode_reg: 0,
            free_run: false,
            sync_waiting: false,
            cwai_waiting: false,
            lds_encountered: false,
            tfm_pending: None,
        }
    }

    pub fn reset(&mut self, mem: &Memory) {
        self.a = 0;
        self.b = 0;
        self.x = 0xFFFF;
        self.y = 0xFFFF;
        self.u = 0xFFFF;
        self.s = 0xFFFF;
        self.dp = 0;
        self.cc = Flags::I | Flags::F;
        self.halted = false;
        self.irq_pending = false;
        self.firq_pending = false;
        self.nmi_pending = false;
        self.pc = mem.read16(0xFFFE);
        self.total_cycles = 0;
        self.w = 0;
        self.v = 0;
        self.mode_reg = 0;
        self.free_run = false;
        self.sync_waiting = false;
        self.cwai_waiting = false;
        self.lds_encountered = false;
        self.tfm_pending = None;
    }

    fn nmi_is_serviceable(&self) -> bool {
        self.nmi_pending && self.lds_encountered
    }

    fn any_interrupt_line(&self) -> bool {
        self.nmi_is_serviceable() || self.firq_pending || self.irq_pending
    }

    fn serviceable_interrupt_vector(&self) -> Option<u16> {
        if self.nmi_is_serviceable() {
            Some(0xFFFC)
        } else if self.firq_pending && !self.cc.contains(Flags::F) {
            Some(0xFFF6)
        } else if self.irq_pending && !self.cc.contains(Flags::I) {
            Some(0xFFF8)
        } else {
            None
        }
    }

    fn clear_pending_for_vector(&mut self, vector: u16) {
        match vector {
            0xFFFC => self.nmi_pending = false,
            0xFFF6 => self.firq_pending = false,
            0xFFF8 => self.irq_pending = false,
            _ => {}
        }
    }

    fn finish_cwai(&mut self, mem: &Memory, vector: u16, pc_before: u16) -> StepResult {
        self.cwai_waiting = false;
        self.halted = false;
        self.clear_pending_for_vector(vector);
        self.cc.insert(Flags::I);
        if vector != 0xFFF8 {
            self.cc.insert(Flags::F);
        }
        self.pc = mem.read16(vector);
        self.total_cycles += 19;
        StepResult {
            cycles: 19,
            pc_before,
            pc_after: self.pc,
            opcode: 0x3C,
            bytes: vec![0x3C],
            mnemonic: "CWAI".into(),
            operands: String::new(),
            trap: None,
        }
    }

    pub fn get_state(&self) -> CpuState {
        CpuState {
            a: self.a,
            b: self.b,
            d: self.get_reg16(Reg16::D),
            x: self.x,
            y: self.y,
            u: self.u,
            s: self.s,
            pc: self.pc,
            dp: self.dp,
            cc: self.cc.bits(),
            flags: FlagState::from(self.cc),
            total_cycles: self.total_cycles,
            halted: self.halted,
            irq_pending: self.irq_pending,
            firq_pending: self.firq_pending,
            nmi_pending: self.nmi_pending,
            lds_encountered: self.lds_encountered,
            variant: self.variant,
            w: self.w,
            v: self.v,
            mode_reg: self.mode_reg,
        }
    }

    pub fn set_register(&mut self, register: &str, value: u16) -> Result<(), String> {
        match register.to_uppercase().as_str() {
            "A" => {
                self.a = value as u8;
                Ok(())
            }
            "B" => {
                self.b = value as u8;
                Ok(())
            }
            "DP" => {
                self.dp = value as u8;
                Ok(())
            }
            "D" => {
                self.set_reg16(Reg16::D, value);
                Ok(())
            }
            "X" => {
                self.x = value;
                Ok(())
            }
            "Y" => {
                self.y = value;
                Ok(())
            }
            "U" => {
                self.u = value;
                Ok(())
            }
            "S" => {
                self.s = value;
                Ok(())
            }
            "PC" => {
                self.pc = value;
                Ok(())
            }
            "W" if self.is_hd6309() => {
                self.w = value;
                Ok(())
            }
            "V" if self.is_hd6309() => {
                self.v = value;
                Ok(())
            }
            "MD" if self.is_hd6309() => {
                self.mode_reg = value as u8;
                Ok(())
            }
            "E" if self.is_hd6309() => {
                self.w = (self.w & 0x00FF) | (value << 8);
                Ok(())
            }
            "F" if self.is_hd6309() => {
                self.w = (self.w & 0xFF00) | value;
                Ok(())
            }
            _ => Err(format!("Unknown register: {register}")),
        }
    }

    pub fn toggle_flag(&mut self, flag: &str) -> Result<(), String> {
        match flag.to_uppercase().as_str() {
            "C" => self.cc.toggle(Flags::C),
            "V" => self.cc.toggle(Flags::V),
            "Z" => self.cc.toggle(Flags::Z),
            "N" => self.cc.toggle(Flags::N),
            "I" => self.cc.toggle(Flags::I),
            "H" => self.cc.toggle(Flags::H),
            "F" => self.cc.toggle(Flags::F),
            "E" => self.cc.toggle(Flags::E),
            _ => return Err(format!("Unknown flag: {flag}")),
        }
        Ok(())
    }

    pub fn push8(&mut self, mem: &mut Memory, value: u8) {
        self.s = self.s.wrapping_sub(1);
        mem.write8(self.s, value);
    }

    pub fn push16(&mut self, mem: &mut Memory, value: u16) {
        self.push8(mem, (value & 0xFF) as u8);
        self.push8(mem, (value >> 8) as u8);
    }

    pub fn pull8(&mut self, mem: &Memory) -> u8 {
        let value = mem.read8(self.s);
        self.s = self.s.wrapping_add(1);
        value
    }

    pub fn pull16(&mut self, mem: &Memory) -> u16 {
        let hi = self.pull8(mem) as u16;
        let lo = self.pull8(mem) as u16;
        (hi << 8) | lo
    }

    fn set_reg8(&mut self, reg: Reg8, value: u8) {
        match reg {
            Reg8::A => self.a = value,
            Reg8::B => self.b = value,
            Reg8::Dp => self.dp = value,
        }
    }

    pub(crate) fn get_reg8(&self, reg: Reg8) -> u8 {
        match reg {
            Reg8::A => self.a,
            Reg8::B => self.b,
            Reg8::Dp => self.dp,
        }
    }

    pub(crate) fn set_reg16(&mut self, reg: Reg16, value: u16) {
        match reg {
            Reg16::D => {
                self.a = (value >> 8) as u8;
                self.b = value as u8;
            }
            Reg16::X => self.x = value,
            Reg16::Y => self.y = value,
            Reg16::U => self.u = value,
            Reg16::S => self.s = value,
            Reg16::Pc => self.pc = value,
        }
    }

    pub(crate) fn get_reg16(&self, reg: Reg16) -> u16 {
        match reg {
            Reg16::D => ((self.a as u16) << 8) | self.b as u16,
            Reg16::X => self.x,
            Reg16::Y => self.y,
            Reg16::U => self.u,
            Reg16::S => self.s,
            Reg16::Pc => self.pc,
        }
    }

    pub fn step(&mut self, mem: &mut Memory) -> StepResult {
        if self.tfm_pending.is_some() {
            return self.run_tfm_chunk_step(mem);
        }

        if self.cwai_waiting {
            if let Some(vector) = self.serviceable_interrupt_vector() {
                return self.finish_cwai(mem, vector, self.pc);
            }
        }

        if self.halted {
            if self.sync_waiting && self.any_interrupt_line() {
                self.sync_waiting = false;
                self.halted = false;
                return self.make_step_result(0, 0, vec![0x13], "SYNC", "", None);
            }
            if self.cwai_waiting {
                if let Some(vector) = self.serviceable_interrupt_vector() {
                    let pc_before = self.pc;
                    return self.finish_cwai(mem, vector, pc_before);
                }
            }
            return self.make_step_result(0, 0, vec![], "HALT", "", Some(Trap::Halted));
        }

        let pc_before = self.pc;

        if self.free_run && self.is_mc6809() {
            if let Some(trap) = self.service_interrupts(mem) {
                self.free_run = false;
                return trap;
            }
            self.total_cycles += 1;
            let ctx = self.exec_freerun_step();
            return StepResult {
                cycles: ctx.cycles,
                pc_before,
                pc_after: self.pc,
                opcode: 0,
                bytes: ctx.bytes,
                mnemonic: ctx.mnemonic,
                operands: ctx.operands,
                trap: None,
            };
        }
        if self.breakpoints.contains(&pc_before) {
            let opcode = mem.read8(self.pc);
            return self.make_step_result(
                0,
                opcode,
                vec![opcode],
                "BPT",
                "",
                Some(Trap::Breakpoint),
            );
        }

        if let Some(trap) = self.service_interrupts(mem) {
            return trap;
        }

        mem.clear_watchpoint_trigger();

        let opcode = mem.read8(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let mut ctx = StepCtx {
            cycles: 0,
            bytes: vec![opcode],
            mnemonic: String::new(),
            operands: String::new(),
            trap: None,
        };

        match opcode {
            0x10 => self.exec_page2(mem, &mut ctx),
            0x11 => self.exec_page3(mem, &mut ctx),
            _ => self.exec_page1(opcode, mem, &mut ctx),
        }

        self.total_cycles += ctx.cycles as u64;

        let trap = if ctx.trap.is_some() {
            ctx.trap
        } else if mem.take_watchpoint_trigger().is_some() {
            Some(Trap::Watchpoint)
        } else {
            None
        };

        StepResult {
            cycles: ctx.cycles,
            pc_before,
            pc_after: self.pc,
            opcode,
            bytes: ctx.bytes,
            mnemonic: ctx.mnemonic,
            operands: ctx.operands,
            trap,
        }
    }

    fn make_step_result(
        &self,
        cycles: u32,
        opcode: u8,
        bytes: Vec<u8>,
        mnemonic: &str,
        operands: &str,
        trap: Option<Trap>,
    ) -> StepResult {
        StepResult {
            cycles,
            pc_before: self.pc,
            pc_after: self.pc,
            opcode,
            bytes,
            mnemonic: mnemonic.to_string(),
            operands: operands.to_string(),
            trap,
        }
    }

    pub(crate) fn is_native_6309(&self) -> bool {
        self.is_hd6309() && self.mode_reg & 0x01 != 0
    }

    fn firq_full_save(&self) -> bool {
        self.is_hd6309() && self.mode_reg & 0x02 != 0
    }

    pub(crate) fn push_interrupt_frame(&mut self, mem: &mut Memory, save_all: bool) {
        if save_all {
            self.push16(mem, self.pc);
            self.push16(mem, self.u);
            self.push16(mem, self.y);
            self.push16(mem, self.x);
            self.push8(mem, self.dp);
            if self.is_native_6309() {
                self.push8(mem, (self.w & 0xFF) as u8);
                self.push8(mem, (self.w >> 8) as u8);
            }
            self.push8(mem, self.b);
            self.push8(mem, self.a);
            self.cc.insert(Flags::E);
            self.push8(mem, self.cc.bits());
            self.cc.insert(Flags::I | Flags::F);
        } else {
            self.push16(mem, self.pc);
            self.push8(mem, self.cc.bits());
            self.cc.insert(Flags::I | Flags::F);
            self.cc.remove(Flags::E);
        }
    }

    fn service_interrupts(&mut self, mem: &mut Memory) -> Option<StepResult> {
        if self.nmi_is_serviceable() {
            self.nmi_pending = false;
            return Some(self.enter_interrupt(mem, 0xFFFC, true, "NMI"));
        }
        if self.firq_pending && !self.cc.contains(Flags::F) {
            self.firq_pending = false;
            let save_all = self.firq_full_save();
            return Some(self.enter_interrupt(mem, 0xFFF6, save_all, "FIRQ"));
        }
        if self.irq_pending && !self.cc.contains(Flags::I) {
            self.irq_pending = false;
            return Some(self.enter_interrupt(mem, 0xFFF8, true, "IRQ"));
        }
        None
    }

    pub(crate) fn enter_hw_trap(&mut self, mem: &mut Memory, error_bit: u8) {
        self.mode_reg |= error_bit;
        let target = mem.read16(0xFFF0);
        self.push_interrupt_frame(mem, true);
        self.pc = target;
        self.total_cycles += 19;
    }

    fn enter_interrupt(
        &mut self,
        mem: &mut Memory,
        vector: u16,
        save_all: bool,
        name: &str,
    ) -> StepResult {
        let pc_before = self.pc;
        // Read vector before pushing — a low S can overwrite vector addresses during push.
        let target = mem.read16(vector);
        self.push_interrupt_frame(mem, save_all);
        self.pc = target;
        self.total_cycles += 19;
        StepResult {
            cycles: 19,
            pc_before,
            pc_after: self.pc,
            opcode: 0,
            bytes: vec![],
            mnemonic: name.to_string(),
            operands: format!("${vector:04X}"),
            trap: None,
        }
    }

    pub(crate) fn exec_nop(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 2;
        ctx.mnemonic = "NOP".into();
    }

    fn exec_sync(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 2;
        ctx.mnemonic = "SYNC".into();
        if !self.any_interrupt_line() {
            self.sync_waiting = true;
            self.halted = true;
            ctx.trap = Some(Trap::Halted);
        }
    }

    fn exec_swi(&mut self, mem: &mut Memory, ctx: &mut StepCtx, vector: u16, name: &str) {
        ctx.cycles = 19;
        ctx.mnemonic = name.into();
        let target = mem.read16(vector);
        self.push_interrupt_frame(mem, true);
        self.pc = target;
        ctx.trap = Some(Trap::Swi);
    }

    fn exec_rti(&mut self, mem: &Memory, ctx: &mut StepCtx) {
        ctx.cycles = 6;
        ctx.mnemonic = "RTI".into();
        self.cc = Flags::from_byte(self.pull8(mem));
        if self.cc.contains(Flags::E) {
            self.a = self.pull8(mem);
            self.b = self.pull8(mem);
            if self.is_native_6309() {
                let e = self.pull8(mem);
                let f = self.pull8(mem);
                self.w = ((e as u16) << 8) | (f as u16);
                ctx.cycles += 2;
            }
            self.dp = self.pull8(mem);
            self.x = self.pull16(mem);
            self.y = self.pull16(mem);
            self.u = self.pull16(mem);
            ctx.cycles += 9;
        }
        self.pc = self.pull16(mem);
    }

    fn exec_rts(&mut self, mem: &Memory, ctx: &mut StepCtx) {
        ctx.cycles = 5;
        ctx.mnemonic = "RTS".into();
        self.pc = self.pull16(mem);
    }

    fn exec_sex(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 3;
        ctx.mnemonic = "SEX".into();
        self.a = if self.b & 0x80 != 0 { 0xFF } else { 0x00 };
        // N/Z from the 16-bit result in D (Motorola SEX).
        self.cc.set_nz16(self.get_reg16(Reg16::D));
    }

    fn exec_cwai(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let mask = self.fetch_imm8(mem, ctx);
        ctx.cycles = 20;
        ctx.mnemonic = "CWAI".into();
        ctx.operands = format!("#${mask:02X}");
        self.cc = Flags::from_bits_retain(self.cc.bits() & mask);
        if let Some(vector) = self.serviceable_interrupt_vector() {
            let target = mem.read16(vector);
            self.push_interrupt_frame(mem, true);
            self.clear_pending_for_vector(vector);
            self.cc.insert(Flags::I);
            if vector != 0xFFF8 {
                self.cc.insert(Flags::F);
            }
            self.pc = target;
        } else {
            self.push_interrupt_frame(mem, true);
            self.cwai_waiting = true;
            self.halted = true;
            ctx.trap = Some(Trap::Halted);
        }
    }

    fn exec_orcc(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm8(mem, ctx);
        ctx.cycles = 3;
        ctx.mnemonic = "ORCC".into();
        ctx.operands = format!("#${val:02X}");
        self.cc = Flags::from_bits_retain(self.cc.bits() | val);
    }

    pub(crate) fn exec_andcc(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm8(mem, ctx);
        ctx.cycles = 3;
        ctx.mnemonic = "ANDCC".into();
        ctx.operands = format!("#${val:02X}");
        self.cc = Flags::from_bits_retain(self.cc.bits() & val);
    }

    fn exec_daa(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 2;
        ctx.mnemonic = "DAA".into();
        let msn = self.a & 0xF0;
        let lsn = self.a & 0x0F;
        let mut cf: u16 = 0;
        if lsn > 0x09 || self.cc.contains(Flags::H) {
            cf |= 0x06;
        }
        if msn > 0x80 && lsn > 0x09 {
            cf |= 0x60;
        }
        if msn > 0x90 || self.cc.contains(Flags::C) {
            cf |= 0x60;
        }
        let t = self.a as u16 + cf;
        self.cc.remove(Flags::V);
        if t & 0x0100 != 0 {
            self.cc.insert(Flags::C);
        }
        self.a = t as u8;
        self.cc.set_nz8(self.a);
        self.cc.remove(Flags::H);
    }

    fn exec_abx(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 3;
        ctx.mnemonic = "ABX".into();
        self.x = self.x.wrapping_add(self.b as u16);
    }

    fn exec_mul(&mut self, ctx: &mut StepCtx) {
        ctx.cycles = 11;
        ctx.mnemonic = "MUL".into();
        let result = (self.a as u16).wrapping_mul(self.b as u16);
        self.a = (result >> 8) as u8;
        self.b = result as u8;
        self.cc.set(Flags::C, self.b & 0x80 != 0);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(result);
    }

    pub(crate) fn fetch_imm8(&mut self, mem: &Memory, ctx: &mut StepCtx) -> u8 {
        let value = mem.read8(self.pc);
        ctx.bytes.push(value);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    pub(crate) fn fetch_imm16(&mut self, mem: &Memory, ctx: &mut StepCtx) -> u16 {
        let hi = self.fetch_imm8(mem, ctx);
        let lo = self.fetch_imm8(mem, ctx);
        ((hi as u16) << 8) | lo as u16
    }

    pub(crate) fn addr_direct(&mut self, mem: &Memory, ctx: &mut StepCtx) -> (u16, String) {
        let offset = self.fetch_imm8(mem, ctx);
        let addr = ((self.dp as u16) << 8) | offset as u16;
        (addr, format!("<${offset:02X}"))
    }

    pub(crate) fn addr_extended(&mut self, mem: &Memory, ctx: &mut StepCtx) -> (u16, String) {
        let hi = self.fetch_imm8(mem, ctx);
        let lo = self.fetch_imm8(mem, ctx);
        let full = ((hi as u16) << 8) | lo as u16;
        (full, format!("${full:04X}"))
    }

    pub(crate) fn addr_indexed(&mut self, mem: &Memory, ctx: &mut StepCtx) -> (u16, u8, String) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let ea = indexed_addr(self, mem, postbyte);
        let op = format_indexed_operand(ea.index_reg, postbyte, &ctx.bytes);
        (ea.addr, ea.extra_cycles, op)
    }

    fn addr_relative8(&mut self, mem: &Memory, ctx: &mut StepCtx) -> (u16, String) {
        let offset = self.fetch_imm8(mem, ctx) as i8;
        let target = self.pc.wrapping_add(offset as u16);
        (target, format!("${target:04X}"))
    }

    fn addr_relative16(&mut self, mem: &Memory, ctx: &mut StepCtx) -> (u16, String) {
        let offset = self.fetch_imm16(mem, ctx) as i16;
        let target = self.pc.wrapping_add(offset as u16);
        (target, format!("${target:04X}"))
    }

    fn logical_nz8(flags: &mut Flags, result: u8) -> u8 {
        flags.remove(Flags::V);
        flags.set_nz8(result);
        result
    }

    pub(crate) fn op_neg8(&mut self, value: u8) -> u8 {
        self.cc.set(Flags::C, value != 0);
        self.cc.set(Flags::V, value == 0x80);
        let result = 0u8.wrapping_sub(value);
        self.cc.set_nz8(result);
        result
    }

    pub(crate) fn op_com8(&mut self, value: u8) -> u8 {
        let result = !value;
        self.cc.remove(Flags::V);
        self.cc.insert(Flags::C);
        self.cc.set_nz8(result);
        result
    }

    pub(crate) fn op_inc8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.cc.set(Flags::V, value == 0x7F);
        self.cc.set_nz8(result);
        result
    }

    pub(crate) fn op_dec8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.cc.set(Flags::V, value == 0x80);
        self.cc.set_nz8(result);
        result
    }

    fn op_tst8(&mut self, value: u8) {
        // Motorola: V cleared; C not affected.
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
    }

    fn op_clr8(&mut self) -> u8 {
        self.cc.remove(Flags::V | Flags::C);
        self.cc.insert(Flags::Z);
        self.cc.remove(Flags::N);
        0
    }

    fn exec_page1(&mut self, opcode: u8, mem: &mut Memory, ctx: &mut StepCtx) {
        if self.try_exec_page1_undoc(opcode, mem, ctx) {
            return;
        }
        match opcode {
            0x00 => self.op_mem_unary(mem, ctx, "NEG", AddrMode::Direct, 6, |cpu, v| cpu.op_neg8(v)),
            0x03 => self.op_mem_unary(mem, ctx, "COM", AddrMode::Direct, 6, |cpu, v| cpu.op_com8(v)),
            0x04 => self.op_mem_shift(mem, ctx, "LSR", AddrMode::Direct, 6, lsr8),
            0x06 => self.op_mem_shift(mem, ctx, "ROR", AddrMode::Direct, 6, ror8),
            0x07 => self.op_mem_shift(mem, ctx, "ASR", AddrMode::Direct, 6, asr8),
            0x08 => self.op_mem_shift(mem, ctx, "ASL", AddrMode::Direct, 6, asl8),
            0x09 => self.op_mem_shift(mem, ctx, "ROL", AddrMode::Direct, 6, rol8),
            0x0A => self.op_mem_unary(mem, ctx, "DEC", AddrMode::Direct, 6, |cpu, v| cpu.op_dec8(v)),
            0x0C => self.op_mem_unary(mem, ctx, "INC", AddrMode::Direct, 6, |cpu, v| cpu.op_inc8(v)),
            0x0D => self.op_mem_test(mem, ctx, "TST", AddrMode::Direct, 6),
            0x0E => self.op_jmp(mem, ctx, "JMP", AddrMode::Direct, 3),
            0x0F => self.op_mem_store(mem, ctx, "CLR", AddrMode::Direct, 6, |cpu| cpu.op_clr8()),

            0x12 => self.exec_nop(ctx),
            0x13 => self.exec_sync(ctx),
            0x14 => {
                if self.is_hd6309() {
                    self.op_sexw(ctx);
                } else {
                    self.exec_freerun(ctx);
                }
            }
            0x15 => {
                if self.is_hd6309() {
                    self.op_illegal(mem, ctx);
                } else {
                    self.exec_freerun(ctx);
                }
            }
            0x16 => self.op_branch16(mem, ctx, true),
            0x17 => self.op_branch16(mem, ctx, false),
            0x18 => {
                if self.is_hd6309() {
                    self.op_illegal(mem, ctx);
                } else {
                    self.exec_x18(mem, ctx);
                }
            }
            0x19 => self.exec_daa(ctx),
            0x1A => self.exec_orcc(mem, ctx),
            0x1B => {
                if self.is_hd6309() {
                    self.op_illegal(mem, ctx);
                } else {
                    self.exec_nop(ctx);
                }
            }
            0x1C => self.exec_andcc(mem, ctx),
            0x1D => self.exec_sex(ctx),
            0x1E => self.op_exg(mem, ctx),
            0x1F => self.op_tfr(mem, ctx),

            0x20 => self.op_branch8(mem, ctx, "BRA", |_| true),
            0x21 => self.op_branch8(mem, ctx, "BRN", |_| false),
            0x22 => self.op_branch8(mem, ctx, "BHI", |c| !c.cc.contains(Flags::C) && !c.cc.contains(Flags::Z)),
            0x23 => self.op_branch8(mem, ctx, "BLS", |c| c.cc.contains(Flags::C) || c.cc.contains(Flags::Z)),
            0x24 => self.op_branch8(mem, ctx, "BCC", |c| !c.cc.contains(Flags::C)),
            0x25 => self.op_branch8(mem, ctx, "BCS", |c| c.cc.contains(Flags::C)),
            0x26 => self.op_branch8(mem, ctx, "BNE", |c| !c.cc.contains(Flags::Z)),
            0x27 => self.op_branch8(mem, ctx, "BEQ", |c| c.cc.contains(Flags::Z)),
            0x28 => self.op_branch8(mem, ctx, "BVC", |c| !c.cc.contains(Flags::V)),
            0x29 => self.op_branch8(mem, ctx, "BVS", |c| c.cc.contains(Flags::V)),
            0x2A => self.op_branch8(mem, ctx, "BPL", |c| !c.cc.contains(Flags::N)),
            0x2B => self.op_branch8(mem, ctx, "BMI", |c| c.cc.contains(Flags::N)),
            0x2C => self.op_branch8(mem, ctx, "BGE", |c| c.cc.contains(Flags::N) == c.cc.contains(Flags::V)),
            0x2D => self.op_branch8(mem, ctx, "BLT", |c| c.cc.contains(Flags::N) != c.cc.contains(Flags::V)),
            0x2E => self.op_branch8(mem, ctx, "BGT", |c| !c.cc.contains(Flags::Z) && c.cc.contains(Flags::N) == c.cc.contains(Flags::V)),
            0x2F => self.op_branch8(mem, ctx, "BLE", |c| c.cc.contains(Flags::Z) || c.cc.contains(Flags::N) != c.cc.contains(Flags::V)),

            0x30..=0x33 => {
                let dest = match opcode {
                    0x30 => Reg16::X,
                    0x31 => Reg16::Y,
                    0x32 => Reg16::S,
                    0x33 => Reg16::U,
                    _ => unreachable!(),
                };
                self.op_lea(mem, ctx, dest);
            }
            0x34 => self.op_psh(mem, ctx, true),
            0x35 => self.op_pul(mem, ctx, true),
            0x36 => self.op_psh(mem, ctx, false),
            0x37 => self.op_pul(mem, ctx, false),
            0x39 => self.exec_rts(mem, ctx),
            0x3A => self.exec_abx(ctx),
            0x3B => self.exec_rti(mem, ctx),
            0x3C => self.exec_cwai(mem, ctx),
            0x3D => self.exec_mul(ctx),
            0x3F => self.exec_swi(mem, ctx, 0xFFFA, "SWI"),

            0x40 => self.op_reg_unary(ctx, "NEG", Reg8::A, 2, |cpu, v| cpu.op_neg8(v)),
            0x43 => self.op_reg_unary(ctx, "COM", Reg8::A, 2, |cpu, v| cpu.op_com8(v)),
            0x44 => self.op_reg_shift(ctx, "LSR", Reg8::A, 2, lsr8),
            0x46 => self.op_reg_shift(ctx, "ROR", Reg8::A, 2, ror8),
            0x47 => self.op_reg_shift(ctx, "ASR", Reg8::A, 2, asr8),
            0x48 => self.op_reg_shift(ctx, "ASL", Reg8::A, 2, asl8),
            0x49 => self.op_reg_shift(ctx, "ROL", Reg8::A, 2, rol8),
            0x4A => self.op_reg_unary(ctx, "DEC", Reg8::A, 2, |cpu, v| cpu.op_dec8(v)),
            0x4C => self.op_reg_unary(ctx, "INC", Reg8::A, 2, |cpu, v| cpu.op_inc8(v)),
            0x4D => self.op_reg_test(ctx, "TST", Reg8::A, 2),
            0x4F => self.op_reg_clr(ctx, "CLRA", Reg8::A),

            0x50 => self.op_reg_unary(ctx, "NEG", Reg8::B, 2, |cpu, v| cpu.op_neg8(v)),
            0x53 => self.op_reg_unary(ctx, "COM", Reg8::B, 2, |cpu, v| cpu.op_com8(v)),
            0x54 => self.op_reg_shift(ctx, "LSR", Reg8::B, 2, lsr8),
            0x56 => self.op_reg_shift(ctx, "ROR", Reg8::B, 2, ror8),
            0x57 => self.op_reg_shift(ctx, "ASR", Reg8::B, 2, asr8),
            0x58 => self.op_reg_shift(ctx, "ASL", Reg8::B, 2, asl8),
            0x59 => self.op_reg_shift(ctx, "ROL", Reg8::B, 2, rol8),
            0x5A => self.op_reg_unary(ctx, "DEC", Reg8::B, 2, |cpu, v| cpu.op_dec8(v)),
            0x5C => self.op_reg_unary(ctx, "INC", Reg8::B, 2, |cpu, v| cpu.op_inc8(v)),
            0x5D => self.op_reg_test(ctx, "TST", Reg8::B, 2),
            0x5F => self.op_reg_clr(ctx, "CLRB", Reg8::B),

            0x60 => self.op_mem_unary(mem, ctx, "NEG", AddrMode::Indexed, 6, |cpu, v| cpu.op_neg8(v)),
            0x63 => self.op_mem_unary(mem, ctx, "COM", AddrMode::Indexed, 6, |cpu, v| cpu.op_com8(v)),
            0x64 => self.op_mem_shift(mem, ctx, "LSR", AddrMode::Indexed, 6, lsr8),
            0x66 => self.op_mem_shift(mem, ctx, "ROR", AddrMode::Indexed, 6, ror8),
            0x67 => self.op_mem_shift(mem, ctx, "ASR", AddrMode::Indexed, 6, asr8),
            0x68 => self.op_mem_shift(mem, ctx, "ASL", AddrMode::Indexed, 6, asl8),
            0x69 => self.op_mem_shift(mem, ctx, "ROL", AddrMode::Indexed, 6, rol8),
            0x6A => self.op_mem_unary(mem, ctx, "DEC", AddrMode::Indexed, 6, |cpu, v| cpu.op_dec8(v)),
            0x6C => self.op_mem_unary(mem, ctx, "INC", AddrMode::Indexed, 6, |cpu, v| cpu.op_inc8(v)),
            0x6D => self.op_mem_test(mem, ctx, "TST", AddrMode::Indexed, 6),
            0x6E => self.op_jmp(mem, ctx, "JMP", AddrMode::Indexed, 3),
            0x6F => self.op_mem_store(mem, ctx, "CLR", AddrMode::Indexed, 6, |cpu| cpu.op_clr8()),

            0x70 => self.op_mem_unary(mem, ctx, "NEG", AddrMode::Extended, 7, |cpu, v| cpu.op_neg8(v)),
            0x73 => self.op_mem_unary(mem, ctx, "COM", AddrMode::Extended, 7, |cpu, v| cpu.op_com8(v)),
            0x74 => self.op_mem_shift(mem, ctx, "LSR", AddrMode::Extended, 7, lsr8),
            0x76 => self.op_mem_shift(mem, ctx, "ROR", AddrMode::Extended, 7, ror8),
            0x77 => self.op_mem_shift(mem, ctx, "ASR", AddrMode::Extended, 7, asr8),
            0x78 => self.op_mem_shift(mem, ctx, "ASL", AddrMode::Extended, 7, asl8),
            0x79 => self.op_mem_shift(mem, ctx, "ROL", AddrMode::Extended, 7, rol8),
            0x7A => self.op_mem_unary(mem, ctx, "DEC", AddrMode::Extended, 7, |cpu, v| cpu.op_dec8(v)),
            0x7C => self.op_mem_unary(mem, ctx, "INC", AddrMode::Extended, 7, |cpu, v| cpu.op_inc8(v)),
            0x7D => self.op_mem_test(mem, ctx, "TST", AddrMode::Extended, 7),
            0x7E => self.op_jmp(mem, ctx, "JMP", AddrMode::Extended, 3),
            0x7F => self.op_mem_store(mem, ctx, "CLR", AddrMode::Extended, 7, |cpu| cpu.op_clr8()),

            0x80 => self.op_alu8_imm(mem, ctx, "SUBA", Reg8::A, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0x81 => self.op_alu8_imm(mem, ctx, "CMPA", Reg8::A, |a, b, f| { cmp8(a, b, f); a }),
            0x82 => self.op_alu8_imm(mem, ctx, "SBCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0x83 => self.op_sub16_imm(mem, ctx, "SUBD", Reg16::D),
            0x84 => self.op_alu8_imm(mem, ctx, "ANDA", Reg8::A, |a, b, f| Self::logical_nz8(f, a & b)),
            0x85 => self.op_bit8_imm(mem, ctx, "BITA", Reg8::A),
            0x86 => self.op_ld8_imm(mem, ctx, "LDA", Reg8::A),
            0x88 => self.op_alu8_imm(mem, ctx, "EORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0x89 => self.op_alu8_imm(mem, ctx, "ADCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0x8A => self.op_alu8_imm(mem, ctx, "ORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a | b)),
            0x8B => self.op_alu8_imm(mem, ctx, "ADDA", Reg8::A, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0x8C => self.op_cmp16_imm(mem, ctx, "CMPX", Reg16::X),
            0x8D => self.op_bsr(mem, ctx),
            0x8E => self.op_ld16_imm(mem, ctx, "LDX", Reg16::X),

            0x90 => self.op_alu8_dir(mem, ctx, "SUBA", Reg8::A, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0x91 => self.op_alu8_dir(mem, ctx, "CMPA", Reg8::A, |a, b, f| { cmp8(a, b, f); a }),
            0x92 => self.op_alu8_dir(mem, ctx, "SBCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0x93 => self.op_sub16_dir(mem, ctx, "SUBD", Reg16::D),
            0x94 => self.op_alu8_dir(mem, ctx, "ANDA", Reg8::A, |a, b, f| Self::logical_nz8(f, a & b)),
            0x95 => self.op_bit8_dir(mem, ctx, "BITA", Reg8::A),
            0x96 => self.op_ld8_dir(mem, ctx, "LDA", Reg8::A),
            0x98 => self.op_alu8_dir(mem, ctx, "EORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0x99 => self.op_alu8_dir(mem, ctx, "ADCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0x9A => self.op_alu8_dir(mem, ctx, "ORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a | b)),
            0x9B => self.op_alu8_dir(mem, ctx, "ADDA", Reg8::A, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0x9C => self.op_cmp16_dir(mem, ctx, "CMPX", Reg16::X),
            0x9D => self.op_jsr(mem, ctx, AddrMode::Direct),
            0x9E => self.op_ld16_dir(mem, ctx, "LDX", Reg16::X),
            0x9F => self.op_st16_dir(mem, ctx, "STX", Reg16::X),

            0xA0 => self.op_alu8_idx(mem, ctx, "SUBA", Reg8::A, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xA1 => self.op_alu8_idx(mem, ctx, "CMPA", Reg8::A, |a, b, f| { cmp8(a, b, f); a }),
            0xA2 => self.op_alu8_idx(mem, ctx, "SBCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xA3 => self.op_sub16_idx(mem, ctx, "SUBD", Reg16::D),
            0xA4 => self.op_alu8_idx(mem, ctx, "ANDA", Reg8::A, |a, b, f| Self::logical_nz8(f, a & b)),
            0xA5 => self.op_bit8_idx(mem, ctx, "BITA", Reg8::A),
            0xA6 => self.op_ld8_idx(mem, ctx, "LDA", Reg8::A),
            0xA8 => self.op_alu8_idx(mem, ctx, "EORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xA9 => self.op_alu8_idx(mem, ctx, "ADCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xAA => self.op_alu8_idx(mem, ctx, "ORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a | b)),
            0xAB => self.op_alu8_idx(mem, ctx, "ADDA", Reg8::A, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xAC => self.op_cmp16_idx(mem, ctx, "CMPX", Reg16::X),
            0xAD => self.op_jsr(mem, ctx, AddrMode::Indexed),
            0xAE => self.op_ld16_idx(mem, ctx, "LDX", Reg16::X),
            0xAF => self.op_st16_idx(mem, ctx, "STX", Reg16::X),

            0xB0 => self.op_alu8_ext(mem, ctx, "SUBA", Reg8::A, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xB1 => self.op_alu8_ext(mem, ctx, "CMPA", Reg8::A, |a, b, f| { cmp8(a, b, f); a }),
            0xB2 => self.op_alu8_ext(mem, ctx, "SBCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xB3 => self.op_sub16_ext(mem, ctx, "SUBD", Reg16::D),
            0xB4 => self.op_alu8_ext(mem, ctx, "ANDA", Reg8::A, |a, b, f| Self::logical_nz8(f, a & b)),
            0xB5 => self.op_bit8_ext(mem, ctx, "BITA", Reg8::A),
            0xB6 => self.op_ld8_ext(mem, ctx, "LDA", Reg8::A),
            0xB8 => self.op_alu8_ext(mem, ctx, "EORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xB9 => self.op_alu8_ext(mem, ctx, "ADCA", Reg8::A, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xBA => self.op_alu8_ext(mem, ctx, "ORA", Reg8::A, |a, b, f| Self::logical_nz8(f, a | b)),
            0xBB => self.op_alu8_ext(mem, ctx, "ADDA", Reg8::A, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xBC => self.op_cmp16_ext(mem, ctx, "CMPX", Reg16::X),
            0xBD => self.op_jsr(mem, ctx, AddrMode::Extended),
            0xBE => self.op_ld16_ext(mem, ctx, "LDX", Reg16::X),
            0xBF => self.op_st16_ext(mem, ctx, "STX", Reg16::X),

            0xC0 => self.op_alu8_imm(mem, ctx, "SUBB", Reg8::B, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xC1 => self.op_alu8_imm(mem, ctx, "CMPB", Reg8::B, |a, b, f| { cmp8(a, b, f); a }),
            0xC2 => self.op_alu8_imm(mem, ctx, "SBCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xC3 => self.op_add16_imm(mem, ctx, "ADDD", Reg16::D),
            0xC4 => self.op_alu8_imm(mem, ctx, "ANDB", Reg8::B, |a, b, f| Self::logical_nz8(f, a & b)),
            0xC5 => self.op_bit8_imm(mem, ctx, "BITB", Reg8::B),
            0xC6 => self.op_ld8_imm(mem, ctx, "LDB", Reg8::B),
            0xC8 => self.op_alu8_imm(mem, ctx, "EORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xC9 => self.op_alu8_imm(mem, ctx, "ADCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xCA => self.op_alu8_imm(mem, ctx, "ORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a | b)),
            0xCB => self.op_alu8_imm(mem, ctx, "ADDB", Reg8::B, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xCC => self.op_ld16_imm(mem, ctx, "LDD", Reg16::D),
            0xCD => {
                if self.is_hd6309() {
                    self.op_ldq_imm(mem, ctx);
                } else {
                    self.exec_freerun(ctx);
                }
            }
            0xCE => self.op_ld16_imm(mem, ctx, "LDU", Reg16::U),

            0xD0 => self.op_alu8_dir(mem, ctx, "SUBB", Reg8::B, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xD1 => self.op_alu8_dir(mem, ctx, "CMPB", Reg8::B, |a, b, f| { cmp8(a, b, f); a }),
            0xD2 => self.op_alu8_dir(mem, ctx, "SBCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xD3 => self.op_add16_dir(mem, ctx, "ADDD", Reg16::D),
            0xD4 => self.op_alu8_dir(mem, ctx, "ANDB", Reg8::B, |a, b, f| Self::logical_nz8(f, a & b)),
            0xD5 => self.op_bit8_dir(mem, ctx, "BITB", Reg8::B),
            0xD6 => self.op_ld8_dir(mem, ctx, "LDB", Reg8::B),
            0xD8 => self.op_alu8_dir(mem, ctx, "EORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xD9 => self.op_alu8_dir(mem, ctx, "ADCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xDA => self.op_alu8_dir(mem, ctx, "ORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a | b)),
            0xDB => self.op_alu8_dir(mem, ctx, "ADDB", Reg8::B, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xDC => self.op_ld16_dir(mem, ctx, "LDD", Reg16::D),
            0xDD => self.op_st16_dir(mem, ctx, "STD", Reg16::D),
            0xDE => self.op_ld16_dir(mem, ctx, "LDU", Reg16::U),
            0xDF => self.op_st16_dir(mem, ctx, "STU", Reg16::U),

            0xE0 => self.op_alu8_idx(mem, ctx, "SUBB", Reg8::B, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xE1 => self.op_alu8_idx(mem, ctx, "CMPB", Reg8::B, |a, b, f| { cmp8(a, b, f); a }),
            0xE2 => self.op_alu8_idx(mem, ctx, "SBCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xE3 => self.op_add16_idx(mem, ctx, "ADDD", Reg16::D),
            0xE4 => self.op_alu8_idx(mem, ctx, "ANDB", Reg8::B, |a, b, f| Self::logical_nz8(f, a & b)),
            0xE5 => self.op_bit8_idx(mem, ctx, "BITB", Reg8::B),
            0xE6 => self.op_ld8_idx(mem, ctx, "LDB", Reg8::B),
            0xE8 => self.op_alu8_idx(mem, ctx, "EORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xE9 => self.op_alu8_idx(mem, ctx, "ADCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xEA => self.op_alu8_idx(mem, ctx, "ORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a | b)),
            0xEB => self.op_alu8_idx(mem, ctx, "ADDB", Reg8::B, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xEC => self.op_ld16_idx(mem, ctx, "LDD", Reg16::D),
            0xED => self.op_st16_idx(mem, ctx, "STD", Reg16::D),
            0xEE => self.op_ld16_idx(mem, ctx, "LDU", Reg16::U),
            0xEF => self.op_st16_idx(mem, ctx, "STU", Reg16::U),

            0xF0 => self.op_alu8_ext(mem, ctx, "SUBB", Reg8::B, |a, b, f| { sub8(a, b, false, f); a.wrapping_sub(b) }),
            0xF1 => self.op_alu8_ext(mem, ctx, "CMPB", Reg8::B, |a, b, f| { cmp8(a, b, f); a }),
            0xF2 => self.op_alu8_ext(mem, ctx, "SBCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); sub8(a, b, c, f); a.wrapping_sub(b).wrapping_sub(u8::from(c)) }),
            0xF3 => self.op_add16_ext(mem, ctx, "ADDD", Reg16::D),
            0xF4 => self.op_alu8_ext(mem, ctx, "ANDB", Reg8::B, |a, b, f| Self::logical_nz8(f, a & b)),
            0xF5 => self.op_bit8_ext(mem, ctx, "BITB", Reg8::B),
            0xF6 => self.op_ld8_ext(mem, ctx, "LDB", Reg8::B),
            0xF8 => self.op_alu8_ext(mem, ctx, "EORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a ^ b)),
            0xF9 => self.op_alu8_ext(mem, ctx, "ADCB", Reg8::B, |a, b, f| { let c = f.contains(Flags::C); add8(a, b, c, f); a.wrapping_add(b).wrapping_add(u8::from(c)) }),
            0xFA => self.op_alu8_ext(mem, ctx, "ORB", Reg8::B, |a, b, f| Self::logical_nz8(f, a | b)),
            0xFB => self.op_alu8_ext(mem, ctx, "ADDB", Reg8::B, |a, b, f| { add8(a, b, false, f); a.wrapping_add(b) }),
            0xFC => self.op_ld16_ext(mem, ctx, "LDD", Reg16::D),
            0xFD => self.op_st16_ext(mem, ctx, "STD", Reg16::D),
            0xFE => self.op_ld16_ext(mem, ctx, "LDU", Reg16::U),
            0xFF => self.op_st16_ext(mem, ctx, "STU", Reg16::U),

            0x97 => self.op_st8_dir(mem, ctx, "STA", Reg8::A),
            0xA7 => self.op_st8_idx(mem, ctx, "STA", Reg8::A),
            0xB7 => self.op_st8_ext(mem, ctx, "STA", Reg8::A),
            0xD7 => self.op_st8_dir(mem, ctx, "STB", Reg8::B),
            0xE7 => self.op_st8_idx(mem, ctx, "STB", Reg8::B),
            0xF7 => self.op_st8_ext(mem, ctx, "STB", Reg8::B),

            _ => {
                if self.is_hd6309() {
                    self.op_illegal(mem, ctx);
                } else {
                    self.op_undoc_illegal(ctx);
                }
            }
        }
    }

    fn exec_page2(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let opcode = self.fetch_imm8(mem, ctx);
        if self.try_hd6309_page2(opcode, mem, ctx) {
            return;
        }
        match opcode {
            0x10 | 0x11 if self.is_mc6809() => self.exec_page2(mem, ctx),
            0x20 => self.op_lbranch16(mem, ctx, "LBRA", |_| true),
            0x21 => self.op_lbranch16(mem, ctx, "LBRN", |_| false),
            0x22 => self.op_lbranch16(mem, ctx, "LBHI", |c| !c.cc.contains(Flags::C) && !c.cc.contains(Flags::Z)),
            0x23 => self.op_lbranch16(mem, ctx, "LBLS", |c| c.cc.contains(Flags::C) || c.cc.contains(Flags::Z)),
            0x24 => self.op_lbranch16(mem, ctx, "LBCC", |c| !c.cc.contains(Flags::C)),
            0x25 => self.op_lbranch16(mem, ctx, "LBCS", |c| c.cc.contains(Flags::C)),
            0x26 => self.op_lbranch16(mem, ctx, "LBNE", |c| !c.cc.contains(Flags::Z)),
            0x27 => self.op_lbranch16(mem, ctx, "LBEQ", |c| c.cc.contains(Flags::Z)),
            0x28 => self.op_lbranch16(mem, ctx, "LBVC", |c| !c.cc.contains(Flags::V)),
            0x29 => self.op_lbranch16(mem, ctx, "LBVS", |c| c.cc.contains(Flags::V)),
            0x2A => self.op_lbranch16(mem, ctx, "LBPL", |c| !c.cc.contains(Flags::N)),
            0x2B => self.op_lbranch16(mem, ctx, "LBMI", |c| c.cc.contains(Flags::N)),
            0x2C => self.op_lbranch16(mem, ctx, "LBGE", |c| c.cc.contains(Flags::N) == c.cc.contains(Flags::V)),
            0x2D => self.op_lbranch16(mem, ctx, "LBLT", |c| c.cc.contains(Flags::N) != c.cc.contains(Flags::V)),
            0x2E => self.op_lbranch16(mem, ctx, "LBGT", |c| !c.cc.contains(Flags::Z) && c.cc.contains(Flags::N) == c.cc.contains(Flags::V)),
            0x2F => self.op_lbranch16(mem, ctx, "LBLE", |c| c.cc.contains(Flags::Z) || c.cc.contains(Flags::N) != c.cc.contains(Flags::V)),

            0x3E if self.is_mc6809() => self.exec_xswi2(mem, ctx),
            0x3F => self.exec_swi(mem, ctx, 0xFFF4, "SWI2"),

            0x8D => self.op_branch16(mem, ctx, false), // LBSR

            0x83 => self.op_cmp16_imm(mem, ctx, "CMPD", Reg16::D),
            0x87 if self.is_mc6809() => self.op_xst8_imm(mem, ctx, Reg8::A),
            0x8C => self.op_cmp16_imm(mem, ctx, "CMPY", Reg16::Y),
            0x8E => self.op_ld16_imm(mem, ctx, "LDY", Reg16::Y),
            0x8F if self.is_mc6809() => self.op_xst16_imm(mem, ctx, Reg16::Y),
            0x93 => self.op_cmp16_dir(mem, ctx, "CMPD", Reg16::D),
            0x9C => self.op_cmp16_dir(mem, ctx, "CMPY", Reg16::Y),
            0x9E => self.op_ld16_dir(mem, ctx, "LDY", Reg16::Y),
            0x9F => self.op_st16_dir(mem, ctx, "STY", Reg16::Y),
            0xA3 => self.op_cmp16_idx(mem, ctx, "CMPD", Reg16::D),
            0xAC => self.op_cmp16_idx(mem, ctx, "CMPY", Reg16::Y),
            0xAE => self.op_ld16_idx(mem, ctx, "LDY", Reg16::Y),
            0xAF => self.op_st16_idx(mem, ctx, "STY", Reg16::Y),
            0xB3 => self.op_cmp16_ext(mem, ctx, "CMPD", Reg16::D),
            0xBC => self.op_cmp16_ext(mem, ctx, "CMPY", Reg16::Y),
            0xBE => self.op_ld16_ext(mem, ctx, "LDY", Reg16::Y),
            0xBF => self.op_st16_ext(mem, ctx, "STY", Reg16::Y),
            0xC3 if self.is_mc6809() => self.op_xadd16_imm(mem, ctx, Reg16::D),
            0xC7 if self.is_mc6809() => self.op_xst8_imm(mem, ctx, Reg8::B),
            0xCE => self.op_ld16_imm(mem, ctx, "LDS", Reg16::S),
            0xCF if self.is_mc6809() => self.op_xst16_imm(mem, ctx, Reg16::S),
            0xD3 if self.is_mc6809() => self.op_xadd16_dir(mem, ctx, Reg16::D),
            0xDE => self.op_ld16_dir(mem, ctx, "LDS", Reg16::S),
            0xDF => self.op_st16_dir(mem, ctx, "STS", Reg16::S),
            0xE3 if self.is_mc6809() => self.op_xadd16_idx(mem, ctx, Reg16::D),
            0xEE => self.op_ld16_idx(mem, ctx, "LDS", Reg16::S),
            0xEF => self.op_st16_idx(mem, ctx, "STS", Reg16::S),
            0xF3 if self.is_mc6809() => self.op_xadd16_ext(mem, ctx, Reg16::D),
            0xFE => self.op_ld16_ext(mem, ctx, "LDS", Reg16::S),
            0xFF => self.op_st16_ext(mem, ctx, "STS", Reg16::S),

            _ => {
                if self.is_hd6309() {
                    self.op_illegal_page(mem, ctx, opcode, 2);
                } else {
                    self.exec_page1(opcode, mem, ctx);
                }
            }
        }
    }

    fn exec_page3(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let opcode = self.fetch_imm8(mem, ctx);
        if self.try_hd6309_page3(opcode, mem, ctx) {
            return;
        }
        match opcode {
            0x10 | 0x11 if self.is_mc6809() => self.exec_page3(mem, ctx),
            0x3E if self.is_mc6809() => self.exec_xfirq(mem, ctx),
            0x3F => self.exec_swi(mem, ctx, 0xFFF2, "SWI3"),

            0x83 => self.op_cmp16_imm(mem, ctx, "CMPU", Reg16::U),
            0x87 if self.is_mc6809() => self.op_xst8_imm(mem, ctx, Reg8::A),
            0x8C => self.op_cmp16_imm(mem, ctx, "CMPS", Reg16::S),
            0x8F if self.is_mc6809() => self.op_xst16_imm(mem, ctx, Reg16::X),
            0xC3 if self.is_mc6809() => self.op_xadd16_imm(mem, ctx, Reg16::U),
            0xC7 if self.is_mc6809() => self.op_xst8_imm(mem, ctx, Reg8::B),
            0xCF if self.is_mc6809() => self.op_xst16_imm(mem, ctx, Reg16::U),
            0xD3 if self.is_mc6809() => self.op_xadd16_dir(mem, ctx, Reg16::U),
            0xE3 if self.is_mc6809() => self.op_xadd16_idx(mem, ctx, Reg16::U),
            0xF3 if self.is_mc6809() => self.op_xadd16_ext(mem, ctx, Reg16::U),
            0x93 => self.op_cmp16_dir(mem, ctx, "CMPU", Reg16::U),
            0x9C => self.op_cmp16_dir(mem, ctx, "CMPS", Reg16::S),
            0xA3 => self.op_cmp16_idx(mem, ctx, "CMPU", Reg16::U),
            0xAC => self.op_cmp16_idx(mem, ctx, "CMPS", Reg16::S),
            0xB3 => self.op_cmp16_ext(mem, ctx, "CMPU", Reg16::U),
            0xBC => self.op_cmp16_ext(mem, ctx, "CMPS", Reg16::S),

            _ => {
                if self.is_hd6309() {
                    self.op_illegal_page(mem, ctx, opcode, 3);
                } else {
                    self.exec_page1(opcode, mem, ctx);
                }
            }
        }
    }

    fn resolve_addr(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        mode: AddrMode,
    ) -> (u16, u8, String) {
        match mode {
            AddrMode::Direct => {
                let (addr, op) = self.addr_direct(mem, ctx);
                (addr, 0, op)
            }
            AddrMode::Indexed => {
                let (addr, extra, op) = self.addr_indexed(mem, ctx);
                (addr, extra, op)
            }
            AddrMode::Extended => {
                let (addr, op) = self.addr_extended(mem, ctx);
                (addr, 0, op)
            }
            _ => (0, 0, String::new()),
        }
    }

    pub(crate) fn op_mem_unary<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        mode: AddrMode,
        base_cycles: u32,
        op: F,
    ) where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        let value = mem.read8(addr);
        let result = op(self, value);
        mem.write8(addr, result);
        ctx.cycles = base_cycles + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    pub(crate) fn op_mem_shift<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        mode: AddrMode,
        base_cycles: u32,
        op: F,
    ) where
        F: Fn(u8, &mut Flags) -> u8,
    {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        let value = mem.read8(addr);
        let result = op(value, &mut self.cc);
        mem.write8(addr, result);
        ctx.cycles = base_cycles + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_mem_test(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, mode: AddrMode, base_cycles: u32) {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        self.op_tst8(mem.read8(addr));
        ctx.cycles = base_cycles + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_mem_store<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        mode: AddrMode,
        base_cycles: u32,
        value_fn: F,
    ) where
        F: FnOnce(&mut Self) -> u8,
    {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        let value = value_fn(self);
        mem.write8(addr, value);
        ctx.cycles = base_cycles + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    pub(crate) fn op_reg_unary<F>(
        &mut self,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        cycles: u32,
        op: F,
    ) where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let value = self.get_reg8(reg);
        let result = op(self, value);
        self.set_reg8(reg, result);
        ctx.cycles = cycles;
        ctx.mnemonic = name.into();
        ctx.operands = reg8_name(reg).into();
    }

    pub(crate) fn op_reg_shift<F>(
        &mut self,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        cycles: u32,
        op: F,
    ) where
        F: Fn(u8, &mut Flags) -> u8,
    {
        let value = self.get_reg8(reg);
        let result = op(value, &mut self.cc);
        self.set_reg8(reg, result);
        ctx.cycles = cycles;
        ctx.mnemonic = name.into();
        ctx.operands = reg8_name(reg).into();
    }

    fn op_reg_test(&mut self, ctx: &mut StepCtx, name: &str, reg: Reg8, cycles: u32) {
        self.op_tst8(self.get_reg8(reg));
        ctx.cycles = cycles;
        ctx.mnemonic = name.into();
        ctx.operands = reg8_name(reg).into();
    }

    fn op_reg_clr(&mut self, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let value = self.op_clr8();
        self.set_reg8(reg, value);
        ctx.cycles = 2;
        ctx.mnemonic = name.into();
        ctx.operands = reg8_name(reg).into();
    }

    fn op_ld8_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let value = self.fetch_imm8(mem, ctx);
        self.set_reg8(reg, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 2;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${value:02X}");
    }

    fn op_ld8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read8(addr);
        self.set_reg8(reg, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 4;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_ld8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read8(addr);
        self.set_reg8(reg, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 4 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_ld8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read8(addr);
        self.set_reg8(reg, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = self.get_reg8(reg);
        mem.write8(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 4;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = self.get_reg8(reg);
        mem.write8(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 4 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = self.get_reg8(reg);
        mem.write8(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_alu8_imm<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        op: F,
    ) where
        F: FnOnce(u8, u8, &mut Flags) -> u8,
    {
        let imm = self.fetch_imm8(mem, ctx);
        let value = self.get_reg8(reg);
        let result = op(value, imm, &mut self.cc);
        self.set_reg8(reg, result);
        ctx.cycles = 2;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${imm:02X}");
    }

    fn op_alu8_dir<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        op: F,
    ) where
        F: FnOnce(u8, u8, &mut Flags) -> u8,
    {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        let result = op(value, mem_val, &mut self.cc);
        self.set_reg8(reg, result);
        ctx.cycles = 4;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_alu8_idx<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        op: F,
    ) where
        F: FnOnce(u8, u8, &mut Flags) -> u8,
    {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        let result = op(value, mem_val, &mut self.cc);
        self.set_reg8(reg, result);
        ctx.cycles = 4 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_alu8_ext<F>(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        reg: Reg8,
        op: F,
    ) where
        F: FnOnce(u8, u8, &mut Flags) -> u8,
    {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        let result = op(value, mem_val, &mut self.cc);
        self.set_reg8(reg, result);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_bit8_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let imm = self.fetch_imm8(mem, ctx);
        let value = self.get_reg8(reg);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value & imm);
        ctx.cycles = 2;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${imm:02X}");
    }

    fn op_bit8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value & mem_val);
        ctx.cycles = 4;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_bit8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value & mem_val);
        ctx.cycles = 4 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_bit8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg8) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let mem_val = mem.read8(addr);
        let value = self.get_reg8(reg);
        self.cc.remove(Flags::V);
        self.cc.set_nz8(value & mem_val);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn ld16_cycles(mode: AddrMode, reg: Reg16) -> u32 {
        match (mode, reg) {
            (AddrMode::Immediate, Reg16::Y) | (AddrMode::Immediate, Reg16::S) => 4,
            (AddrMode::Immediate, _) => 3,
            (AddrMode::Direct, Reg16::Y) | (AddrMode::Direct, Reg16::S) => 6,
            (AddrMode::Direct, _) => 5,
            (AddrMode::Indexed, Reg16::Y) | (AddrMode::Indexed, Reg16::S) => 6,
            (AddrMode::Indexed, _) => 5,
            (AddrMode::Extended, Reg16::Y) | (AddrMode::Extended, Reg16::S) => 7,
            (AddrMode::Extended, _) => 6,
            _ => 3,
        }
    }

    fn st16_cycles(mode: AddrMode, reg: Reg16) -> u32 {
        match (mode, reg) {
            (AddrMode::Direct, Reg16::Y) | (AddrMode::Direct, Reg16::S) => 6,
            (AddrMode::Direct, _) => 5,
            (AddrMode::Indexed, Reg16::Y) | (AddrMode::Indexed, Reg16::S) => 6,
            (AddrMode::Indexed, _) => 5,
            (AddrMode::Extended, Reg16::Y) | (AddrMode::Extended, Reg16::S) => 7,
            (AddrMode::Extended, _) => 6,
            _ => 5,
        }
    }

    fn cmp16_cycles(mode: AddrMode, reg: Reg16) -> u32 {
        match (mode, reg) {
            (AddrMode::Immediate, Reg16::X) => 4,
            (AddrMode::Immediate, _) => 5,
            (AddrMode::Direct, Reg16::X) => 6,
            (AddrMode::Direct, _) => 7,
            (AddrMode::Indexed, Reg16::X) => 6,
            (AddrMode::Indexed, _) => 7,
            (AddrMode::Extended, Reg16::X) => 7,
            (AddrMode::Extended, _) => 8,
            _ => 4,
        }
    }

    pub(crate) fn add16_cycles(mode: AddrMode) -> u32 {
        match mode {
            AddrMode::Immediate => 4,
            AddrMode::Direct => 6,
            AddrMode::Indexed => 6,
            AddrMode::Extended => 7,
            _ => 4,
        }
    }

    fn op_ld16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let value = self.fetch_imm16(mem, ctx);
        self.set_reg16(reg, value);
        if reg == Reg16::S {
            self.lds_encountered = true;
        }
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::ld16_cycles(AddrMode::Immediate, reg);
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${value:04X}");
    }

    fn op_ld16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read16(addr);
        self.set_reg16(reg, value);
        if reg == Reg16::S {
            self.lds_encountered = true;
        }
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::ld16_cycles(AddrMode::Direct, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_ld16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read16(addr);
        self.set_reg16(reg, value);
        if reg == Reg16::S {
            self.lds_encountered = true;
        }
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::ld16_cycles(AddrMode::Indexed, reg) + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_ld16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read16(addr);
        self.set_reg16(reg, value);
        if reg == Reg16::S {
            self.lds_encountered = true;
        }
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::ld16_cycles(AddrMode::Extended, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = self.get_reg16(reg);
        mem.write16(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::st16_cycles(AddrMode::Direct, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = self.get_reg16(reg);
        mem.write16(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::st16_cycles(AddrMode::Indexed, reg) + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_st16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = self.get_reg16(reg);
        mem.write16(addr, value);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(value);
        ctx.cycles = Self::st16_cycles(AddrMode::Extended, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_cmp16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let value = self.fetch_imm16(mem, ctx);
        let current = self.get_reg16(reg);
        cmp16(current, value, &mut self.cc);
        ctx.cycles = Self::cmp16_cycles(AddrMode::Immediate, reg);
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${value:04X}");
    }

    fn op_cmp16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        cmp16(current, value, &mut self.cc);
        ctx.cycles = Self::cmp16_cycles(AddrMode::Direct, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_cmp16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        cmp16(current, value, &mut self.cc);
        ctx.cycles = Self::cmp16_cycles(AddrMode::Indexed, reg) + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_cmp16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        cmp16(current, value, &mut self.cc);
        ctx.cycles = Self::cmp16_cycles(AddrMode::Extended, reg);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_add16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let value = self.fetch_imm16(mem, ctx);
        let current = self.get_reg16(reg);
        let result = add16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Immediate);
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${value:04X}");
    }

    fn op_add16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = add16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Direct);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_add16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = add16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Indexed) + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_add16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = add16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Extended);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_sub16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let value = self.fetch_imm16(mem, ctx);
        let current = self.get_reg16(reg);
        let result = sub16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Immediate);
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${value:04X}");
    }

    fn op_sub16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_direct(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = sub16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Direct);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_sub16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = sub16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Indexed) + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_sub16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, reg: Reg16) {
        let (addr, operand) = self.addr_extended(mem, ctx);
        let value = mem.read16(addr);
        let current = self.get_reg16(reg);
        let result = sub16(current, value, &mut self.cc);
        self.set_reg16(reg, result);
        ctx.cycles = Self::add16_cycles(AddrMode::Extended);
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_jmp(
        &mut self,
        mem: &mut Memory,
        ctx: &mut StepCtx,
        name: &str,
        mode: AddrMode,
        base_cycles: u32,
    ) {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        self.pc = addr;
        ctx.cycles = base_cycles + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_jsr(&mut self, mem: &mut Memory, ctx: &mut StepCtx, mode: AddrMode) {
        let (addr, extra, operand) = self.resolve_addr(mem, ctx, mode);
        let return_addr = self.pc;
        self.push16(mem, return_addr);
        self.pc = addr;
        ctx.cycles = match mode {
            AddrMode::Direct => 7,
            AddrMode::Indexed => 7 + extra as u32,
            AddrMode::Extended => 8,
            _ => 7,
        };
        ctx.mnemonic = "JSR".into();
        ctx.operands = operand;
    }

    fn op_bsr(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (target, operand) = self.addr_relative8(mem, ctx);
        let return_addr = self.pc;
        self.push16(mem, return_addr);
        self.pc = target;
        ctx.cycles = 7;
        ctx.mnemonic = "BSR".into();
        ctx.operands = operand;
    }

    fn op_branch8<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, cond: F)
    where
        F: FnOnce(&Cpu) -> bool,
    {
        let (target, operand) = self.addr_relative8(mem, ctx);
        if cond(self) {
            self.pc = target;
            ctx.cycles = 3;
        } else {
            ctx.cycles = 2;
        }
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    fn op_branch16(&mut self, mem: &mut Memory, ctx: &mut StepCtx, is_branch: bool) {
        let (target, operand) = self.addr_relative16(mem, ctx);
        if is_branch {
            ctx.mnemonic = "LBRA".into();
            self.pc = target;
            ctx.cycles = 5;
        } else {
            ctx.mnemonic = "LBSR".into();
            self.push16(mem, self.pc);
            self.pc = target;
            ctx.cycles = 9;
        }
        ctx.operands = operand;
    }

    fn op_lbranch16<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, cond: F)
    where
        F: FnOnce(&Cpu) -> bool,
    {
        let (target, operand) = self.addr_relative16(mem, ctx);
        if cond(self) {
            self.pc = target;
            ctx.cycles = 6;
        } else {
            ctx.cycles = 5;
        }
        ctx.mnemonic = name.into();
        ctx.operands = operand;
    }

    pub(crate) fn op_lea(&mut self, mem: &mut Memory, ctx: &mut StepCtx, dest: Reg16) {
        let (addr, extra, operand) = self.addr_indexed(mem, ctx);
        self.set_reg16(dest, addr);
        // LEAX/LEAY set Z (6800 INX/DEX compat); LEAS/LEAU leave CC alone.
        match dest {
            Reg16::X | Reg16::Y => {
                self.cc.set(Flags::Z, addr == 0);
            }
            _ => {}
        }
        ctx.cycles = 4 + extra as u32;
        ctx.mnemonic = match dest {
            Reg16::X => "LEAX",
            Reg16::Y => "LEAY",
            Reg16::S => "LEAS",
            Reg16::U => "LEAU",
            _ => "LEA",
        }
        .into();
        ctx.operands = operand;
    }

    fn push8_u(&mut self, mem: &mut Memory, value: u8) {
        self.u = self.u.wrapping_sub(1);
        mem.write8(self.u, value);
    }

    pub(crate) fn push16_u(&mut self, mem: &mut Memory, value: u16) {
        self.push8_u(mem, (value & 0xFF) as u8);
        self.push8_u(mem, (value >> 8) as u8);
    }

    fn pull8_u(&mut self, mem: &Memory) -> u8 {
        let value = mem.read8(self.u);
        self.u = self.u.wrapping_add(1);
        value
    }

    pub(crate) fn pull16_u(&mut self, mem: &Memory) -> u16 {
        let hi = self.pull8_u(mem) as u16;
        let lo = self.pull8_u(mem) as u16;
        (hi << 8) | lo
    }

    fn op_psh(&mut self, mem: &mut Memory, ctx: &mut StepCtx, use_s: bool) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let mut cycles = 5u32;
        // 6809 stack order: PC, U/S, Y, X, DP, B, A, CC (high to low bit)
        for bit in (0..8).rev() {
            if postbyte & (1 << bit) == 0 {
                continue;
            }
            match bit {
                0 => { // CC
                    if use_s { self.push8(mem, self.cc.bits()); } else { self.push8_u(mem, self.cc.bits()); }
                    cycles += 1;
                }
                1 => { // A
                    if use_s { self.push8(mem, self.a); } else { self.push8_u(mem, self.a); }
                    cycles += 1;
                }
                2 => { // B
                    if use_s { self.push8(mem, self.b); } else { self.push8_u(mem, self.b); }
                    cycles += 1;
                }
                3 => { // DP
                    if use_s { self.push8(mem, self.dp); } else { self.push8_u(mem, self.dp); }
                    cycles += 1;
                }
                4 => { // X
                    if use_s { self.push16(mem, self.x); } else { self.push16_u(mem, self.x); }
                    cycles += 2;
                }
                5 => { // Y
                    if use_s { self.push16(mem, self.y); } else { self.push16_u(mem, self.y); }
                    cycles += 2;
                }
                6 => { // U/S (the other stack pointer)
                    if use_s {
                        self.push16(mem, self.u);
                    } else {
                        self.push16_u(mem, self.s);
                    }
                    cycles += 2;
                }
                7 => { // PC
                    if use_s { self.push16(mem, self.pc); } else { self.push16_u(mem, self.pc); }
                    cycles += 2;
                }
                _ => {}
            }
        }
        ctx.cycles = cycles;
        ctx.mnemonic = if use_s { "PSHS" } else { "PSHU" }.into();
        ctx.operands = format!("${postbyte:02X}");
    }

    fn op_pul(&mut self, mem: &mut Memory, ctx: &mut StepCtx, use_s: bool) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let mut cycles = 5u32;
        // 6809 pull order: CC, A, B, DP, X, Y, U/S, PC (low to high bit)
        for bit in 0..8 {
            if postbyte & (1 << bit) == 0 {
                continue;
            }
            match bit {
                0 => { // CC
                    let val = if use_s { self.pull8(mem) } else { self.pull8_u(mem) };
                    self.cc = Flags::from_byte(val);
                    cycles += 1;
                }
                1 => { // A
                    self.a = if use_s { self.pull8(mem) } else { self.pull8_u(mem) };
                    cycles += 1;
                }
                2 => { // B
                    self.b = if use_s { self.pull8(mem) } else { self.pull8_u(mem) };
                    cycles += 1;
                }
                3 => { // DP
                    self.dp = if use_s { self.pull8(mem) } else { self.pull8_u(mem) };
                    cycles += 1;
                }
                4 => { // X
                    self.x = if use_s { self.pull16(mem) } else { self.pull16_u(mem) };
                    cycles += 2;
                }
                5 => { // Y
                    self.y = if use_s { self.pull16(mem) } else { self.pull16_u(mem) };
                    cycles += 2;
                }
                6 => { // U/S
                    if use_s {
                        self.u = self.pull16(mem);
                    } else {
                        self.s = self.pull16_u(mem);
                    }
                    cycles += 2;
                }
                7 => { // PC
                    self.pc = if use_s { self.pull16(mem) } else { self.pull16_u(mem) };
                    cycles += 2;
                }
                _ => {}
            }
        }
        ctx.cycles = cycles;
        ctx.mnemonic = if use_s { "PULS" } else { "PULU" }.into();
        ctx.operands = format!("${postbyte:02X}");
    }

    fn op_tfr(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src_code = postbyte >> 4;
        let dst_code = postbyte & 0x0F;
        let value = self.read_tfr_reg(src_code);
        self.write_tfr_reg(dst_code, value);
        if dst_code == 4 {
            self.lds_encountered = true;
        }
        ctx.cycles = if self.is_hd6309() { 5 } else { 6 };
        ctx.mnemonic = "TFR".into();
        ctx.operands = format!(
            "{},{}",
            tfr_reg_name(src_code),
            tfr_reg_name(dst_code)
        );
    }

    fn op_exg(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src_code = postbyte >> 4;
        let dst_code = postbyte & 0x0F;
        let (reg1, reg2) = if postbyte & 0x80 != 0 {
            (
                self.read_tfr_reg(src_code),
                self.read_tfr_reg(dst_code),
            )
        } else {
            (
                self.read_exg_reg_8first(src_code),
                self.read_exg_reg_8first(dst_code),
            )
        };
        self.write_tfr_reg(dst_code, reg1);
        self.write_tfr_reg(src_code, reg2);
        // Any write to S (including EXG) arms NMI after reset, same as LDS/TFR.
        if src_code == 4 || dst_code == 4 {
            self.lds_encountered = true;
        }
        ctx.cycles = if self.is_hd6309() { 7 } else { 8 };
        ctx.mnemonic = "EXG".into();
        ctx.operands = format!(
            "{},{}",
            tfr_reg_name(src_code),
            tfr_reg_name(dst_code)
        );
    }

    fn dup8(byte: u8) -> u16 {
        u16::from(byte) << 8 | u16::from(byte)
    }

    fn read_tfr_reg(&self, code: u8) -> u16 {
        if self.is_hd6309() {
            return match code {
                0x0..=0x7 => self.get_hd6309_reg16(code),
                0x8 => Self::dup8(self.a),
                0x9 => Self::dup8(self.b),
                0xA => Self::dup8(self.cc.bits()),
                0xB => Self::dup8(self.dp),
                0xC | 0xD => 0,
                0xE => Self::dup8((self.w >> 8) as u8),
                0xF => Self::dup8(self.w as u8),
                _ => 0,
            };
        }
        match code {
            0x0 => self.get_reg16(Reg16::D),
            0x1 => self.x,
            0x2 => self.y,
            0x3 => self.u,
            0x4 => self.s,
            0x5 => self.pc,
            0x8 => 0xFF00 | u16::from(self.a),
            0x9 => 0xFF00 | u16::from(self.b),
            0xA => Self::dup8(self.cc.bits()),
            0xB => Self::dup8(self.dp),
            _ => 0xFFFF,
        }
    }

    fn read_exg_reg_8first(&self, code: u8) -> u16 {
        if self.is_hd6309() {
            return match code {
                0x0..=0x7 => self.get_hd6309_reg16(code),
                0x8 => 0xFF00 | u16::from(self.a),
                0x9 => 0xFF00 | u16::from(self.b),
                0xA => 0xFF00 | u16::from(self.cc.bits()),
                0xB => 0xFF00 | u16::from(self.dp),
                0xC | 0xD => 0,
                0xE => 0xFF00 | u16::from((self.w >> 8) as u8),
                0xF => 0xFF00 | u16::from(self.w as u8),
                _ => 0xFFFF,
            };
        }
        match code {
            0x0 => self.get_reg16(Reg16::D),
            0x1 => self.x,
            0x2 => self.y,
            0x3 => self.u,
            0x4 => self.s,
            0x5 => self.pc,
            0x8 => 0xFF00 | u16::from(self.a),
            0x9 => 0xFF00 | u16::from(self.b),
            0xA => 0xFF00 | u16::from(self.cc.bits()),
            0xB => 0xFF00 | u16::from(self.dp),
            _ => 0xFFFF,
        }
    }

    fn write_tfr_reg(&mut self, code: u8, value: u16) {
        if self.is_hd6309() {
            match code {
                0x0..=0x7 => self.set_hd6309_reg16(code, value),
                0x8 => self.a = (value >> 8) as u8,
                0x9 => self.b = value as u8,
                0xA => self.cc = Flags::from_byte(value as u8),
                0xB => self.dp = (value >> 8) as u8,
                0xC | 0xD => {}
                0xE => self.w = (self.w & 0x00FF) | ((value >> 8) << 8),
                0xF => self.w = (self.w & 0xFF00) | value,
                _ => {}
            }
            return;
        }
        match code {
            0x0 => self.set_reg16(Reg16::D, value),
            0x1 => self.x = value,
            0x2 => self.y = value,
            0x3 => self.u = value,
            0x4 => self.s = value,
            0x5 => self.pc = value,
            0x8 => self.a = value as u8,
            0x9 => self.b = value as u8,
            0xA => self.cc = Flags::from_byte(value as u8),
            0xB => self.dp = value as u8,
            _ => {}
        }
    }

    fn op_illegal(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        ctx.cycles = 1;
        ctx.mnemonic = "???".into();
        if self.is_hd6309() {
            self.enter_hw_trap(mem, 0x40);
            ctx.cycles = 19;
            ctx.mnemonic = "TRAP".into();
            ctx.operands = "$FFF0".into();
        }
        ctx.trap = Some(Trap::IllegalOpcode);
    }

    fn op_illegal_page(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8, page: u8) {
        ctx.cycles = 1;
        ctx.mnemonic = "???".into();
        ctx.operands = format!("page{page}:${opcode:02X}");
        if self.is_hd6309() {
            self.enter_hw_trap(mem, 0x40);
            ctx.cycles = 19;
            ctx.mnemonic = "TRAP".into();
            ctx.operands = "$FFF0".into();
        }
        ctx.trap = Some(Trap::IllegalOpcode);
    }
}

fn reg8_name(reg: Reg8) -> &'static str {
    match reg {
        Reg8::A => "A",
        Reg8::B => "B",
        Reg8::Dp => "DP",
    }
}

fn tfr_reg_name(code: u8) -> &'static str {
    match code {
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
        0xE => "E",
        0xF => "F",
        _ => "?",
    }
}

fn format_indexed_operand(_index_reg: Option<char>, postbyte: u8, bytes: &[u8]) -> String {
    // The extra bytes in `bytes` start at index 1 (index 0 is the opcode).
    // The postbyte is at bytes[0] for indexed ops, but here we receive the
    // already-fetched postbyte separately. Extra operand bytes follow.
    let extra = if bytes.len() > 1 { &bytes[1..] } else { &[] };
    crate::addressing::format_index_operand(postbyte, extra)
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_with_program(pc: u16, bytes: &[u8]) -> Memory {
        let mut mem = Memory::new();
        mem.load_binary(pc, bytes).unwrap();
        mem
    }

    #[test]
    fn nop_advances_pc() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x12]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NOP");
        assert_eq!(cpu.pc, 0x0101);
        assert_eq!(step.cycles, 2);
    }

    #[test]
    fn lda_immediate_loads_a() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x86, 0x42]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.a, 0x42);
        assert!(!cpu.cc.contains(Flags::Z));
        assert_eq!(cpu.pc, 0x0102);
    }

    #[test]
    fn ldb_immediate_sets_zero_flag() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0xC6, 0x00]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.b, 0x00);
        assert!(cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn reset_reads_vector() {
        let mut cpu = Cpu::new();
        let mut mem = Memory::new();
        mem.write16(0xFFFE, 0x8000);
        cpu.reset(&mem);
        assert_eq!(cpu.pc, 0x8000);
        assert!(cpu.cc.contains(Flags::I));
        assert!(cpu.cc.contains(Flags::F));
    }

    #[test]
    fn bra_taken_updates_pc() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x20, 0x05, 0x12, 0x12, 0x12, 0x12, 0x12]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0107);
    }

    #[test]
    fn adda_immediate() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        let mut mem = mem_with_program(0x0100, &[0x8B, 0x02]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.a, 0x12);
    }

    #[test]
    fn ldx_immediate() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x8E, 0x12, 0x34]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x1234);
        assert_eq!(cpu.pc, 0x0103);
    }

    #[test]
    fn undoc_neg_direct_alias_on_mc6809() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x01, 0x20]);
        mem.write8(0x0020, 0x01);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NEG");
        assert!(step.trap.is_none());
    }

    #[test]
    fn page2_ldy_immediate() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x10, 0x8E, 0xAB, 0xCD]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LDY");
        assert_eq!(cpu.y, 0xABCD);
    }

    #[test]
    fn sync_halts_without_interrupt() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x13]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert!(cpu.halted);
    }

    #[test]
    fn clra_clears_accumulator() {
        let mut cpu = Cpu::new();
        cpu.a = 0x55;
        let mut mem = mem_with_program(0x0100, &[0x4F]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.cc.contains(Flags::Z));
    }

    #[test]
    fn inx_dex_iny_dey() {
        let mut cpu = Cpu::new();
        cpu.x = 0x0100;
        cpu.y = 0x0200;
        // INX=LEAX 1,X (0x30 0x01), DEX=LEAX -1,X (0x30 0x1F),
        // INY=LEAY 1,Y (0x31 0x21), DEY=LEAY -1,Y (0x31 0x3F)
        let mut mem = mem_with_program(0x0100, &[0x30, 0x01, 0x30, 0x1F, 0x31, 0x21, 0x31, 0x3F]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x0101);
        cpu.step(&mut mem);
        assert_eq!(cpu.x, 0x0100);
        cpu.step(&mut mem);
        assert_eq!(cpu.y, 0x0201);
        cpu.step(&mut mem);
        assert_eq!(cpu.y, 0x0200);
    }

    #[test]
    fn orcc_andcc_update_flags() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x1A, 0x01, 0x1C, 0xFE]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert!(cpu.cc.contains(Flags::C));
        cpu.step(&mut mem);
        assert!(!cpu.cc.contains(Flags::C));
    }

    #[test]
    fn tfr_exg_separate_opcodes() {
        let mut cpu = Cpu::new();
        cpu.a = 0xAA;
        cpu.b = 0xBB;
        // TFR A,B (0x1F 0x89): A=8, B=9 → copy A→B
        let mut mem = mem_with_program(0x0100, &[0x1F, 0x89]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.a, 0xAA);
        assert_eq!(cpu.b, 0xAA); // B gets A's value

        // EXG A,B (0x1E 0x89): swap A and B
        cpu.a = 0x11;
        cpu.b = 0x22;
        let mut mem2 = mem_with_program(0x0100, &[0x1E, 0x89]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem2);
        assert_eq!(cpu.a, 0x22);
        assert_eq!(cpu.b, 0x11);
    }

    #[test]
    fn watchpoint_traps_on_store() {
        let mut cpu = Cpu::new();
        cpu.a = 0x42;
        let mut mem = mem_with_program(0x0100, &[0x97, 0x20]);
        mem.set_watchpoint(0x0020);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.trap, Some(Trap::Watchpoint));
        assert_eq!(mem.read8(0x0020), 0x42);
    }

    #[test]
    fn native_irq_pushes_w_on_stack() {
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.mode_reg = 0x01;
        cpu.w = 0xBEEF;
        cpu.s = 0x0200;
        cpu.pc = 0x0100;
        cpu.cc.remove(Flags::I);
        let mut mem = Memory::new();
        mem.write16(0xFFF8, 0x0300);
        cpu.irq_pending = true;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0300);
        assert_eq!(mem.read8(0x01F6), 0xEF);
        assert_eq!(mem.read8(0x01F5), 0xBE);
    }

    #[test]
    fn firq_full_save_when_md_bit1() {
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.mode_reg = 0x03;
        cpu.w = 0x1234;
        cpu.s = 0x0200;
        cpu.pc = 0x0100;
        cpu.cc.remove(Flags::F);
        let mut mem = Memory::new();
        mem.write16(0xFFF6, 0x0400);
        cpu.firq_pending = true;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0400);
        assert_eq!(mem.read8(0x01F6), 0x34);
        assert_eq!(mem.read8(0x01F5), 0x12);
    }

    #[test]
    fn hd6309_illegal_opcode_traps_to_fff0() {
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.pc = 0x0100;
        let mut mem = mem_with_program(0x0100, &[0x01]);
        mem.write16(0xFFF0, 0x0500);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "TRAP");
        assert_eq!(cpu.pc, 0x0500);
        assert_ne!(cpu.mode_reg & 0x40, 0);
    }

    #[test]
    fn jsr_ext_pushes_return_after_operand() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        let mut mem = mem_with_program(0x0100, &[0xBD, 0x02, 0x00]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0200);
        assert_eq!(mem.read16(0x0EFE), 0x0103, "JSR must push address after instruction");
    }

    #[test]
    fn jsr_idx_pushes_return_after_postbyte() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        cpu.x = 0x0200;
        let mut mem = mem_with_program(0x0100, &[0xAD, 0x84]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0200);
        assert_eq!(mem.read16(0x0EFE), 0x0102, "JSR ,X must push address after postbyte");
    }

    #[test]
    fn bsr_pushes_return_after_offset() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        let mut mem = mem_with_program(0x0100, &[0x8D, 0x10]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0112);
        assert_eq!(mem.read16(0x0EFE), 0x0102, "BSR must push address after offset byte");
    }

    #[test]
    fn mul_sets_carry_from_bit7_of_result() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.b = 0x08;
        let mut mem = mem_with_program(0x0100, &[0x3D]);
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.a, 0x00);
        assert_eq!(cpu.b, 0x80);
        assert!(cpu.cc.contains(Flags::C), "MUL 0x10*0x08=0x80 -> C=1 (bit7 of low byte set)");
    }

    #[test]
    fn mul_clears_carry_when_bit7_clear() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.b = 0x10;
        let mut mem = mem_with_program(0x0100, &[0x3D]);
        cpu.pc = 0x0100;
        cpu.cc.insert(Flags::C);
        cpu.step(&mut mem);
        assert_eq!(cpu.b, 0x00);
        assert!(!cpu.cc.contains(Flags::C), "MUL 0x10*0x10=0x100 -> C=0 (bit7 of low byte clear)");
    }

    #[test]
    fn rti_restores_w_after_irq_native_6309() {
        let mut cpu = Cpu::new();
        cpu.variant = CpuVariant::Hd6309;
        cpu.mode_reg = 0x01;
        cpu.w = 0xBEEF;
        cpu.s = 0x0200;
        cpu.pc = 0x0100;
        cpu.cc.remove(Flags::I);
        let mut mem = Memory::new();
        mem.write16(0xFFF8, 0x0400);
        cpu.irq_pending = true;
        cpu.step(&mut mem);
        assert_eq!(cpu.pc, 0x0400);
        cpu.w = 0;
        cpu.pc = 0x0100;
        mem.write8(0x0100, 0x3B);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "RTI");
        assert_eq!(cpu.w, 0xBEEF, "RTI must restore W in correct byte order");
    }

    #[test]
    fn cwai_decodes_from_opcode_3c() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x3C, 0xFF]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "CWAI");
        assert_eq!(step.operands, "#$FF");
    }

    #[test]
    fn cwai_pushes_full_frame_with_u_and_e() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0200;
        cpu.u = 0x1234;
        cpu.pc = 0x0100;
        cpu.cc = Flags::from_bits_truncate(0x00);
        let mut mem = mem_with_program(0x0100, &[0x3C, 0xFF]);
        cpu.step(&mut mem);
        let cc_byte = mem.read8(0x01F4);
        assert!(cc_byte & 0x80 != 0, "CWAI must set E flag in pushed CC (at $01F4)");
        let u_hi = mem.read8(0x01FC);
        let u_lo = mem.read8(0x01FD);
        assert_eq!((u_hi as u16) << 8 | u_lo as u16, 0x1234, "CWAI must push U register (at $01FC)");
    }

    #[test]
    fn opcode_14_is_hcf_on_mc6809() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x14]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "HCF");
        assert!(cpu.free_run);
    }

    #[test]
    fn pshs_cc_pushes_cc_register() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        cpu.cc = Flags::from_bits_truncate(0xA5);
        let mut mem = mem_with_program(0x0100, &[0x34, 0x01]); // PSHS CC
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(mem.read8(0x0EFF), 0xA5, "PSHS CC must push CC byte");
    }

    #[test]
    fn puls_cc_restores_cc_register() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        cpu.cc = Flags::from_bits_truncate(0x00);
        let mut mem = Memory::new();
        mem.write8(0x0F00, 0xFF);
        let _ = mem.load_binary(0x0100, &[0x35, 0x01]); // PULS CC
        cpu.pc = 0x0100;
        cpu.step(&mut mem);
        assert_eq!(cpu.cc.bits(), 0xFF, "PULS CC must restore CC from stack");
    }

    #[test]
    fn pshs_full_roundtrip_all_registers() {
        let mut cpu = Cpu::new();
        cpu.s = 0x0F00;
        cpu.a = 0x11;
        cpu.b = 0x22;
        cpu.dp = 0x33;
        cpu.x = 0x4444;
        cpu.y = 0x5555;
        cpu.u = 0x6666;
        cpu.pc = 0x0100;
        cpu.cc = Flags::from_bits_truncate(0x77);
        // PSHS CC,A,B,DP,X,Y,U,PC = 0xFF
        let mut mem = mem_with_program(0x0100, &[0x34, 0xFF, 0x35, 0xFF]);
        cpu.step(&mut mem); // PSHS
        // Clear all registers
        cpu.a = 0; cpu.b = 0; cpu.dp = 0; cpu.x = 0; cpu.y = 0; cpu.u = 0; cpu.cc = Flags::from_bits_truncate(0);
        cpu.step(&mut mem); // PULS
        assert_eq!(cpu.a, 0x11);
        assert_eq!(cpu.b, 0x22);
        assert_eq!(cpu.dp, 0x33);
        assert_eq!(cpu.x, 0x4444);
        assert_eq!(cpu.y, 0x5555);
        assert_eq!(cpu.u, 0x6666);
    }

    #[test]
    fn reset_clears_w_v_mode_reg() {
        let mut cpu = Cpu::new();
        cpu.w = 0x1234;
        cpu.v = 0x5678;
        cpu.mode_reg = 0xFF;
        let mut mem = Memory::new();
        mem.write16(0xFFFE, 0x0100);
        cpu.reset(&mem);
        assert_eq!(cpu.w, 0, "reset must clear W");
        assert_eq!(cpu.v, 0, "reset must clear V");
        assert_eq!(cpu.mode_reg, 0, "reset must clear MD");
    }

    #[test]
    fn branch_taken_is_3_cycles() {
        let mut cpu = Cpu::new();
        let mut mem = mem_with_program(0x0100, &[0x20, 0x05]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.cycles, 3, "BRA taken = 3 cycles");
    }

    #[test]
    fn branch_not_taken_is_2_cycles() {
        let mut cpu = Cpu::new();
        // BNE with Z flag set → not taken
        cpu.cc.insert(Flags::Z);
        let mut mem = mem_with_program(0x0100, &[0x26, 0x05]);
        cpu.pc = 0x0100;
        let step = cpu.step(&mut mem);
        assert_eq!(step.cycles, 2, "BNE not taken = 2 cycles");
        assert_eq!(cpu.pc, 0x0102, "PC must advance past branch instruction");
    }
}