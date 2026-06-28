use serde::{Deserialize, Serialize};

use crate::cpu::Cpu;
use crate::memory::Memory;
use crate::types::{CpuState, CpuVariant, LoadConfig, StepResult, Trap};

/// Complete M6809 emulator combining CPU and memory.
#[derive(Debug, Clone)]
pub struct Emulator {
    pub cpu: Cpu,
    pub memory: Memory,
}

impl Emulator {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            memory: Memory::new(),
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset(&self.memory);
    }

    pub fn step(&mut self) -> StepResult {
        let result = self.cpu.step(&mut self.memory);
        if let Some(io) = &mut self.memory.io {
            io.tick(result.cycles);
            if io.poll_irq() {
                self.trigger_irq();
            }
        }
        result
    }

    pub fn run(&mut self, max_cycles: u64) -> u64 {
        let start = self.cpu.total_cycles;
        while self.cpu.total_cycles - start < max_cycles && !self.cpu.halted {
            let result = self.step();
            if matches!(
                result.trap,
                Some(Trap::Halted) | Some(Trap::Breakpoint) | Some(Trap::Watchpoint)
            ) {
                break;
            }
        }
        self.cpu.total_cycles - start
    }

    pub fn load_program(&mut self, offset: u16, data: &[u8]) -> Result<(), String> {
        self.memory.load_binary(offset, data)
    }

    pub fn load_and_reset(&mut self, offset: u16, data: &[u8], reset_pc: u16) -> Result<(), String> {
        self.memory.load_binary(offset, data)?;
        self.memory.write16(0xFFFE, reset_pc);
        self.cpu.reset(&self.memory);
        Ok(())
    }

    pub fn get_state(&self) -> CpuState {
        self.cpu.get_state()
    }

    pub fn set_breakpoint(&mut self, addr: u16) {
        self.cpu.breakpoints.insert(addr);
    }

    pub fn clear_breakpoint(&mut self, addr: u16) {
        self.cpu.breakpoints.remove(&addr);
    }

    pub fn trigger_irq(&mut self) {
        self.cpu.irq_pending = true;
        self.cpu.halted = false;
        self.cpu.sync_waiting = false;
    }

    pub fn trigger_firq(&mut self) {
        self.cpu.firq_pending = true;
        self.cpu.halted = false;
        self.cpu.sync_waiting = false;
    }

    pub fn trigger_nmi(&mut self) {
        if self.cpu.lds_encountered {
            self.cpu.nmi_pending = true;
            self.cpu.halted = false;
            self.cpu.sync_waiting = false;
        }
    }

    pub fn set_variant(&mut self, variant: CpuVariant) {
        self.cpu.variant = variant;
        if variant == CpuVariant::Hd6309 {
            self.cpu.mode_reg |= 0x01;
        }
    }

    pub fn get_variant(&self) -> CpuVariant {
        self.cpu.variant
    }

    pub fn set_register(&mut self, register: &str, value: u16) -> Result<(), String> {
        self.cpu.set_register(register, value)
    }

    pub fn toggle_flag(&mut self, flag: &str) -> Result<(), String> {
        self.cpu.toggle_flag(flag)
    }

    pub fn get_breakpoints(&self) -> Vec<u16> {
        let mut addrs: Vec<u16> = self.cpu.breakpoints.iter().copied().collect();
        addrs.sort_unstable();
        addrs
    }

    pub fn clear_all_breakpoints(&mut self) {
        self.cpu.breakpoints.clear();
    }

    pub fn set_watchpoint(&mut self, addr: u16) {
        self.memory.set_watchpoint(addr);
    }

    pub fn clear_watchpoint(&mut self, addr: u16) {
        self.memory.clear_watchpoint(addr);
    }

    pub fn clear_all_watchpoints(&mut self) {
        self.memory.clear_all_watchpoints();
    }

    pub fn get_watchpoints(&self) -> Vec<u16> {
        self.memory.get_watchpoints()
    }

    pub fn snapshot(&self) -> EmulatorSnapshot {
        let (machine_kind, machine_state) = if let Some(io) = &self.memory.io {
            (io.kind_id().to_string(), io.snapshot())
        } else {
            ("bare".to_string(), serde_json::Value::Null)
        };
        EmulatorSnapshot {
            cpu: self.cpu.clone(),
            ram: self.memory.ram.to_vec(),
            config: self.memory.config.clone(),
            watchpoints: self.memory.get_watchpoints(),
            machine_kind,
            machine_state,
        }
    }

    pub fn restore(&mut self, snap: &EmulatorSnapshot) -> Result<(), String> {
        if snap.ram.len() != 0x10000 {
            return Err(format!(
                "Session RAM size {} is invalid (expected 65536 bytes)",
                snap.ram.len()
            ));
        }
        self.cpu = snap.cpu.clone();
        self.memory.ram.copy_from_slice(&snap.ram);
        self.memory.config = snap.config.clone();
        self.memory.io = None;
        self.memory.clear_all_watchpoints();
        for addr in &snap.watchpoints {
            self.memory.set_watchpoint(*addr);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorSnapshot {
    pub cpu: Cpu,
    pub ram: Vec<u8>,
    pub config: LoadConfig,
    pub watchpoints: Vec<u16>,
    #[serde(default = "default_machine_kind")]
    pub machine_kind: String,
    #[serde(default)]
    pub machine_state: serde_json::Value,
}

fn default_machine_kind() -> String {
    "bare".to_string()
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CpuVariant;

    #[test]
    fn emulator_load_and_step() {
        let mut emu = Emulator::new();
        emu.load_and_reset(0x0100, &[0x86, 0x7B, 0x12], 0x0100).unwrap();
        let step = emu.step();
        assert_eq!(step.mnemonic, "LDA");
        assert_eq!(emu.cpu.a, 0x7B);
    }

    #[test]
    fn emulator_run_consumes_cycles() {
        let mut emu = Emulator::new();
        emu.load_and_reset(0x0100, &[0x12, 0x12, 0x12, 0x12, 0x12], 0x0100).unwrap();
        let consumed = emu.run(10);
        assert_eq!(consumed, 10);
        assert_eq!(emu.cpu.pc, 0x0105);
    }

    #[test]
    fn snapshot_preserves_hd6309_state() {
        let mut emu = Emulator::new();
        emu.set_variant(CpuVariant::Hd6309);
        emu.cpu.w = 0x1234;
        emu.cpu.v = 0x5678;
        emu.cpu.mode_reg = 0x01;
        let snap = emu.snapshot();
        let mut restored = Emulator::new();
        restored.restore(&snap).unwrap();
        assert_eq!(restored.cpu.variant, CpuVariant::Hd6309);
        assert_eq!(restored.cpu.w, 0x1234);
        assert_eq!(restored.cpu.v, 0x5678);
        assert_eq!(restored.cpu.mode_reg, 0x01);
    }

    #[test]
    fn restore_rejects_invalid_ram_size() {
        let mut emu = Emulator::new();
        let mut snap = emu.snapshot();
        snap.ram.truncate(100);
        let err = emu.restore(&snap).unwrap_err();
        assert!(err.contains("65536"));
    }
}