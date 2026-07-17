use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};

use crate::ay38910::{Ay38910, AyConfig, AyStateDto};
use crate::mc6850::{Acia6850, AciaConfig, AciaTerminalDto};
use crate::pia6821::{Pia6821, PiaConfig, PiaStateDto};
use crate::{Coco2Machine, Dragon32Machine, MachineKind};

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
            MachineKind::Bare => None,
            MachineKind::Coco2 => Some(BoardState::Coco2(Coco2Machine::new())),
            MachineKind::Dragon32 => Some(BoardState::Dragon32(Dragon32Machine::new())),
        };
        Self {
            kind,
            board,
            acia: Acia6850::new(AciaConfig::default()),
            pia: None,
            ay: Ay38910::new(AyConfig::default()),
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
        self.acia.enqueue_rx(text.as_bytes());
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

    pub fn host_key(&mut self, code: &str, down: bool) {
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.host_key(code, down),
            Some(BoardState::Dragon32(m)) => m.host_key(code, down),
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
        if self.ay.enabled() && self.ay.handles(addr) {
            return Some(self.ay.read(addr));
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

    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        if self.ay.enabled() && self.ay.handles(addr) {
            self.ay.write(addr, value);
            return IoWriteResult::Consumed;
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
        regs
    }

    fn tick(&mut self, cycles: u32) {
        self.acia.tick(cycles);
        self.ay.tick(cycles);
        match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_tick(cycles),
            Some(BoardState::Dragon32(m)) => m.board_tick(cycles),
            None => {}
        }
    }

    fn poll_irq(&mut self) -> bool {
        let mut irq = self.acia.poll_irq();
        irq |= match self.board.as_mut() {
            Some(BoardState::Coco2(m)) => m.board_poll_irq(),
            Some(BoardState::Dragon32(m)) => m.board_poll_irq(),
            None => false,
        };
        irq
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}