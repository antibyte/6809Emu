use crate::keyboard::KeyboardMatrix;
use crate::sam::Sam;
use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PiaRegs {
    data_a: u8,
    control_a: u8,
    data_b: u8,
    control_b: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coco2Machine {
    pia0: PiaRegs,
    pia1: PiaRegs,
    sam: Sam,
    keyboard: KeyboardMatrix,
}

impl Coco2Machine {
    pub fn new() -> Self {
        let mut machine = Self::default();
        machine.pia0.control_a = 0x04;
        machine.pia0.control_b = 0x05;
        machine
    }

    fn is_pia0(&self, addr: u16) -> bool {
        (0xFF00..=0xFF03).contains(&addr)
    }

    fn is_pia1(&self, addr: u16) -> bool {
        (0xFF20..=0xFF23).contains(&addr)
    }

    fn is_rom(&self, addr: u16) -> bool {
        (0xC000..=0xFEFF).contains(&addr)
    }

    fn read_pia0(&self, offset: u8) -> u8 {
        match offset {
            0 => self.pia0.data_a,
            1 => self.pia0.control_a,
            2 => self.keyboard.read_columns(),
            3 => self.pia0.control_b,
            _ => 0xFF,
        }
    }

    fn write_pia0(&mut self, offset: u8, value: u8) {
        match offset {
            0 => {
                self.pia0.data_a = value;
                self.keyboard.select_row(!value);
            }
            1 => self.pia0.control_a = value,
            2 => self.pia0.data_b = value,
            3 => self.pia0.control_b = value,
            _ => {}
        }
    }

    fn read_pia1(pia: &PiaRegs, offset: u8) -> u8 {
        match offset {
            0 => pia.data_a,
            1 => pia.control_a,
            2 => pia.data_b,
            3 => pia.control_b,
            _ => 0xFF,
        }
    }

    fn write_pia1(pia: &mut PiaRegs, offset: u8, value: u8) {
        match offset {
            0 => pia.data_a = value,
            1 => pia.control_a = value,
            2 => pia.data_b = value,
            3 => pia.control_b = value,
            _ => {}
        }
    }
}

impl MemoryIo for Coco2Machine {
    fn kind_id(&self) -> &str {
        "coco2"
    }

    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        if self.is_pia0(addr) {
            return Some(self.read_pia0((addr & 3) as u8));
        }
        if self.is_pia1(addr) {
            return Some(Self::read_pia1(&self.pia1, (addr & 3) as u8));
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
        if self.is_pia0(addr) {
            self.write_pia0((addr & 3) as u8, value);
            return IoWriteResult::Consumed;
        }
        if self.is_pia1(addr) {
            Self::write_pia1(&mut self.pia1, (addr & 3) as u8, value);
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
                name: "PIA0 Data A (Kbd Row)".into(),
                value: self.pia0.data_a,
            },
            IoRegisterView {
                address: 0xFF01,
                name: "PIA0 Control A".into(),
                value: self.pia0.control_a,
            },
            IoRegisterView {
                address: 0xFF02,
                name: "PIA0 Data B (Kbd Col)".into(),
                value: self.keyboard.read_columns(),
            },
            IoRegisterView {
                address: 0xFF03,
                name: "PIA0 Control B".into(),
                value: self.pia0.control_b,
            },
            IoRegisterView {
                address: 0xFF20,
                name: "PIA1 Data A".into(),
                value: self.pia1.data_a,
            },
            IoRegisterView {
                address: 0xFF21,
                name: "PIA1 Control A".into(),
                value: self.pia1.control_a,
            },
            IoRegisterView {
                address: 0xFFC0,
                name: "SAM V0".into(),
                value: (self.sam.bits() & 0x01) as u8,
            },
            IoRegisterView {
                address: 0xFF22,
                name: "PIA1 VDG Mode".into(),
                value: self.pia1.data_b,
            },
        ]
    }
}