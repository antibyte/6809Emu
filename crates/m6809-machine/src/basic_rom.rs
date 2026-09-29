//! Microsoft BASIC firmware images for CoCo 2, Dragon 32, and the ACIA SBC.
//!
//! ROM binaries live in `crates/m6809-machine/roms/` and are embedded at
//! compile time. They are copyrighted by Microsoft / Tandy / Dragon Data;
//! redistribute only where you have the right to do so.
//!
//! The CoCo 2 and Dragon 32 boards serve their ROMs themselves (map type 0
//! of the SAM, see `coco2.rs`): the images are never copied into RAM, so the
//! RAM underneath (`$8000-$FEFF` on the 64K CoCo) stays available for the
//! SAM's all-RAM mode. Only the Grant Searle SBC keeps its ROM in RAM.

use crate::MachineKind;

/// Color BASIC 1.2 — maps to `$A000–$BFFF` (8 KiB).
pub const COCO_COLOR_BASIC: &[u8] = include_bytes!("../roms/bas12.rom");
/// Extended Color BASIC 1.1 — maps to `$8000–$9FFF` (8 KiB).
pub const COCO_EXTENDED_BASIC: &[u8] = include_bytes!("../roms/extbas11.rom");
/// Dragon 32 Microsoft BASIC — maps to `$8000–$BFFF` (16 KiB).
pub const DRAGON32_BASIC: &[u8] = include_bytes!("../roms/d32.rom");
/// Grant Searle Microsoft Extended BASIC — maps to `$C000–$FFFF` (16 KiB).
/// Console I/O is a 6850 ACIA at `$A000` (Motorola register order).
pub const MSBASIC_ACIA: &[u8] = include_bytes!("../roms/exbasrom.bin");

pub const COCO_COLOR_BASIC_ADDR: u16 = 0xA000;
pub const COCO_EXTENDED_BASIC_ADDR: u16 = 0x8000;
pub const DRAGON_BASIC_ADDR: u16 = 0x8000;
pub const MSBASIC_ROM_ADDR: u16 = 0xC000;
pub const MSBASIC_ACIA_BASE: u16 = 0xA000;

/// CoCo 2 ROM byte for `$8000-$BFFF` (Extended BASIC, then Color BASIC).
/// Addresses outside that window wrap into it.
pub fn coco_rom_byte(addr: u16) -> u8 {
    let offset = (addr & 0x1FFF) as usize;
    if addr & 0x2000 == 0 {
        COCO_EXTENDED_BASIC[offset]
    } else {
        COCO_COLOR_BASIC[offset]
    }
}

/// Dragon 32 ROM byte for `$8000-$BFFF` (one 16K BASIC image).
/// Addresses outside that window wrap into it.
pub fn dragon_rom_byte(addr: u16) -> u8 {
    DRAGON32_BASIC[(addr & 0x3FFF) as usize]
}

/// Install firmware for the given machine and return the reset PC from the
/// ROM vector table (`$FFFE`). Falls back to `default_reset` if the vector is
/// empty. CoCo 2 / Dragon 32: the board maps the ROM, `ram` is left alone.
pub fn install_firmware(ram: &mut [u8; 0x10000], kind: MachineKind, default_reset: u16) -> u16 {
    match kind {
        MachineKind::Bare => default_reset,
        MachineKind::Coco2 => install_coco(ram, default_reset),
        MachineKind::Dragon32 => install_dragon(ram, default_reset),
        MachineKind::MsBasic => install_msbasic(ram, default_reset),
    }
}

fn install_coco(_ram: &mut [u8; 0x10000], default_reset: u16) -> u16 {
    assert_eq!(COCO_EXTENDED_BASIC.len(), 0x2000);
    assert_eq!(COCO_COLOR_BASIC.len(), 0x2000);
    // The CPU fetches its vectors from $FFE0-$FFFF, which the SAM maps to
    // the last 32 bytes of Color BASIC ($BFE0-$BFFF).
    rom_vector(COCO_COLOR_BASIC, default_reset)
}

fn install_dragon(_ram: &mut [u8; 0x10000], default_reset: u16) -> u16 {
    assert_eq!(DRAGON32_BASIC.len(), 0x4000);
    // Vectors come from $BFE0-$BFFF, the end of the 16K BASIC ROM.
    rom_vector(DRAGON32_BASIC, default_reset)
}

fn install_msbasic(ram: &mut [u8; 0x10000], default_reset: u16) -> u16 {
    assert_eq!(MSBASIC_ACIA.len(), 0x4000);
    write_rom(ram, MSBASIC_ROM_ADDR, MSBASIC_ACIA);
    // Image already contains `$FFF0–$FFFF` (reset `$DB46`).
    vector_reset(ram, default_reset)
}

fn write_rom(ram: &mut [u8; 0x10000], addr: u16, data: &[u8]) {
    let start = addr as usize;
    let end = start + data.len();
    assert!(end <= 0x10000, "ROM overflow at ${addr:04X}");
    ram[start..end].copy_from_slice(data);
}

/// Reset vector stored in the last two bytes of a ROM image.
fn rom_vector(rom: &[u8], default_reset: u16) -> u16 {
    let n = rom.len();
    let pc = u16::from_be_bytes([rom[n - 2], rom[n - 1]]);
    if pc == 0 {
        default_reset
    } else {
        pc
    }
}

fn vector_reset(ram: &[u8; 0x10000], default_reset: u16) -> u16 {
    let hi = ram[0xFFFE] as u16;
    let lo = ram[0xFFFF] as u16;
    let pc = (hi << 8) | lo;
    if pc == 0 {
        default_reset
    } else {
        pc
    }
}

/// Firmware status for the UI / debugger.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FirmwareInfo {
    pub kind: MachineKind,
    pub name: String,
    pub present: bool,
    pub reset_pc: u16,
    pub regions: Vec<FirmwareRegion>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FirmwareRegion {
    pub name: String,
    pub address: u16,
    pub size: u16,
}

pub fn firmware_info(kind: MachineKind) -> FirmwareInfo {
    match kind {
        MachineKind::Bare => FirmwareInfo {
            kind,
            name: "None".into(),
            present: false,
            reset_pc: 0x0100,
            regions: vec![],
        },
        MachineKind::Coco2 => FirmwareInfo {
            kind,
            name: "Microsoft Extended Color BASIC 1.1 + Color BASIC 1.2".into(),
            present: true,
            reset_pc: rom_vector(COCO_COLOR_BASIC, 0),
            regions: vec![
                FirmwareRegion {
                    name: "Extended Color BASIC 1.1".into(),
                    address: COCO_EXTENDED_BASIC_ADDR,
                    size: COCO_EXTENDED_BASIC.len() as u16,
                },
                FirmwareRegion {
                    name: "Color BASIC 1.2".into(),
                    address: COCO_COLOR_BASIC_ADDR,
                    size: COCO_COLOR_BASIC.len() as u16,
                },
            ],
        },
        MachineKind::Dragon32 => FirmwareInfo {
            kind,
            name: "Dragon 32 Microsoft BASIC".into(),
            present: true,
            reset_pc: rom_vector(DRAGON32_BASIC, 0),
            regions: vec![FirmwareRegion {
                name: "Dragon BASIC".into(),
                address: DRAGON_BASIC_ADDR,
                size: DRAGON32_BASIC.len() as u16,
            }],
        },
        MachineKind::MsBasic => FirmwareInfo {
            kind,
            name: "Microsoft Extended BASIC (ACIA)".into(),
            present: true,
            reset_pc: rom_vector(MSBASIC_ACIA, 0),
            regions: vec![FirmwareRegion {
                name: "ExBasROM $C000".into(),
                address: MSBASIC_ROM_ADDR,
                size: MSBASIC_ACIA.len() as u16,
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coco_roms_have_expected_sizes_and_reset() {
        assert_eq!(COCO_COLOR_BASIC.len(), 8192);
        assert_eq!(COCO_EXTENDED_BASIC.len(), 8192);
        let mut ram = [0u8; 0x10000];
        let pc = install_coco(&mut ram, 0xC000);
        assert_eq!(pc, 0xA027);
        assert!(ram.iter().all(|&b| b == 0), "ROM must not be copied into RAM");
        assert_eq!(coco_rom_byte(0xA000), COCO_COLOR_BASIC[0]);
        assert_eq!(coco_rom_byte(0x8000), COCO_EXTENDED_BASIC[0]);
        assert_eq!(coco_rom_byte(0xBFFE), 0xA0);
        assert_eq!(coco_rom_byte(0xBFFF), 0x27);
        assert_eq!(firmware_info(MachineKind::Coco2).reset_pc, 0xA027);
    }

    #[test]
    fn dragon_rom_reset_vector() {
        let mut ram = [0u8; 0x10000];
        let pc = install_dragon(&mut ram, 0xC000);
        assert_eq!(pc, 0xB3B4);
        assert!(ram.iter().all(|&b| b == 0), "ROM must not be copied into RAM");
        assert_eq!(dragon_rom_byte(0x8000), DRAGON32_BASIC[0]);
        assert_eq!(dragon_rom_byte(0xBFFE), 0xB3);
        assert_eq!(firmware_info(MachineKind::Dragon32).reset_pc, 0xB3B4);
    }

    #[test]
    fn msbasic_rom_reset_and_size() {
        assert_eq!(MSBASIC_ACIA.len(), 0x4000);
        let mut ram = [0u8; 0x10000];
        let pc = install_msbasic(&mut ram, 0xC000);
        assert_eq!(pc, 0xDB46);
        assert_eq!(ram[0xC000], 0xFF);
        assert_eq!(ram[0xDB00], 0x8D); // BSR KEYIN
        assert_eq!(ram[0xFFFE], 0xDB);
        assert_eq!(ram[0xFFFF], 0x46);
    }
}
