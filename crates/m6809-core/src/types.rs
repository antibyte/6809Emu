use serde::{Deserialize, Serialize};

use crate::flags::Flags;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CpuVariant {
    #[default]
    Mc6809,
    Hd6309,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trap {
    Breakpoint,
    Watchpoint,
    Halted,
    IllegalOpcode,
    Swi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub cycles: u32,
    pub pc_before: u16,
    pub pc_after: u16,
    pub opcode: u8,
    pub bytes: Vec<u8>,
    pub mnemonic: String,
    pub operands: String,
    pub trap: Option<Trap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuState {
    pub a: u8,
    pub b: u8,
    pub d: u16,
    pub x: u16,
    pub y: u16,
    pub u: u16,
    pub s: u16,
    pub pc: u16,
    pub dp: u8,
    pub cc: u8,
    pub flags: FlagState,
    pub total_cycles: u64,
    pub halted: bool,
    pub irq_pending: bool,
    pub firq_pending: bool,
    pub nmi_pending: bool,
    #[serde(default)]
    pub lds_encountered: bool,
    #[serde(default)]
    pub variant: CpuVariant,
    #[serde(default)]
    pub w: u16,
    #[serde(default)]
    pub v: u16,
    #[serde(default)]
    pub mode_reg: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagState {
    pub c: bool,
    pub v: bool,
    pub z: bool,
    pub n: bool,
    pub i: bool,
    pub h: bool,
    pub f: bool,
    pub e: bool,
}

impl From<Flags> for FlagState {
    fn from(flags: Flags) -> Self {
        Self {
            c: flags.contains(Flags::C),
            v: flags.contains(Flags::V),
            z: flags.contains(Flags::Z),
            n: flags.contains(Flags::N),
            i: flags.contains(Flags::I),
            h: flags.contains(Flags::H),
            f: flags.contains(Flags::F),
            e: flags.contains(Flags::E),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadConfig {
    pub load_addr: u16,
    pub reset_pc: u16,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            load_addr: 0x0100,
            reset_pc: 0x0100,
        }
    }
}