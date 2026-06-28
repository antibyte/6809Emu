use serde::{Deserialize, Serialize};

/// SAM 16-bit flip-flop state ($FFC0–$FFDF).
/// Even address clears a bit, odd address sets it (CoCo/Dragon).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sam {
    bits: u16,
    #[serde(default = "sam_base_coco")]
    base: u16,
}

fn sam_base_coco() -> u16 {
    0xFFC0
}

impl Default for Sam {
    fn default() -> Self {
        Self::with_coco_default()
    }
}

impl Sam {
    /// Screen at $0400: screen-address bit 1 set (bit index 4).
    pub fn with_coco_default() -> Self {
        Self {
            bits: 1 << 4,
            base: 0xFFC0,
        }
    }

    /// Dragon text screen defaults to $0C00.
    pub fn with_dragon_default() -> Self {
        Self {
            bits: 0x30,
            base: 0xFF60,
        }
    }

    pub fn is_mapped(&self, addr: u16) -> bool {
        (self.base..=self.base + 0x1F).contains(&addr)
    }

    pub fn write(&mut self, addr: u16) {
        if !self.is_mapped(addr) {
            return;
        }
        let bit = ((addr - self.base) / 2) as u8;
        if bit >= 16 {
            return;
        }
        let mask = 1u16 << bit;
        if addr & 1 == 0 {
            self.bits &= !mask;
        } else {
            self.bits |= mask;
        }
    }

    pub fn read(&self, _addr: u16) -> u8 {
        0xFF
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }

    pub fn base_addr(&self) -> u16 {
        self.base
    }

    #[allow(dead_code)]
    pub fn from_bits(bits: u16) -> Self {
        Self {
            bits,
            base: 0xFFC0,
        }
    }

    pub fn with_bits_and_base(bits: u16, base: u16) -> Self {
        Self { bits, base }
    }

    /// Video RAM base: seven screen-address bits (SAM $FFC6–$FFD3) → %DDDDDDD0_00000000.
    pub fn video_base(&self) -> u16 {
        let addr_bits = (self.bits >> 3) & 0x7F;
        addr_bits << 9
    }

    pub fn v_mode_bits(&self) -> u8 {
        (self.bits & 0x07) as u8
    }

    #[allow(dead_code)]
    pub fn io_registers(&self) -> [(u16, &'static str, u8); 2] {
        [
            (0xFFC0, "SAM V0", (self.bits & 0x01) as u8),
            (0xFFC2, "SAM V1/V2", ((self.bits >> 1) & 0x03) as u8),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_maps_screen_to_0400() {
        let sam = Sam::default();
        assert_eq!(sam.video_base(), 0x0400);
    }

    #[test]
    fn odd_address_sets_bit() {
        let mut sam = Sam::default();
        sam.write(0xFFCB); // set screen addr bit 2 → base $0C00
        assert_eq!(sam.video_base(), 0x0C00);
    }

    #[test]
    fn even_address_clears_bit() {
        let mut sam = Sam::default();
        sam.write(0xFFC8); // clear screen addr bit 1 → base $0000
        assert_eq!(sam.video_base(), 0x0000);
    }
}