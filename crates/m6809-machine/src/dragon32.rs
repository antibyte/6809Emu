//! Dragon 32 board: the CoCo board core (`coco2.rs`) with Dragon wiring.
//!
//! Differences from the CoCo 2:
//! - PAL timing: 14.218 MHz crystal (E = 888,625 Hz), 312 lines per field
//!   (17,784 E cycles, 49.97 Hz); FS is low for 57 lines.
//! - Keyboard rows 0-5 are wired rotated (`dragon_row = (coco_row + 2) % 6`).
//! - One 16K Microsoft BASIC ROM at `$8000-$BFFF` (vectors from `$BFE0`).
//! - 32K of RAM built from 32K × 1 chips (4532, the good half of a 4164):
//!   PIA1 PB2 is tied low, so BASIC selects the SAM's 64K mode (M1), but the
//!   chips do not decode A15. Map type 1 (`TY`) therefore makes
//!   `$8000-$FEFF` a mirror of `$0000-$7EFF`, and P1 has no visible effect
//!   (as XRoar's 32K × 1 RAM organisation).
//! - PIA1 PA1 is the printer strobe, PB0 printer BUSY, CA1 printer ACK
//!   (handled by the peripherals); CB1 is CART* as on the CoCo.

use std::cell::RefCell;

use crate::basic_rom;
use crate::board_pia::BoardPia;
use crate::coco2::{board_machine_impl, BoardCore, BoardSpec};
use crate::keyboard::KeyboardLayout;
use crate::peripherals::BoardKind;
use serde::{Deserialize, Serialize};

/// PAL E clock: 14.218 MHz / 16.
pub const CPU_CLOCK_HZ: u32 = 888_625;

/// Dragon 32: 32K RAM (32K × 1), PAL.
#[derive(Debug, Clone, Default)]
pub(crate) struct DragonSpec;

impl BoardSpec for DragonSpec {
    const KIND_ID: &'static str = "dragon32";
    const PERIPHERALS: BoardKind = BoardKind::Dragon32;
    const LAYOUT: KeyboardLayout = KeyboardLayout::Dragon;
    const BASE_CLOCK_HZ: u32 = CPU_CLOCK_HZ;
    const LINES_PER_FIELD: u32 = 312;
    const FS_HIGH_LINES: u32 = 255;
    const RAM_MASK: u16 = 0x7FFF;

    fn rom_byte(addr: u16) -> u8 {
        basic_rom::dragon_rom_byte(addr)
    }

    /// PB2 is tied low on the Dragon 32 (32K × 1 RAM).
    fn ram_size_sense(_pia0: &BoardPia) -> bool {
        false
    }
}

/// Dragon Data Dragon 32 (32K, PAL, Microsoft BASIC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dragon32Machine {
    inner: RefCell<BoardCore<DragonSpec>>,
}

board_machine_impl!(Dragon32Machine, DragonSpec);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coco2::test_support::*;
    use crate::vdg::VdgInputs;
    use m6809_core::{IoWriteResult, MemoryIo};

    type M = Dragon32Machine;

    impl TestBoard for Dragon32Machine {
        const FIELD_CYCLES: u64 = 312 * 57;

        fn fresh() -> Self {
            Dragon32Machine::new()
        }

        fn key(&mut self, code: &str, key: Option<&str>, down: bool) {
            self.host_key_event(code, key, down);
        }

        fn video(&self) -> VdgInputs {
            self.vdg_inputs()
        }

        fn fields(&self) -> u64 {
            self.inner.borrow().field_count()
        }

        fn lines(&self) -> u64 {
            self.inner.borrow().line_count()
        }

        fn sam_bits(&self) -> u16 {
            self.inner.borrow().sam().bits()
        }
    }

    #[test]
    fn rom_vectors_and_io_map() {
        let mut m = Dragon32Machine::new();
        let mut ram = [0u8; 0x10000];
        assert_eq!(m.kind_id(), "dragon32");
        assert_eq!(m.read(0xFFFE, &ram), Some(0xB3));
        assert_eq!(m.read(0xFFFF, &ram), Some(0xB4));
        assert_eq!(m.read(0x8000, &ram), Some(basic_rom::DRAGON32_BASIC[0]));
        assert_eq!(m.write(0x8000, 0, &mut ram), IoWriteResult::Ignored);
        assert_eq!(m.write(0xFFFE, 0, &mut ram), IoWriteResult::Ignored);
        assert_eq!(m.write(0xFF60, 1, &mut ram), IoWriteResult::Ignored);
        assert_eq!(m.read(0xFF60, &ram), Some(0xFF));
        assert_eq!(m.read(0x7FFF, &ram), None);
        assert_eq!(Dragon32Machine::cpu_clock_hz(&m), 888_625);
    }

    #[test]
    fn map_type_1_mirrors_the_32k_of_ram() {
        let mut emu = emu_with::<M>();
        emu.memory.write8(0x0123, 0x42);
        emu.memory.write8(0xFFDF, 0); // TY=1
        assert_eq!(emu.memory.read8(0x8123), 0x42, "A15 is not decoded");
        emu.memory.write8(0x8124, 0x43);
        assert_eq!(emu.memory.ram[0x0124], 0x43);
        assert_eq!(emu.memory.ram[0x8124], 0x00);
        emu.memory.write8(0xFFDE, 0);
        assert_eq!(emu.memory.read8(0x8123), basic_rom::DRAGON32_BASIC[0x123]);
        // P1 in 64K mode: page 1 is page 0 again.
        emu.memory.write8(0xFFDD, 0);
        emu.memory.write8(0xFFD5, 0);
        assert_eq!(emu.memory.read8(0x0123), 0x42);
    }

    #[test]
    fn pal_field_and_line_rates() {
        let mut emu = emu_with::<M>();
        idle_loop(&mut emu);
        let (l0, f0) = (board::<M>(&emu).lines(), board::<M>(&emu).fields());
        run_cycles(&mut emu, 888_625);
        let lines = board::<M>(&emu).lines() - l0;
        let fields = board::<M>(&emu).fields() - f0;
        assert!((15_500..=15_700).contains(&lines), "HS per second {lines}");
        assert!((49..=51).contains(&fields), "FS per second {fields}");
        assert_eq!(<DragonSpec as BoardSpec>::field_ticks(), 17_784 * 2);
    }

    #[test]
    fn fs_irq_handler_runs_once_per_field_50_hz() {
        let mut emu = emu_with::<M>();
        load_irq_counter(&mut emu, true);
        run_cycles(&mut emu, 2_000);
        let f0 = board::<M>(&emu).fields();
        let c0 = emu.memory.read16(0x0300);
        run_cycles(&mut emu, 888_625);
        let count = emu.memory.read16(0x0300) - c0;
        let fields = board::<M>(&emu).fields() - f0;
        assert!((49..=51).contains(&count), "IRQs per second {count}");
        assert!(u64::from(count).abs_diff(fields) <= 1, "one IRQ per field ({count} vs {fields})");
    }

    #[test]
    fn dragon_basic_boots_with_32k() {
        let emu = boot_basic::<M>();
        let screen = screen_text::<M>(&emu);
        assert!(screen.contains("OK"), "{screen}");
        assert_eq!(emu.memory.read16(0x0074), 0x7FFE, "top of RAM");
        let sam = crate::sam::Sam::from_bits(board::<M>(&emu).sam_bits());
        assert_eq!(sam.f_bits(), 2, "text screen at $0400");
    }

    #[test]
    fn dragon_timer_counts_50_per_second() {
        let mut emu = boot_basic::<M>();
        let t0 = emu.memory.read16(0x0112);
        run_cycles(&mut emu, 888_625);
        let dt = emu.memory.read16(0x0112).wrapping_sub(t0);
        assert!((49..=51).contains(&dt), "TIMER advanced by {dt}");
    }

    #[test]
    fn dragon_matrix_typing() {
        let mut emu = boot_basic::<M>();
        // Positional codes, no typed characters: the Dragon rows must be used.
        for code in ["KeyA", "KeyB", "KeyP", "Digit1", "Digit9", "KeyX", "Digit0", "Slash"] {
            tap::<M>(&mut emu, code, None);
        }
        let line = screen_line_with::<M>(&emu, "ABP");
        assert!(line.starts_with("ABP19X0/"), "screen line {line:?}");
    }

    #[test]
    fn dragon_character_mapping() {
        let mut emu = boot_basic::<M>();
        let text = "AZ09!\"#$%&'()*+,-./:;<=>?@^[]\\_";
        type_text::<M>(&mut emu, text);
        let screen = screen_text::<M>(&emu);
        let line = screen_line_with::<M>(&emu, "AZ09");
        assert!(line.starts_with(text), "screen line {line:?}\n{screen}");
    }

    #[test]
    fn vdg_inputs_follow_pia1_port_b() {
        let mut m = Dragon32Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF22, 0xF8, &mut ram); // DDRB (CRB bit 2 = 0 after reset)
        m.write(0xFF23, 0x04, &mut ram);
        m.write(0xFF22, 0x80, &mut ram);
        assert_eq!(m.vdg_inputs().vdg_ctrl, 0x80);
        m.write(0xFF22, 0x08, &mut ram);
        assert_eq!(m.vdg_inputs().vdg_ctrl, 0x08);
    }

    #[test]
    fn io_registers_include_both_pias_and_the_sam() {
        let mut m = Dragon32Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF22, 0xF8, &mut ram);
        m.write(0xFF23, 0x04, &mut ram);
        m.write(0xFF22, 0x58, &mut ram);
        m.write(0xFFC9, 0, &mut ram);
        let regs = m.io_registers();
        let find = |a: u16| regs.iter().find(|r| r.address == a).expect("reg").value;
        assert_eq!(find(0xFF22), 0x58);
        assert_eq!(find(0xFF23), 0x04);
        assert_eq!(find(0xFF20), 0xFF);
        assert_eq!(find(0xFFC6), 2);
    }
}
