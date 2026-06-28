use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};

use crate::mc6850::{Acia6850, AciaConfig, AciaTerminalDto};
use crate::{Coco2Machine, Dragon32Machine, MachineKind};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MachineContainer {
    kind: MachineKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    board: Option<BoardState>,
    acia: Acia6850,
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

    pub fn acia_send_input(&self, text: &str) {
        self.acia.enqueue_rx(text.as_bytes());
    }
}

impl MemoryIo for MachineContainer {
    fn kind_id(&self) -> &str {
        self.kind.id()
    }

    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        if self.acia.enabled() && self.acia.handles(addr) {
            return Some(self.acia.read(addr));
        }
        match self.board.as_ref()? {
            BoardState::Coco2(m) => m.read(addr, ram),
            BoardState::Dragon32(m) => m.read(addr, ram),
        }
    }

    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        if self.acia.enabled() && self.acia.handles(addr) {
            self.acia.write(addr, value);
            return IoWriteResult::Consumed;
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
        regs
    }

    fn tick(&mut self, cycles: u32) {
        let _ = cycles;
        self.acia.tick(cycles);
    }

    fn poll_irq(&mut self) -> bool {
        self.acia.poll_irq()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}