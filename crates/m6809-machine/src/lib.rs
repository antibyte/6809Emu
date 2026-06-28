mod coco2;
mod coco2_rom;
mod dragon32;
mod dragon_rom;
mod keyboard;
mod machine_io;
mod mc6850;
mod sam;
mod vdg;
mod video;

use m6809_core::{Emulator, IoRegisterView, LoadConfig, MemoryIo};
use serde::{Deserialize, Serialize};

pub use coco2::Coco2Machine;
pub use dragon32::Dragon32Machine;
pub use machine_io::MachineContainer;
pub use mc6850::{AciaConfig, AciaTerminalDto, Acia6850};
pub use video::VideoFrameDto;

pub fn machine_video_frame(emu: &Emulator) -> Option<VideoFrameDto> {
    video::video_frame(emu)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineKind {
    Bare,
    Coco2,
    Dragon32,
}

impl MachineKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Bare => "bare",
            Self::Coco2 => "coco2",
            Self::Dragon32 => "dragon32",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "bare" => Some(Self::Bare),
            "coco2" => Some(Self::Coco2),
            "dragon32" => Some(Self::Dragon32),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineInfo {
    pub kind: MachineKind,
    pub name: String,
    pub load_addr: u16,
    pub reset_pc: u16,
    pub description: String,
}

pub fn list_machines() -> Vec<MachineInfo> {
    vec![
        MachineInfo {
            kind: MachineKind::Bare,
            name: "Bare Metal".into(),
            load_addr: 0x0100,
            reset_pc: 0x0100,
            description: "Flat 64 KB RAM, no I/O mapping.".into(),
        },
        MachineInfo {
            kind: MachineKind::Coco2,
            name: "TRS-80 CoCo 2".into(),
            load_addr: 0x0100,
            reset_pc: 0xC000,
            description: "CoCo 2 with PIA/SAM/VDG and boot ROM (hardware init + banner).".into(),
        },
        MachineInfo {
            kind: MachineKind::Dragon32,
            name: "Dragon 32".into(),
            load_addr: 0x0100,
            reset_pc: 0xC000,
            description: "Dragon 32 with PIA/SAM and boot ROM (hardware init + banner).".into(),
        },
    ]
}

pub fn create_io(kind: MachineKind) -> Option<Box<dyn MemoryIo>> {
    Some(Box::new(MachineContainer::new(kind)))
}

pub fn machine_container(emu: &m6809_core::Emulator) -> Option<&MachineContainer> {
    emu.memory
        .io
        .as_ref()?
        .as_any()
        .downcast_ref::<MachineContainer>()
}

pub fn machine_container_mut(emu: &mut m6809_core::Emulator) -> Option<&mut MachineContainer> {
    emu.memory
        .io
        .as_mut()?
        .as_any_mut()
        .downcast_mut::<MachineContainer>()
}

pub fn get_acia_config(emu: &m6809_core::Emulator) -> AciaConfig {
    machine_container(emu)
        .map(|c| c.acia_config())
        .unwrap_or_default()
}

pub fn set_acia_config(emu: &mut m6809_core::Emulator, config: AciaConfig) {
    if let Some(c) = machine_container_mut(emu) {
        c.set_acia_config(config);
    }
}

pub fn get_acia_terminal(emu: &m6809_core::Emulator) -> AciaTerminalDto {
    machine_container(emu)
        .map(|c| c.acia_terminal())
        .unwrap_or(AciaTerminalDto {
            tx_text: String::new(),
            rdrf: false,
            tdre: true,
            irq: false,
        })
}

pub fn acia_send_input(emu: &m6809_core::Emulator, text: &str) {
    if let Some(c) = machine_container(emu) {
        c.acia_send_input(text);
    }
}

pub fn restore_io(kind_id: &str, state: &serde_json::Value) -> Option<Box<dyn MemoryIo>> {
    let kind = MachineKind::from_id(kind_id)?;
    let mut io = create_io(kind)?;
    io.restore(state);
    Some(io)
}

pub fn current_kind(emu: &Emulator) -> MachineKind {
    emu.memory
        .io
        .as_ref()
        .and_then(|io| MachineKind::from_id(io.kind_id()))
        .unwrap_or(MachineKind::Bare)
}

pub fn apply_machine(emu: &mut Emulator, kind: MachineKind) -> LoadConfig {
    let info = list_machines()
        .into_iter()
        .find(|m| m.kind == kind)
        .expect("machine info");

    emu.memory.io = create_io(kind);
    emu.memory.reset(true);
    install_boot_rom(&mut emu.memory.ram, kind, info.load_addr);
    install_trap_vector(&mut emu.memory, kind);

    let config = LoadConfig {
        load_addr: info.load_addr,
        reset_pc: info.reset_pc,
    };
    emu.memory.config = config.clone();
    emu.memory.write16(0xFFFE, info.reset_pc);
    emu.cpu.breakpoints.clear();
    emu.memory.clear_all_watchpoints();
    emu.reset();
    config
}

fn install_boot_rom(ram: &mut [u8; 0x10000], kind: MachineKind, entry: u16) {
    match kind {
        MachineKind::Bare => {}
        MachineKind::Coco2 => coco2_rom::install(ram, entry),
        MachineKind::Dragon32 => dragon_rom::install(ram, entry),
    }
}

fn install_trap_vector(mem: &mut m6809_core::Memory, kind: MachineKind) {
    if kind == MachineKind::Bare {
        return;
    }
    mem.write16(0xFFF0, coco2_rom::TRAP_HANDLER);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineStateDto {
    pub kind: MachineKind,
    pub io_registers: Vec<IoRegisterView>,
    pub acia: AciaConfig,
}

pub fn machine_state(emu: &Emulator) -> MachineStateDto {
    let kind = current_kind(emu);
    let io_registers = emu
        .memory
        .io
        .as_ref()
        .map(|io| io.io_registers())
        .unwrap_or_default();
    let acia = get_acia_config(emu);
    MachineStateDto {
        kind,
        io_registers,
        acia,
    }
}

pub fn restore_machine_io(emu: &mut Emulator, kind_id: &str, state: &serde_json::Value) {
    emu.memory.io = restore_io(kind_id, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coco2_io_read_pia() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        let v = emu.memory.read8(0xFF00);
        assert_eq!(v, 0x00);
    }

    #[test]
    fn coco2_rom_resets_to_c000() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        assert_eq!(emu.memory.read8(0xC000), 0x1A); // ORCC
        assert_eq!(emu.memory.read16(0xFFFE), 0xC000);
        assert_eq!(emu.memory.read16(0xFFF0), coco2_rom::TRAP_HANDLER);
    }

    #[test]
    fn coco2_keyboard_defaults_no_keys() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        emu.memory.write8(0xFF00, 0xFE);
        assert_eq!(emu.memory.read8(0xFF02), 0xFF);
    }

    #[test]
    fn coco2_boot_runs_to_load_addr() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        let _ = emu.memory.load_binary(0x0100, &[0x12, 0x12, 0x12, 0x12]);
        for _ in 0..8000 {
            if emu.cpu.pc == 0x0100 {
                break;
            }
            emu.step();
        }
        assert_eq!(emu.cpu.pc, 0x0100);
        for _ in 0..4 {
            emu.step();
        }
        assert_eq!(emu.cpu.pc, 0x0104);
    }

    #[test]
    fn coco2_trap_vector_installed() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        assert_eq!(emu.memory.read16(0xFFF0), coco2_rom::TRAP_HANDLER);
        assert_eq!(emu.memory.read8(coco2_rom::TRAP_HANDLER), 0x12);
    }

    #[test]
    fn coco2_boot_banner_on_screen() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        for _ in 0..8000 {
            if emu.cpu.pc == 0x0100 {
                break;
            }
            emu.step();
        }
        let frame = machine_video_frame(&emu).expect("frame");
        assert!(frame.rows_text[0].contains("6809Emu"));
    }

    #[test]
    fn dragon32_apply_sets_reset_vector() {
        let mut emu = Emulator::new();
        let cfg = apply_machine(&mut emu, MachineKind::Dragon32);
        assert_eq!(cfg.reset_pc, 0xC000);
        assert_eq!(emu.cpu.pc, 0xC000);
    }

    #[test]
    fn dragon32_boot_banner_on_screen() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Dragon32);
        for _ in 0..8000 {
            if emu.cpu.pc == 0x0100 {
                break;
            }
            emu.step();
        }
        let frame = machine_video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "Text32x16");
        assert_eq!(frame.base_addr, 0x0C00);
        assert!(frame.rows_text[0].contains("6809Emu"));
    }

    #[test]
    fn coco2_video_frame_reads_text_at_0400() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        emu.memory.write8(0x0400, b'H');
        emu.memory.write8(0x0401, b'I');
        let frame = machine_video_frame(&emu).expect("frame");
        assert_eq!(frame.base_addr, 0x0400);
        assert_eq!(frame.mode, "Text32x16");
        assert!(frame.rows_text[0].starts_with("HI"));
    }

    #[test]
    fn coco2_sam_relocates_video_base() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        emu.memory.write8(0xFFCB, 0x00); // set screen addr bit 2
        emu.memory.write8(0x0C00, b'X');
        let frame = machine_video_frame(&emu).expect("frame");
        assert_eq!(frame.base_addr, 0x0C00);
        assert!(frame.rows_text[0].starts_with('X'));
    }

    #[test]
    fn bare_acia_echo_via_irq() {
        use m6809_asm::assemble;

        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Bare);
        set_acia_config(
            &mut emu,
            AciaConfig {
                enabled: true,
                base_addr: 0xFFA0,
                baud: 1_000_000,
                e_clock_hz: 1_000_000,
            },
        );

        let source = r#"
        ORG $0100
        ANDCC #$EF
        LDA  #$C2
        STA  $FFA1
        LDA  #$03
        STA  $FFA1
idle    BRA  idle
        ORG $0200
irq     LDA  $FFA1
        ANDA #$01
        BEQ  no_rx
        LDA  $FFA0
        STA  $FFA0
no_rx   RTI
        END
"#;
        let program = assemble(source).expect("assemble");
        emu.memory
            .load_binary(program.origin, &program.bytes)
            .expect("load");
        emu.memory.write16(0xFFF8, 0x0200);
        assert_eq!(emu.memory.read16(0xFFF8), 0x0200);
        assert_eq!(emu.memory.read8(0x0200), 0xB6, "irq handler should start with LDA");
        emu.cpu.pc = 0x0100;
        emu.cpu.halted = false;

        for _ in 0..6 {
            emu.step();
        }
        acia_send_input(&emu, "E");
        for _ in 0..5000 {
            if get_acia_terminal(&emu).tx_text.contains('E') {
                break;
            }
            emu.step();
        }
        assert!(
            get_acia_terminal(&emu).tx_text.contains('E'),
            "expected echo in TX buffer"
        );
    }

    #[test]
    fn frontend_acia_echo_example_assembles() {
        use m6809_asm::assemble;
        let source = r#"
        ORG $0100
        ANDCC #$EF
        LDA  #$C2
        STA  $FFA1
        LDA  #$03
        STA  $FFA1
idle    BRA  idle
        ORG $0200
irq     LDA  $FFA1
        ANDA #$01
        BEQ  no_rx
        LDA  $FFA0
        STA  $FFA0
no_rx   RTI
        ORG $FFF8
        FDB  $0200
        END
"#;
        let program = assemble(source).expect("frontend acia example");
        assert_eq!(program.origin, 0x0100);
        assert!(program.bytes.len() > 10);
    }
}