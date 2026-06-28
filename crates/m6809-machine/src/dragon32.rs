use crate::keyboard::KeyboardMatrix;
use crate::sam::Sam;
use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};
use serde::{Deserialize, Serialize};

fn default_dragon_vdg_mode() -> u8 {
    0x04
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PiaRegs {
    data_a: u8,
    data_b: u8,
    control_a: u8,
    control_b: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dragon32Machine {
    pia: PiaRegs,
    /// VDG mode latch (PIA port B bits 7–4); separate from column readback.
    #[serde(default = "default_dragon_vdg_mode")]
    vdg_mode: u8,
    sam: Sam,
    keyboard: KeyboardMatrix,
}

impl Default for Dragon32Machine {
    fn default() -> Self {
        Self {
            pia: PiaRegs::default(),
            vdg_mode: 0x04,
            sam: Sam::with_dragon_default(),
            keyboard: KeyboardMatrix::default(),
        }
    }
}

impl Dragon32Machine {
    pub fn new() -> Self {
        let mut machine = Self::default();
        machine.pia.control_a = 0x04;
        machine.pia.control_b = 0x05;
        machine
    }

    fn is_pia(&self, addr: u16) -> bool {
        (0xFF00..=0xFF04).contains(&addr)
    }

    fn is_rom(&self, addr: u16) -> bool {
        (0xC000..=0xFEFF).contains(&addr)
    }

    fn read_pia(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.pia.data_a,
            0xFF01 => self.keyboard.read_columns(),
            0xFF02 => self.pia.control_a,
            0xFF03 => self.pia.control_b,
            0xFF04 => 0x00,
            _ => 0xFF,
        }
    }

    fn write_pia(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF00 => {
                self.pia.data_a = value;
                self.keyboard.select_row(!value);
            }
            0xFF01 => {
                self.pia.data_b = value;
                self.vdg_mode = (self.vdg_mode & 0x0F) | (value & 0xF0);
            }
            0xFF02 => self.pia.control_a = value,
            0xFF03 => self.pia.control_b = value,
            _ => {}
        }
    }
}

impl MemoryIo for Dragon32Machine {
    fn kind_id(&self) -> &str {
        "dragon32"
    }

    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        if self.is_pia(addr) {
            return Some(self.read_pia(addr));
        }
        if self.sam.is_mapped(addr) {
            return Some(self.sam.read(addr));
        }
        if self.is_rom(addr) {
            return Some(ram[addr as usize]);
        }
        None
    }

    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        if self.is_pia(addr) {
            self.write_pia(addr, value);
            return IoWriteResult::Consumed;
        }
        if self.sam.is_mapped(addr) {
            let _ = value;
            self.sam.write(addr);
            return IoWriteResult::Consumed;
        }
        if self.is_rom(addr) {
            return IoWriteResult::Ignored;
        }
        let _ = ram;
        IoWriteResult::PassThrough
    }

    fn clone_box(&self) -> Box<dyn MemoryIo> {
        Box::new(self.clone())
    }

    fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    fn restore(&mut self, snapshot: &serde_json::Value) {
        if let Ok(state) = serde_json::from_value(snapshot.clone()) {
            *self = state;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn io_registers(&self) -> Vec<IoRegisterView> {
        vec![
            IoRegisterView {
                address: 0xFF00,
                name: "PIA Data A (Kbd Row)".into(),
                value: self.pia.data_a,
            },
            IoRegisterView {
                address: 0xFF01,
                name: "PIA Data B (Kbd Col)".into(),
                value: self.keyboard.read_columns(),
            },
            IoRegisterView {
                address: 0xFF02,
                name: "PIA Control A".into(),
                value: self.pia.control_a,
            },
            IoRegisterView {
                address: 0xFF03,
                name: "PIA Control B".into(),
                value: self.pia.control_b,
            },
            IoRegisterView {
                address: 0xFF60,
                name: "SAM V0".into(),
                value: (self.sam.bits() & 0x01) as u8,
            },
            IoRegisterView {
                address: 0xFF62,
                name: "SAM V1/V2".into(),
                value: ((self.sam.bits() >> 1) & 0x03) as u8,
            },
        ]
    }
}