use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};

use crate::ay38910::{Ay38910, AyConfig, AyStateDto};
use crate::mc6850::{Acia6850, AciaConfig, AciaTerminalDto};
use crate::pia6821::{Pia6821, PiaConfig, PiaStateDto};
use crate::speech::{SpeechBox, SpeechConfig, SpeechStateDto};
use crate::basic_rom::{MSBASIC_ACIA_BASE, MSBASIC_ROM_ADDR};
use crate::peripherals::{CartridgeStateDto, CassetteStateDto, Peripherals};
use crate::{Coco2Machine, Dragon32Machine, MachineKind};

/// Grant Searle 6809 SBC: 7.3728 MHz crystal / 4.
pub const MSBASIC_E_CLOCK_HZ: u32 = 1_843_200;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MachineContainer {
    kind: MachineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    board: Option<BoardState>,
    acia: Acia6850,
    #[serde(skip_serializing_if = "Option::is_none")]
    pia: Option<Pia6821>,
    #[serde(default)]
    ay: Ay38910,
    #[serde(default)]
    speech: SpeechBox,
}

/// Address decode of the Grant Searle SBC outside RAM and ROM.
enum MsBasicRegion {
    /// Not decoded on the board: reads $FF, writes ignored.
    Unmapped,
    /// Minimally decoded 6850 (register address after folding RS = A0).
    Acia(u16),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "state")]
enum BoardState {
    #[serde(rename = "coco2")]
    Coco2(Coco2Machine),
    #[serde(rename = "dragon32")]
    Dragon32(Dragon32Machine),
}

impl MachineContainer {
    pub fn new(kind: MachineKind) -> Self {
        let board = match kind {
            MachineKind::Bare | MachineKind::MsBasic => None,
            MachineKind::Coco2 => Some(BoardState::Coco2(Coco2Machine::new())),
            MachineKind::Dragon32 => Some(BoardState::Dragon32(Dragon32Machine::new())),
        };
        let acia = match kind {
            // Grant Searle SBC: 7.3728 MHz crystal -> E = 1.8432 MHz; the ROM
            // programs the ACIA for /16 -> 115200 baud 8N1.
            MachineKind::MsBasic => Acia6850::new(AciaConfig {
                enabled: true,
                base_addr: MSBASIC_ACIA_BASE,
                baud: 115_200,
                e_clock_hz: MSBASIC_E_CLOCK_HZ,
                motorola: true,
                ..AciaConfig::default()
            }),
            _ => Acia6850::new(AciaConfig::default()),
        };
        Self {
            kind,
            board,
            acia,
            pia: None,
            ay: Ay38910::new(AyConfig::default()),
            speech: SpeechBox::new(SpeechConfig::default()),
        }
    }

    pub fn acia_config(&self) -> AciaConfig {
        self.acia.config()
    }

    pub fn set_acia_config(&mut self, config: AciaConfig) {
        self.acia.set_config(config);
    }

    pub fn acia_terminal(&self) -> AciaTerminalDto {
        self.acia.terminal_state()
    }

    pub fn clear_acia_terminal(&self) {
        self.acia.clear_terminal();
    }

    pub fn acia_send_input(&self, text: &str) {
        // Latin-1, matching the terminal's decoding of the output.
        self.acia.enqueue_rx_text(text);
    }

    /// Grant Searle SBC address decode: $8000-$9FFF is unmapped and the 6850 is
    /// minimally decoded across $A000-$BFFF (RS = A0). `None` = not decoded here.
    fn msbasic_region(&self, addr: u16) -> Option<MsBasicRegion> {
        if self.kind != MachineKind::MsBasic {
            return None;
        }
        match addr {
            0x8000..=0x9FFF => Some(MsBasicRegion::Unmapped),
            0xA000..=0xBFFF
                if self.acia.enabled() && self.acia.config().base_addr == MSBASIC_ACIA_BASE =>
            {
                Some(MsBasicRegion::Acia(MSBASIC_ACIA_BASE | (addr & 1)))
            }
            _ => None,
        }
    }

    pub fn pia_config(&self) -> Option<PiaConfig> {
        self.pia.as_ref().map(|p| p.config())
    }

    pub fn set_pia_config(&mut self, config: PiaConfig) {
        if let Some(pia) = self.pia.as_mut() {
            pia.set_config(config);
            if !config.enabled {
                self.pia = None;
            }
        } else if config.enabled {
            self.pia = Some(Pia6821::new(config));
        }
    }

    pub fn pia_state(&self) -> Option<PiaStateDto> {
        self.pia.as_ref().map(|p| p.state_snapshot())
    }

    pub fn set_pia_input(&self, port: &str, bit: u8, on: bool) {
        if let Some(pia) = &self.pia {
            match port {
                "a" => pia.set_input_a(bit, on),
                "b" => pia.set_input_b(bit, on),
                _ => {}
            }
        }
    }

    // ---- AY-3-8910 ----

    pub fn ay_config(&self) -> AyConfig {
        self.ay.config()
    }

    pub fn set_ay_config(&mut self, config: AyConfig) {
        self.ay.set_config(config);
    }

    pub fn ay_state(&self) -> AyStateDto {
        self.ay.state_snapshot()
    }

    pub fn ay_set_port_input(&self, port: char, value: u8) {
        self.ay.set_port_input(port, value);
    }

    pub fn ay_drain_audio(&mut self) -> Vec<f32> {
        self.ay.drain_audio()
    }

    pub fn ay_fill_audio_to(&mut self, target_count: usize) {
        self.ay.fill_audio_to(target_count);
    }

    pub fn ay_take_samples(&mut self, count: usize) -> Vec<f32> {
        self.ay.take_samples(count)
    }

    // ---- SP0256 / CTS256 speech ----

    pub fn speech_config(&self) -> SpeechConfig {
        self.speech.config()
    }

    pub fn set_speech_config(&mut self, config: SpeechConfig) {
        self.speech.set_config(config);
    }

    pub fn speech_state(&self) -> SpeechStateDto {
        self.speech.state_snapshot()
    }

    pub fn speech_say_text(&mut self, text: &str) {
        self.speech.say_text(text);
    }

    pub fn speech_drain_audio(&mut self) -> Vec<f32> {
        self.speech.drain_audio()
    }

    pub fn speech_take_samples(&mut self, count: usize) -> Vec<f32> {
        self.speech.take_samples(count)
    }

    pub fn speech_run_until_idle(&mut self, max_samples: usize) {
        self.speech.run_until_idle(max_samples);
    }

    /// Render until the speech chips are idle and return all audio produced
    /// (not limited by the 2 s playback buffer).
    pub fn speech_run_until_idle_collect(&mut self, max_samples: usize) -> Vec<f32> {
        self.speech.run_until_idle_collect(max_samples)
    }

    /// CTS256A "O.K." greeting after reset (hardware behaviour) on/off.
    pub fn set_speech_greeting(&mut self, on: bool) {
        self.speech.set_cts_greeting(on);
    }

    /// Drive a control line of the bare-metal 6821 ("ca1" | "ca2" | "cb1" | "cb2").
    pub fn set_pia_control_line(&self, line: &str, level: bool) {
        if let Some(pia) = &self.pia {
            pia.set_control_line(line, level);
        }
    }

    // ---- CoCo / Dragon board peripherals ----

    /// Run `f` on the board peripherals (`None` without a CoCo / Dragon board).
    fn with_peripherals<R>(&self, f: impl FnOnce(&mut Peripherals) -> R) -> Option<R> {
        match self.board.as_ref()? {
            BoardState::Coco2(m) => Some(m.with_peripherals(f)),
            BoardState::Dragon32(m) => Some(m.with_peripherals(f)),
        }
    }

    /// Board sound (6-bit DAC, single-bit sound) since the last drain.
    pub fn board_drain_audio(&mut self) -> Vec<f32> {
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.drain_audio(),
            Some(BoardState::Dragon32(m)) => m.drain_audio(),
            None => Vec::new(),
        }
    }

    pub fn set_joystick(&self, port: usize, x: u8, y: u8, button: bool) {
        self.with_peripherals(|p| p.set_joystick(port, x, y, button));
    }

    pub fn cassette_insert(&self, name: String, data: Vec<u8>) -> Option<CassetteStateDto> {
        self.with_peripherals(|p| p.cassette_insert(name, data))
    }

    pub fn cassette_eject(&self) -> Option<CassetteStateDto> {
        self.with_peripherals(|p| p.cassette_eject())
    }

    pub fn cassette_rewind(&self) -> Option<CassetteStateDto> {
        self.with_peripherals(|p| p.cassette_rewind())
    }

    pub fn cassette_state(&self) -> Option<CassetteStateDto> {
        self.with_peripherals(|p| p.cassette_state())
    }

    pub fn cassette_take_recording(&self) -> Vec<u8> {
        self.with_peripherals(|p| p.cassette_take_recording())
            .unwrap_or_default()
    }

    pub fn printer_take_output(&self) -> String {
        self.with_peripherals(|p| p.printer_take_output())
            .unwrap_or_default()
    }

    pub fn cartridge_insert(
        &self,
        name: String,
        data: Vec<u8>,
        autostart: bool,
    ) -> Option<CartridgeStateDto> {
        self.with_peripherals(|p| p.cartridge_insert(name, data, autostart))
    }

    pub fn cartridge_eject(&self) -> Option<CartridgeStateDto> {
        self.with_peripherals(|p| p.cartridge_eject())
    }

    pub fn cartridge_state(&self) -> Option<CartridgeStateDto> {
        self.with_peripherals(|p| p.cartridge_state())
    }

    /// VDG / SAM video inputs of the CoCo / Dragon board (`None` without video).
    pub fn vdg_inputs(&self) -> Option<crate::vdg::VdgInputs> {
        match self.board.as_ref()? {
            BoardState::Coco2(m) => Some(m.vdg_inputs()),
            BoardState::Dragon32(m) => Some(m.vdg_inputs()),
        }
    }

    pub fn host_key(&mut self, code: &str, down: bool) {
        self.host_key_event(code, None, down);
    }

    pub fn host_key_event(&mut self, code: &str, key: Option<&str>, down: bool) {
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.host_key_event(code, key, down),
            Some(BoardState::Dragon32(m)) => m.host_key_event(code, key, down),
            None => {}
        }
    }

    pub fn clear_keys(&mut self) {
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.clear_keys(),
            Some(BoardState::Dragon32(m)) => m.clear_keys(),
            None => {}
        }
    }
}

impl MemoryIo for MachineContainer {
    fn kind_id(&self) -> &str {
        self.kind.id()
    }

    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        if self.speech.handles(addr) {
            return Some(self.speech.read(addr));
        }
        if self.ay.enabled() && self.ay.handles(addr) {
            return Some(self.ay.read(addr));
        }
        match self.msbasic_region(addr) {
            Some(MsBasicRegion::Unmapped) => return Some(0xFF),
            Some(MsBasicRegion::Acia(reg)) => return Some(self.acia.read(reg)),
            None => {}
        }
        if self.acia.enabled() && self.acia.handles(addr) {
            return Some(self.acia.read(addr));
        }
        if let Some(pia) = &self.pia {
            if pia.handles(addr) {
                return Some(pia.read(addr));
            }
        }
        match self.board.as_ref()? {
            BoardState::Coco2(m) => m.read(addr, ram),
            BoardState::Dragon32(m) => m.read(addr, ram),
        }
    }

    fn peek(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        if self.speech.handles(addr) {
            return Some(self.speech.peek(addr));
        }
        if self.ay.enabled() && self.ay.handles(addr) {
            return Some(self.ay.peek(addr));
        }
        match self.msbasic_region(addr) {
            Some(MsBasicRegion::Unmapped) => return Some(0xFF),
            Some(MsBasicRegion::Acia(reg)) => return Some(self.acia.peek(reg)),
            None => {}
        }
        if self.acia.enabled() && self.acia.handles(addr) {
            return Some(self.acia.peek(addr));
        }
        if let Some(pia) = &self.pia {
            if pia.handles(addr) {
                return Some(pia.peek(addr));
            }
        }
        match self.board.as_ref()? {
            BoardState::Coco2(m) => m.peek(addr, ram),
            BoardState::Dragon32(m) => m.peek(addr, ram),
        }
    }

    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        // Memory-mapped devices first: on the MsBasic board they live above the
        // ROM base and must not be swallowed by the ROM write protection.
        if self.speech.handles(addr) {
            self.speech.write(addr, value);
            return IoWriteResult::Consumed;
        }
        if self.ay.enabled() && self.ay.handles(addr) {
            self.ay.write(addr, value);
            return IoWriteResult::Consumed;
        }
        match self.msbasic_region(addr) {
            Some(MsBasicRegion::Unmapped) => return IoWriteResult::Ignored,
            Some(MsBasicRegion::Acia(reg)) => {
                self.acia.write(reg, value);
                return IoWriteResult::Consumed;
            }
            None => {}
        }
        if self.acia.enabled() && self.acia.handles(addr) {
            self.acia.write(addr, value);
            return IoWriteResult::Consumed;
        }
        if let Some(pia) = &self.pia {
            if pia.handles(addr) {
                pia.write(addr, value);
                return IoWriteResult::Consumed;
            }
        }
        if self.kind == MachineKind::MsBasic && addr >= MSBASIC_ROM_ADDR {
            return IoWriteResult::Ignored;
        }
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.write(addr, value, ram),
            Some(BoardState::Dragon32(m)) => m.write(addr, value, ram),
            None => IoWriteResult::PassThrough,
        }
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

    fn io_registers(&self) -> Vec<IoRegisterView> {
        let mut regs = match self.board.as_ref() {
            Some(BoardState::Coco2(m)) => m.io_registers(),
            Some(BoardState::Dragon32(m)) => m.io_registers(),
            None => Vec::new(),
        };
        regs.extend(self.acia.io_registers());
        if let Some(pia) = &self.pia {
            regs.extend(pia.io_registers());
        }
        regs.extend(self.ay.io_registers());
        regs.extend(self.speech.io_registers());
        regs
    }

    fn tick(&mut self, cycles: u32) {
        // Devices convert E cycles to real time with the machine's current clock.
        let clock_hz = self.cpu_clock_hz().unwrap_or(crate::ay38910::HOST_CLOCK_HZ);
        self.ay.set_host_clock_hz(clock_hz);
        self.speech.set_host_clock_hz(clock_hz);
        self.acia.tick(cycles);
        self.ay.tick(cycles);
        self.speech.tick(cycles);
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_tick(cycles),
            Some(BoardState::Dragon32(m)) => m.board_tick(cycles),
            None => {}
        }
    }

    fn poll_irq(&mut self) -> bool {
        let mut irq = self.acia.poll_irq();
        if let Some(pia) = &self.pia {
            irq |= pia.poll_irq();
        }
        irq |= match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_poll_irq(),
            Some(BoardState::Dragon32(m)) => m.board_poll_irq(),
            None => false,
        };
        irq
    }

    fn poll_firq(&mut self) -> bool {
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_poll_firq(),
            Some(BoardState::Dragon32(m)) => m.board_poll_firq(),
            None => false,
        }
    }

    fn reset(&mut self) {
        self.acia.reset();
        self.ay.reset();
        self.speech.reset();
        if let Some(pia) = self.pia.as_mut() {
            pia.reset();
        }
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_reset(),
            Some(BoardState::Dragon32(m)) => m.board_reset(),
            None => {}
        }
    }

    fn cpu_clock_hz(&self) -> Option<u32> {
        match self.board.as_ref() {
            Some(BoardState::Coco2(m)) => Some(m.cpu_clock_hz()),
            Some(BoardState::Dragon32(m)) => Some(m.cpu_clock_hz()),
            None if self.kind == MachineKind::MsBasic => Some(MSBASIC_E_CLOCK_HZ),
            None => None,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}