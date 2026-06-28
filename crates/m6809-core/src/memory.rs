use std::collections::HashSet;

use crate::io_map::{IoWriteResult, MemoryIo};
use crate::types::LoadConfig;

#[derive(Debug)]
pub struct Memory {
    pub ram: [u8; 0x10000],
    pub config: LoadConfig,
    pub io: Option<Box<dyn MemoryIo>>,
    watchpoints: HashSet<u16>,
    watchpoint_trigger: Option<u16>,
}

impl Clone for Memory {
    fn clone(&self) -> Self {
        Self {
            ram: self.ram,
            config: self.config.clone(),
            io: self.io.as_ref().map(|io| io.clone_box()),
            watchpoints: self.watchpoints.clone(),
            watchpoint_trigger: self.watchpoint_trigger,
        }
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Self {
            ram: [0; 0x10000],
            config: LoadConfig::default(),
            io: None,
            watchpoints: HashSet::new(),
            watchpoint_trigger: None,
        }
    }

    pub fn read8(&self, addr: u16) -> u8 {
        if let Some(io) = &self.io {
            if let Some(value) = io.read(addr, &self.ram) {
                return value;
            }
        }
        self.ram[addr as usize]
    }

    pub fn write8(&mut self, addr: u16, value: u8) {
        if let Some(io) = self.io.as_mut() {
            match io.write(addr, value, &mut self.ram) {
                IoWriteResult::Consumed | IoWriteResult::Ignored => return,
                IoWriteResult::PassThrough => {}
            }
        }
        self.ram[addr as usize] = value;
        if self.watchpoints.contains(&addr) {
            self.watchpoint_trigger = Some(addr);
        }
    }

    pub fn read16(&self, addr: u16) -> u16 {
        let hi = self.read8(addr) as u16;
        let lo = self.read8(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    pub fn write16(&mut self, addr: u16, value: u16) {
        self.write8(addr, (value >> 8) as u8);
        self.write8(addr.wrapping_add(1), value as u8);
    }

    pub fn load_binary(&mut self, offset: u16, data: &[u8]) -> Result<(), String> {
        let start = offset as usize;
        let end = start + data.len();
        if end > 0x10000 {
            return Err(format!(
                "Binary too large: {} bytes at ${offset:04X} exceeds 64KB",
                data.len()
            ));
        }
        self.ram[start..end].copy_from_slice(data);
        Ok(())
    }

    pub fn export_range(&self, start: u16, len: u16) -> Result<Vec<u8>, String> {
        let s = start as usize;
        let e = s + len as usize;
        if e > 0x10000 {
            return Err("Export range exceeds memory".into());
        }
        Ok(self.ram[s..e].to_vec())
    }

    pub fn reset(&mut self, clear: bool) {
        if clear {
            self.ram = [0; 0x10000];
        }
    }

    pub fn set_watchpoint(&mut self, addr: u16) {
        self.watchpoints.insert(addr);
    }

    pub fn clear_watchpoint(&mut self, addr: u16) {
        self.watchpoints.remove(&addr);
    }

    pub fn clear_all_watchpoints(&mut self) {
        self.watchpoints.clear();
    }

    pub fn get_watchpoints(&self) -> Vec<u16> {
        let mut addrs: Vec<u16> = self.watchpoints.iter().copied().collect();
        addrs.sort_unstable();
        addrs
    }

    pub fn clear_watchpoint_trigger(&mut self) {
        self.watchpoint_trigger = None;
    }

    pub fn take_watchpoint_trigger(&mut self) -> Option<u16> {
        self.watchpoint_trigger.take()
    }
}