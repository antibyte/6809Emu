//! Motorola MC6883 SAM (Synchronous Address Multiplexer) control register.
//!
//! 16 flip-flops written through `$FFC0-$FFDF`: an even address clears a
//! bit, the following odd address sets it (the data written is ignored;
//! reads do not change the register).
//!
//! | bit | name | addresses     | meaning                                   |
//! |-----|------|---------------|-------------------------------------------|
//! | 0-2 | V0-2 | `$FFC0-$FFC5` | VDG addressing mode                       |
//! | 3-9 | F0-6 | `$FFC6-$FFD3` | display offset (video base = F × 512)     |
//! | 10  | P1   | `$FFD4/5`     | page #1 (64K, map type 0: $0000-$7FFF → upper 32K) |
//! | 11  | R0   | `$FFD6/7`     | MPU rate: address-dependent speed-up      |
//! | 12  | R1   | `$FFD8/9`     | MPU rate: fast (1.8 MHz everywhere)       |
//! | 13  | M0   | `$FFDA/B`     | memory size (00 4K, 01 16K, 10 64K, 11 static) |
//! | 14  | M1   | `$FFDC/D`     |                                           |
//! | 15  | TY   | `$FFDE/F`     | map type: 0 = RAM/ROM, 1 = all RAM        |
//!
//! Power-up and RESET clear the whole register (MAME and XRoar do the same);
//! Color BASIC / Dragon BASIC then program F (screen at `$0400`) and M.
//!
//! M1/M0 are stored and reported, but the RAM is not aliased according to
//! them: the boards always decode their fitted RAM in full (aliasing a
//! 4K/16K setting would only matter for software that relies on mirrors,
//! and BASIC sizes memory with its own read-back test).

use serde::{Deserialize, Serialize};

pub const SAM_BASE: u16 = 0xFFC0;

const BIT_P1: u16 = 1 << 10;
const BIT_R0: u16 = 1 << 11;
const BIT_R1: u16 = 1 << 12;
const BIT_M1: u16 = 1 << 14;
const BIT_TY: u16 = 1 << 15;

/// MPU rate selected by R1/R0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuRate {
    /// R1 R0 = 00: 0.89 MHz for every access.
    Slow,
    /// R1 R0 = 01: RAM and `$FF00-$FF1F` slow, ROM / other I/O / dead
    /// cycles fast ("POKE 65495,0").
    AddressDependent,
    /// R1 = 1: 1.8 MHz for every access (no video refresh on a CoCo 1/2).
    Fast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sam {
    bits: u16,
    #[serde(default = "sam_base")]
    base: u16,
}

fn sam_base() -> u16 {
    SAM_BASE
}

impl Default for Sam {
    fn default() -> Self {
        Self::new()
    }
}

impl Sam {
    /// Power-up state: every bit clear.
    pub fn new() -> Self {
        Self {
            bits: 0,
            base: SAM_BASE,
        }
    }

    /// A SAM holding `bits` (tests / debugger).
    #[cfg(test)]
    pub fn from_bits(bits: u16) -> Self {
        Self {
            bits,
            base: SAM_BASE,
        }
    }

    /// Hardware RESET clears the control register.
    pub fn reset(&mut self) {
        self.bits = 0;
    }

    pub fn is_mapped(&self, addr: u16) -> bool {
        (self.base..=self.base + 0x1F).contains(&addr)
    }

    /// CPU write to `$FFC0-$FFDF`: even address clears, odd address sets.
    pub fn write(&mut self, addr: u16) {
        if !self.is_mapped(addr) {
            return;
        }
        let mask = 1u16 << ((addr - self.base) / 2);
        if addr & 1 == 0 {
            self.bits &= !mask;
        } else {
            self.bits |= mask;
        }
    }

    /// The whole 16-bit register.
    #[allow(dead_code)]
    pub fn bits(&self) -> u16 {
        self.bits
    }

    /// V2..V0 (VDG addressing mode).
    pub fn v_mode_bits(&self) -> u8 {
        (self.bits & 0x07) as u8
    }

    /// F6..F0 (display offset in 512-byte units).
    pub fn f_bits(&self) -> u8 {
        ((self.bits >> 3) & 0x7F) as u8
    }

    /// P1: page #1.
    pub fn page1(&self) -> bool {
        self.bits & BIT_P1 != 0
    }

    /// R1R0 as a number 0-3.
    pub fn rate_bits(&self) -> u8 {
        ((self.bits >> 11) & 0x03) as u8
    }

    /// M1M0 as a number 0-3 (0 = 4K, 1 = 16K, 2 = 64K dynamic, 3 = 64K static).
    pub fn memory_size_bits(&self) -> u8 {
        ((self.bits >> 13) & 0x03) as u8
    }

    /// M1 set: 64K RAM mode (the only mode in which P1 reaches RAM A15).
    pub fn mode_64k(&self) -> bool {
        self.bits & BIT_M1 != 0
    }

    /// TY: map type 1 (all RAM).
    pub fn map_type_all_ram(&self) -> bool {
        self.bits & BIT_TY != 0
    }

    /// P1 remaps `$0000-$7FFF` to the upper 32K of RAM (64K mode, map type 0).
    pub fn page1_active(&self) -> bool {
        self.page1() && self.mode_64k() && !self.map_type_all_ram()
    }

    pub fn cpu_rate(&self) -> CpuRate {
        if self.bits & BIT_R1 != 0 {
            CpuRate::Fast
        } else if self.bits & BIT_R0 != 0 {
            CpuRate::AddressDependent
        } else {
            CpuRate::Slow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_up_state_is_all_zero() {
        let sam = Sam::default();
        assert_eq!(sam.bits(), 0);
        assert_eq!(sam.f_bits(), 0);
        assert_eq!(sam.v_mode_bits(), 0);
        assert_eq!(sam.cpu_rate(), CpuRate::Slow);
        assert!(!sam.map_type_all_ram());
    }

    #[test]
    fn odd_address_sets_bit() {
        let mut sam = Sam::default();
        sam.write(0xFFC9); // set F1 → $0400
        assert_eq!(sam.f_bits(), 2);
        sam.write(0xFFCB); // set F2 → $0C00
        assert_eq!(sam.f_bits(), 6);
        sam.write(0xFFC5); // V2
        assert_eq!(sam.v_mode_bits(), 4);
    }

    #[test]
    fn even_address_clears_bit() {
        let mut sam = Sam::from_bits(1 << 4);
        assert_eq!(sam.f_bits(), 2);
        sam.write(0xFFC8); // clear F1
        assert_eq!(sam.f_bits(), 0);
        sam.write(0xFFE0); // not a SAM address
        assert_eq!(sam.bits(), 0);
    }

    #[test]
    fn rate_type_page_and_size_bits() {
        let mut sam = Sam::default();
        sam.write(0xFFD7); // R0: POKE 65495
        assert_eq!(sam.cpu_rate(), CpuRate::AddressDependent);
        assert_eq!(sam.rate_bits(), 1);
        sam.write(0xFFD9); // R1: POKE 65497
        assert_eq!(sam.cpu_rate(), CpuRate::Fast);
        sam.write(0xFFD6);
        sam.write(0xFFD8);
        assert_eq!(sam.cpu_rate(), CpuRate::Slow);

        sam.write(0xFFDF);
        assert!(sam.map_type_all_ram());
        sam.write(0xFFDE);
        assert!(!sam.map_type_all_ram());

        sam.write(0xFFD5); // P1
        assert!(sam.page1());
        assert!(!sam.page1_active(), "P1 needs 64K mode");
        sam.write(0xFFDD); // M1
        assert_eq!(sam.memory_size_bits(), 2);
        assert!(sam.page1_active());
        sam.write(0xFFDF); // TY=1 disables P1 remapping
        assert!(!sam.page1_active());

        sam.reset();
        assert_eq!(sam.bits(), 0);
    }
}
