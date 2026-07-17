mod ay38910;
mod basic_rom;
mod board_pia;
mod coco2;
#[allow(dead_code)]
mod coco2_rom;
mod dragon32;
#[allow(dead_code)]
mod dragon_rom;
mod keyboard;
mod machine_io;
mod mc6850;
mod pia6821;
mod sam;
mod vdg;
mod video;

use m6809_core::{Emulator, IoRegisterView, LoadConfig, MemoryIo};
use serde::{Deserialize, Serialize};

pub use ay38910::{Ay38910, AyConfig, AyStateDto, AUDIO_SAMPLE_RATE};
pub use basic_rom::{firmware_info, FirmwareInfo, FirmwareRegion};
pub use coco2::Coco2Machine;
pub use dragon32::Dragon32Machine;
pub use machine_io::MachineContainer;
pub use mc6850::{AciaConfig, AciaTerminalDto, Acia6850};
pub use pia6821::{Pia6821, PiaConfig, PiaStateDto};
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
    let coco_fw = firmware_info(MachineKind::Coco2);
    let dgn_fw = firmware_info(MachineKind::Dragon32);
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
            reset_pc: coco_fw.reset_pc,
            description: format!(
                "CoCo 2 with PIA/SAM/VDG and {} ($8000/$A000).",
                coco_fw.name
            ),
        },
        MachineInfo {
            kind: MachineKind::Dragon32,
            name: "Dragon 32".into(),
            load_addr: 0x0100,
            reset_pc: dgn_fw.reset_pc,
            description: format!(
                "Dragon 32 with PIA/SAM/VDG and {} ($8000).",
                dgn_fw.name
            ),
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

pub fn clear_acia_terminal(emu: &m6809_core::Emulator) {
    if let Some(c) = machine_container(emu) {
        c.clear_acia_terminal();
    }
}

pub fn acia_send_input(emu: &m6809_core::Emulator, text: &str) {
    if let Some(c) = machine_container(emu) {
        c.acia_send_input(text);
    }
}

pub fn get_pia_config(emu: &m6809_core::Emulator) -> Option<PiaConfig> {
    machine_container(emu).and_then(|c| c.pia_config())
}

pub fn set_pia_config(emu: &mut m6809_core::Emulator, config: PiaConfig) {
    if let Some(c) = machine_container_mut(emu) {
        c.set_pia_config(config);
    }
}

pub fn get_pia_state(emu: &m6809_core::Emulator) -> Option<PiaStateDto> {
    machine_container(emu).and_then(|c| c.pia_state())
}

pub fn set_pia_input(emu: &m6809_core::Emulator, port: &str, bit: u8, on: bool) {
    if let Some(c) = machine_container(emu) {
        c.set_pia_input(port, bit, on);
    }
}

// ---- AY-3-8910 helpers ----

pub fn get_ay_config(emu: &m6809_core::Emulator) -> AyConfig {
    machine_container(emu).map(|c| c.ay_config()).unwrap_or_default()
}

pub fn set_ay_config(emu: &mut m6809_core::Emulator, config: AyConfig) {
    if let Some(c) = machine_container_mut(emu) {
        c.set_ay_config(config);
    }
}

pub fn get_ay_state(emu: &m6809_core::Emulator) -> Option<AyStateDto> {
    let c = machine_container(emu)?;
    if c.ay_config().enabled {
        Some(c.ay_state())
    } else {
        None
    }
}

pub fn ay_set_port_input(emu: &m6809_core::Emulator, port: char, value: u8) {
    if let Some(c) = machine_container(emu) {
        c.ay_set_port_input(port, value);
    }
}

pub fn ay_drain_audio(emu: &mut m6809_core::Emulator) -> Vec<f32> {
    match machine_container_mut(emu) {
        Some(c) => c.ay_drain_audio(),
        None => Vec::new(),
    }
}

pub fn ay_fill_audio(emu: &mut m6809_core::Emulator, target_samples: usize) {
    if let Some(c) = machine_container_mut(emu) {
        c.ay_fill_audio_to(target_samples);
    }
}

/// Render exactly `count` continuous samples for wall-clock playback.
pub fn ay_take_samples(emu: &mut m6809_core::Emulator, count: usize) -> Vec<f32> {
    match machine_container_mut(emu) {
        Some(c) => c.ay_take_samples(count),
        None => Vec::new(),
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

    let reset_pc = match kind {
        MachineKind::Bare => info.reset_pc,
        MachineKind::Coco2 | MachineKind::Dragon32 => {
            basic_rom::install_firmware(&mut emu.memory.ram, kind, info.reset_pc)
        }
    };

    let config = LoadConfig {
        load_addr: info.load_addr,
        reset_pc,
    };
    emu.memory.config = config.clone();
    // Vectors already written by firmware install; re-assert reset vector.
    if kind != MachineKind::Bare {
        emu.memory.ram[0xFFFE] = (reset_pc >> 8) as u8;
        emu.memory.ram[0xFFFF] = (reset_pc & 0xFF) as u8;
    } else {
        emu.memory.write16(0xFFFE, reset_pc);
    }
    emu.cpu.breakpoints.clear();
    emu.memory.clear_all_watchpoints();
    emu.reset();
    config
}

/// Inject a host keyboard event into the active CoCo/Dragon matrix.
pub fn machine_host_key(emu: &mut Emulator, code: &str, down: bool) {
    if let Some(c) = machine_container_mut(emu) {
        c.host_key(code, down);
    }
}

/// Clear all keys on the active machine keyboard matrix.
pub fn machine_clear_keys(emu: &mut Emulator) {
    if let Some(c) = machine_container_mut(emu) {
        c.clear_keys();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineStateDto {
    pub kind: MachineKind,
    pub io_registers: Vec<IoRegisterView>,
    pub acia: AciaConfig,
    pub pia: Option<PiaConfig>,
    #[serde(default)]
    pub ay: AyConfig,
    #[serde(default)]
    pub firmware: Option<FirmwareInfo>,
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
    let pia = get_pia_config(emu);
    let ay = get_ay_config(emu);
    let firmware = if kind == MachineKind::Bare {
        None
    } else {
        Some(firmware_info(kind))
    };
    MachineStateDto {
        kind,
        io_registers,
        acia,
        pia,
        ay,
        firmware,
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
    fn coco2_loads_microsoft_basic_and_reset_vector() {
        let mut emu = Emulator::new();
        let cfg = apply_machine(&mut emu, MachineKind::Coco2);
        assert_eq!(cfg.reset_pc, 0xA027);
        assert_eq!(emu.cpu.pc, 0xA027);
        assert_eq!(emu.memory.read16(0xFFFE), 0xA027);
        // Color BASIC signature region + Extended BASIC present
        assert_ne!(emu.memory.read8(0xA000), 0x00);
        assert_ne!(emu.memory.read8(0x8000), 0x00);
        // ROM write-protect
        emu.memory.write8(0xA000, 0x00);
        assert_ne!(emu.memory.read8(0xA000), 0x00);
    }

    #[test]
    fn coco2_keyboard_defaults_no_keys() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        // Configure PIA0 data access, DDRB all outputs, DDRA all inputs
        emu.memory.write8(0xFF01, 0x00);
        emu.memory.write8(0xFF00, 0x00); // DDRA inputs
        emu.memory.write8(0xFF01, 0x04);
        emu.memory.write8(0xFF03, 0x00);
        emu.memory.write8(0xFF02, 0xFF); // DDRB outputs
        emu.memory.write8(0xFF03, 0x04);
        emu.memory.write8(0xFF02, 0x00); // select all columns
        // No keys → rows high
        assert_eq!(emu.memory.read8(0xFF00), 0xFF);
    }

    #[test]
    fn coco2_basic_prints_banner() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        // Run cold start long enough to reach READY/OK prompt.
        for _ in 0..2_000_000 {
            emu.step();
            if emu.cpu.halted {
                break;
            }
        }
        let frame = machine_video_frame(&emu).expect("frame");
        let screen: String = frame.rows_text.join("");
        let upper = screen.to_ascii_uppercase();
        assert!(
            upper.contains("BASIC") || upper.contains("OK") || upper.contains("MICROSOFT"),
            "expected BASIC banner on screen, got: {screen:?}"
        );
    }

    #[test]
    fn dragon32_loads_basic_and_reset_vector() {
        let mut emu = Emulator::new();
        let cfg = apply_machine(&mut emu, MachineKind::Dragon32);
        assert_eq!(cfg.reset_pc, 0xB3B4);
        assert_eq!(emu.cpu.pc, 0xB3B4);
        assert_eq!(emu.memory.read16(0xFFFE), 0xB3B4);
    }

    #[test]
    fn dragon32_basic_prints_banner() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Dragon32);
        for _ in 0..2_000_000 {
            emu.step();
            if emu.cpu.halted {
                break;
            }
        }
        let frame = machine_video_frame(&emu).expect("frame");
        let screen: String = frame.rows_text.join("");
        let upper = screen.to_ascii_uppercase();
        assert!(
            upper.contains("BASIC") || upper.contains("OK") || upper.contains("DRAGON"),
            "expected Dragon BASIC banner, got: {screen:?}"
        );
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
    fn frontend_coco2video_example_draws_banner() {
        use m6809_asm::assemble;
        use std::path::Path;

        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/examples.ts"),
        )
        .expect("examples.ts");
        let after = text
            .split("id: \"coco2video\"")
            .nth(1)
            .expect("coco2video example");
        let source = after
            .split("source: `")
            .nth(1)
            .expect("source")
            .split('`')
            .next()
            .expect("end");
        let program = assemble(source).expect("assemble coco2video");
        assert!(
            program.bytes.len() < 0x300,
            "example must not span into VRAM @ $0400 (len={})",
            program.bytes.len()
        );

        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        emu.load_program(program.origin, &program.bytes)
            .expect("load");
        emu.cpu.pc = 0x0100;
        emu.cpu.halted = false;

        // Init draws CLS+border+banner then enters the frame loop.
        for _ in 0..50_000 {
            emu.step();
        }
        let frame = machine_video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "Text32x16");
        assert_eq!(frame.base_addr, 0x0400);
        let screen = frame.rows_text.join("");
        assert!(
            screen.contains("6809EMU"),
            "banner missing, row0={:?}",
            frame.rows_text.get(0)
        );
        assert!(
            screen.contains('*') || screen.contains("TEXT MODE"),
            "sprite or third banner line missing"
        );
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
        LDA  #$42
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
        // Mirrors src/lib/examples.ts aciaecho (core flow).
        let source = r#"
        ORG $0100
START   LDS  #$01FF
        LDA  #$C0
        STA  $FFA1
        LDA  #$42
        STA  $FFA1
        ANDCC #$EF
        LDX  #$0180
PUTS    LDA  ,X+
        BEQ  IDLE
WAITTX  LDB  $FFA1
        ANDB #$02
        BEQ  WAITTX
        STA  $FFA0
        BRA  PUTS
IDLE    BRA  IDLE
        ORG $0180
        FCB $41,$43,$49,$41,$00
        ORG $0200
IRQ     LDA  $FFA1
        ANDA #$01
        BEQ  NORX
        LDA  $FFA0
W2      LDB  $FFA1
        ANDB #$02
        BEQ  W2
        STA  $FFA0
NORX    RTI
        ORG $FFF8
        FDB  $0200
        END
"#;
        let program = assemble(source).expect("frontend acia example");
        assert_eq!(program.origin, 0x0100);
        assert!(program.bytes.len() > 10);
        assert_eq!(program.bytes[0x0200 - 0x0100], 0xB6, "IRQ handler LDA ext");
    }

    #[test]
    fn frontend_ay_music_example_from_examples_ts_assembles() {
        use m6809_asm::assemble;
        use std::path::Path;

        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/examples.ts"),
        )
        .expect("examples.ts");
        let after = text
            .split("id: \"aymusic\"")
            .nth(1)
            .expect("aymusic example");
        let source = after
            .split("source: `")
            .nth(1)
            .expect("source")
            .split('`')
            .next()
            .expect("end");
        let program = assemble(source).expect("assemble aymusic from examples.ts");
        assert_eq!(program.origin, 0x0100);
        assert!(program.bytes.len() > 10);
    }

    #[test]
    fn frontend_ay_music_example_assembles_and_plays() {
        use m6809_asm::assemble;

        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Bare);
        set_ay_config(
            &mut emu,
            AyConfig {
                enabled: true,
                base_addr: 0xFF40,
                chip_clock_hz: 1_000_000,
            },
        );

        // Mirrors src/lib/examples.ts aymusic (core flow).
        let source = r#"
        ORG $0100
START    LDS  #$01FF
         LDA  #7
         STA  $FF40
         LDA  #$1C
         STA  $FF41
         LDA  #6
         STA  $FF40
         LDA  #8
         STA  $FF41
         LDA  #8
         STA  $FF40
         LDA  #$10
         STA  $FF41
         LDA  #9
         STA  $FF40
         LDA  #$0C
         STA  $FF41
         LDA  #10
         STA  $FF40
         LDA  #$0A
         STA  $FF41
         LDA  #11
         STA  $FF40
         LDA  #$E8
         STA  $FF41
         LDA  #12
         STA  $FF40
         LDA  #$03
         STA  $FF41
         LDA  #13
         STA  $FF40
         LDA  #$0A
         STA  $FF41
         LDA  #2
         STA  $FF40
         LDA  #$BC
         STA  $FF41
         LDA  #3
         STA  $FF40
         LDA  #$03
         STA  $FF41
         CLRA
         STA  $0200
         STA  $0201
         LDA  #40
         STA  $0202
MAIN     LDA  $0201
         ADDA #1
         STA  $0201
         LDA  $0202
         SUBA #1
         STA  $0202
         BNE  SKIPNOTE
         LDA  #40
         STA  $0202
         LDA  $0200
         ADDA #1
         STA  $0200
         LDA  $0200
         ANDA #$07
         STA  $0200
         LDB  $0200
         ASLB
         LDX  #$0280
         ABX
         LDA  ,X
         PSHS B
         LDB  #0
         STB  $FF40
         STA  $FF41
         LDA  1,X
         LDB  #1
         STB  $FF40
         STA  $FF41
         PULS B
         LDA  #13
         STA  $FF40
         LDA  #$0A
         STA  $FF41
SKIPNOTE BSR  DELAY
         BRA  MAIN
DELAY    PSHS A,B
         LDA  #$20
D1       LDB  #$FF
D2       SUBB #1
         BNE  D2
         SUBA #1
         BNE  D1
         PULS A,B,PC
         ORG $0280
NOTETAB  FCB $EE,$00, $BD,$00, $9F,$00, $77,$00
         FCB $5F,$00, $50,$00, $3C,$00, $2F,$00
         END
"#;
        let program = assemble(source).expect("frontend ay example");
        assert_eq!(program.origin, 0x0100);
        emu.memory
            .load_binary(program.origin, &program.bytes)
            .expect("load");
        emu.cpu.pc = 0x0100;
        emu.cpu.halted = false;

        // Run enough steps to pass the init and enter the main loop.
        for _ in 0..50_000 {
            emu.step();
        }

        // Verify AY registers were programmed correctly.
        let state = get_ay_state(&emu).expect("ay state");
        assert_eq!(state.registers[7], 0x1C, "mixer register");
        assert_eq!(state.registers[6], 0x08, "noise period");
        assert_eq!(state.registers[8], 0x10, "amp A = env-enabled");
        assert_eq!(state.registers[9], 0x0C, "amp B = 12");
        assert_eq!(state.registers[10], 0x0A, "amp C = 10");
        assert_eq!(state.registers[13], 0x0A, "envelope shape");

        // Wall-clock render path (CPU steps only write registers).
        let audio = ay_take_samples(&mut emu, 2000);
        assert_eq!(audio.len(), 2000, "take_samples should return the requested length");
        assert!(
            audio.iter().any(|&s| s.abs() > 1e-6),
            "audio should contain non-zero samples (tone is playing)"
        );
    }
}