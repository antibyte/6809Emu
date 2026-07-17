//! Microsoft BASIC firmware images for CoCo 2 and Dragon 32.
//!
//! ROM binaries live in `crates/m6809-machine/roms/` and are embedded at
//! compile time. They are copyrighted by Microsoft / Tandy / Dragon Data;
//! redistribute only where you have the right to do so.

use crate::MachineKind;

/// Color BASIC 1.2 — maps to `$A000–$BFFF` (8 KiB).
pub const COCO_COLOR_BASIC: &[u8] = include_bytes!("../roms/bas12.rom");
/// Extended Color BASIC 1.1 — maps to `$8000–$9FFF` (8 KiB).
pub const COCO_EXTENDED_BASIC: &[u8] = include_bytes!("../roms/extbas11.rom");
/// Dragon 32 Microsoft BASIC — maps to `$8000–$BFFF` (16 KiB).
pub const DRAGON32_BASIC: &[u8] = include_bytes!("../roms/d32.rom");

pub const COCO_COLOR_BASIC_ADDR: u16 = 0xA000;
pub const COCO_EXTENDED_BASIC_ADDR: u16 = 0x8000;
pub const DRAGON_BASIC_ADDR: u16 = 0x8000;

/// Install firmware for the given machine and return the reset PC from the
/// ROM vector table (`$FFFE`). Falls back to `default_reset` if vectors are empty.
pub fn install_firmware(ram: &mut [u8; 0x10000], kind: MachineKind, default_reset: u16) -> u16 {
    match kind {
        MachineKind::Bare => default_reset,
        MachineKind::Coco2 => install_coco(ram, default_reset),
        MachineKind::Dragon32 => install_dragon(ram, default_reset),
    }
}

fn install_coco(ram: &mut [u8; 0x10000], default_reset: u16) -> u16 {
    assert_eq!(COCO_EXTENDED_BASIC.len(), 0x2000);
    assert_eq!(COCO_COLOR_BASIC.len(), 0x2000);

    write_rom(ram, COCO_EXTENDED_BASIC_ADDR, COCO_EXTENDED_BASIC);
    write_rom(ram, COCO_COLOR_BASIC_ADDR, COCO_COLOR_BASIC);

    // CoCo mirrors the last 16 bytes of Color BASIC into the vector page.
    mirror_vectors(ram, COCO_COLOR_BASIC);
    vector_reset(ram, default_reset)
}

fn install_dragon(ram: &mut [u8; 0x10000], default_reset: u16) -> u16 {
    assert_eq!(DRAGON32_BASIC.len(), 0x4000);
    write_rom(ram, DRAGON_BASIC_ADDR, DRAGON32_BASIC);
    // Dragon mirrors last 16 bytes of BASIC ROM to `$FFF0`.
    mirror_vectors(ram, DRAGON32_BASIC);
    vector_reset(ram, default_reset)
}

fn write_rom(ram: &mut [u8; 0x10000], addr: u16, data: &[u8]) {
    let start = addr as usize;
    let end = start + data.len();
    assert!(end <= 0x10000, "ROM overflow at ${addr:04X}");
    ram[start..end].copy_from_slice(data);
}

fn mirror_vectors(ram: &mut [u8; 0x10000], rom: &[u8]) {
    let n = rom.len();
    assert!(n >= 16);
    ram[0xFFF0..0x10000].copy_from_slice(&rom[n - 16..]);
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
            reset_pc: {
                let hi = COCO_COLOR_BASIC[0x1FFE] as u16;
                let lo = COCO_COLOR_BASIC[0x1FFF] as u16;
                (hi << 8) | lo
            },
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
            reset_pc: {
                let n = DRAGON32_BASIC.len();
                let hi = DRAGON32_BASIC[n - 2] as u16;
                let lo = DRAGON32_BASIC[n - 1] as u16;
                (hi << 8) | lo
            },
            regions: vec![FirmwareRegion {
                name: "Dragon BASIC".into(),
                address: DRAGON_BASIC_ADDR,
                size: DRAGON32_BASIC.len() as u16,
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
        assert_eq!(ram[0xA000], COCO_COLOR_BASIC[0]);
        assert_eq!(ram[0x8000], COCO_EXTENDED_BASIC[0]);
        assert_eq!(ram[0xFFFE], 0xA0);
        assert_eq!(ram[0xFFFF], 0x27);
    }

    #[test]
    fn dragon_rom_reset_vector() {
        let mut ram = [0u8; 0x10000];
        let pc = install_dragon(&mut ram, 0xC000);
        assert_eq!(pc, 0xB3B4);
        assert_eq!(&ram[0x8000..0x8004], &DRAGON32_BASIC[0..4]);
    }
}
