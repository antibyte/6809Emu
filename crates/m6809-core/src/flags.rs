use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Flags: u8 {
        const C = 0x01;
        const V = 0x02;
        const Z = 0x04;
        const N = 0x08;
        const I = 0x10;
        const H = 0x20;
        const F = 0x40;
        const E = 0x80;
    }
}

impl Flags {
    pub fn from_byte(value: u8) -> Self {
        Self::from_bits_retain(value)
    }

    pub fn set_nz8(&mut self, value: u8) {
        self.set(Flags::N, value & 0x80 != 0);
        self.set(Flags::Z, value == 0);
    }

    pub fn set_nz16(&mut self, value: u16) {
        self.set(Flags::N, value & 0x8000 != 0);
        self.set(Flags::Z, value == 0);
    }

    pub fn set_nz32(&mut self, value: u32) {
        self.set(Flags::N, value & 0x8000_0000 != 0);
        self.set(Flags::Z, value == 0);
    }
}