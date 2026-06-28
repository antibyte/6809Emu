use std::cell::RefCell;
use std::collections::VecDeque;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_ADDR: u16 = 0xFFA0;
pub const DEFAULT_BAUD: u32 = 9600;
pub const DEFAULT_E_CLOCK_HZ: u32 = 1_000_000;
pub const TX_HISTORY_CAP: usize = 8192;

const RDRF: u8 = 0x01;
const TDRE: u8 = 0x02;
const IRQ_STATUS: u8 = 0x80;

const RIE: u8 = 0x02;
const TIE: u8 = 0x01;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AciaConfig {
    pub enabled: bool,
    pub base_addr: u16,
    pub baud: u32,
    pub e_clock_hz: u32,
}

impl Default for AciaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_addr: DEFAULT_BASE_ADDR,
            baud: DEFAULT_BAUD,
            e_clock_hz: DEFAULT_E_CLOCK_HZ,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AciaTerminalDto {
    pub tx_text: String,
    pub rdrf: bool,
    pub tdre: bool,
    pub irq: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TxJob {
    byte: u8,
    cycles_left: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RxJob {
    byte: u8,
    cycles_left: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AciaState {
    control: u8,
    rx_queue: VecDeque<u8>,
    rx_pending: Option<RxJob>,
    tx_pending: Option<TxJob>,
    tx_history: Vec<u8>,
    irq_latched: bool,
}

#[derive(Debug)]
pub struct Acia6850 {
    config: AciaConfig,
    state: RefCell<AciaState>,
}

#[derive(Serialize, Deserialize)]
struct AciaSnapshot {
    config: AciaConfig,
    state: AciaState,
}

impl Serialize for Acia6850 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AciaSnapshot {
            config: self.config,
            state: self.state.borrow().clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Acia6850 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let snap = AciaSnapshot::deserialize(deserializer)?;
        Ok(Self {
            config: snap.config,
            state: RefCell::new(snap.state),
        })
    }
}

impl Clone for Acia6850 {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            state: RefCell::new(self.state.borrow().clone()),
        }
    }
}

impl Default for Acia6850 {
    fn default() -> Self {
        Self::new(AciaConfig::default())
    }
}

impl Acia6850 {
    pub fn new(config: AciaConfig) -> Self {
        let mut acia = Self {
            config,
            state: RefCell::new(AciaState::default()),
        };
        acia.apply_master_reset();
        acia
    }

    pub fn config(&self) -> AciaConfig {
        self.config
    }

    pub fn set_config(&mut self, config: AciaConfig) {
        let was_enabled = self.config.enabled;
        self.config = config;
        if config.enabled && !was_enabled {
            self.apply_master_reset();
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled
            && (addr == self.config.base_addr || addr == self.config.base_addr.wrapping_add(1))
    }

    pub fn read(&self, addr: u16) -> u8 {
        if !self.handles(addr) {
            return 0xFF;
        }
        if addr == self.config.base_addr {
            return self.read_data();
        }
        self.read_status()
    }

    pub fn write(&self, addr: u16, value: u8) {
        if !self.handles(addr) {
            return;
        }
        if addr == self.config.base_addr {
            self.write_data(value);
        } else {
            self.write_control(value);
        }
    }

    pub fn tick(&self, cycles: u32) {
        if !self.config.enabled || cycles == 0 {
            return;
        }
        let mut state = self.state.borrow_mut();

        if let Some(job) = state.tx_pending.as_mut() {
            job.cycles_left = job.cycles_left.saturating_sub(cycles);
            if job.cycles_left == 0 {
                let byte = job.byte;
                state.tx_pending = None;
                push_tx_history(&mut state, byte);
            }
        }

        if let Some(job) = state.rx_pending.as_mut() {
            job.cycles_left = job.cycles_left.saturating_sub(cycles);
            if job.cycles_left == 0 {
                let byte = job.byte;
                state.rx_pending = None;
                state.rx_queue.push_back(byte);
            }
        }

        let control = state.control;
        update_irq(&mut state, &self.config, control);
        let irq = irq_active(&state, control);
        if irq {
            state.irq_latched = true;
        }
    }

    pub fn poll_irq(&self) -> bool {
        let mut state = self.state.borrow_mut();
        let pending = state.irq_latched;
        state.irq_latched = false;
        pending
    }

    pub fn enqueue_rx(&self, bytes: &[u8]) {
        if !self.config.enabled {
            return;
        }
        let mut state = self.state.borrow_mut();
        let delay = cycles_per_byte(&self.config);
        for &byte in bytes {
            if state.rx_pending.is_none() && state.rx_queue.is_empty() {
                state.rx_pending = Some(RxJob {
                    byte,
                    cycles_left: delay,
                });
            } else {
                state.rx_queue.push_back(byte);
            }
        }
        let control = state.control;
        update_irq(&mut state, &self.config, control);
        if irq_active(&state, control) {
            state.irq_latched = true;
        }
    }

    pub fn terminal_state(&self) -> AciaTerminalDto {
        let state = self.state.borrow();
        AciaTerminalDto {
            tx_text: String::from_utf8_lossy(&state.tx_history).into_owned(),
            rdrf: rdrf(&state),
            tdre: tdre(&state),
            irq: status_byte(&state, state.control) & IRQ_STATUS != 0,
        }
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        vec![
            IoRegisterView {
                address: self.config.base_addr,
                name: "ACIA Data".into(),
                value: peek_data(&state),
            },
            IoRegisterView {
                address: self.config.base_addr.wrapping_add(1),
                name: "ACIA Status/Ctrl".into(),
                value: status_byte(&state, state.control),
            },
        ]
    }

    fn read_data(&self) -> u8 {
        let mut state = self.state.borrow_mut();
        if state.rx_pending.is_some() {
            return 0x00;
        }
        let byte = state.rx_queue.pop_front().unwrap_or(0x00);
        let control = state.control;
        update_irq(&mut state, &self.config, control);
        byte
    }

    fn write_data(&self, value: u8) {
        let mut state = self.state.borrow_mut();
        if !tdre(&state) {
            return;
        }
        state.tx_pending = Some(TxJob {
            byte: value,
            cycles_left: cycles_per_byte(&self.config),
        });
        let control = state.control;
        update_irq(&mut state, &self.config, control);
    }

    fn write_control(&self, value: u8) {
        let mut state = self.state.borrow_mut();
        if value & 0xC0 == 0xC0 {
            state.control = 0;
            state.rx_queue.clear();
            state.rx_pending = None;
            state.tx_pending = None;
            state.irq_latched = false;
            return;
        }
        state.control = value;
        update_irq(&mut state, &self.config, value);
    }

    fn apply_master_reset(&mut self) {
        let mut state = self.state.borrow_mut();
        state.control = 0;
        state.rx_queue.clear();
        state.rx_pending = None;
        state.tx_pending = None;
        state.irq_latched = false;
        state.tx_history.clear();
    }

    fn read_status(&self) -> u8 {
        let state = self.state.borrow();
        status_byte(&state, state.control)
    }
}

fn rdrf(state: &AciaState) -> bool {
    state.rx_pending.is_none() && !state.rx_queue.is_empty()
}

fn tdre(state: &AciaState) -> bool {
    state.tx_pending.is_none()
}

fn irq_active(state: &AciaState, control: u8) -> bool {
    (control & RIE != 0 && rdrf(state)) || (control & TIE != 0 && tdre(state))
}

fn status_byte(state: &AciaState, control: u8) -> u8 {
    let mut status = 0u8;
    if rdrf(state) {
        status |= RDRF;
    }
    if tdre(state) {
        status |= TDRE;
    }
    if irq_active(state, control) {
        status |= IRQ_STATUS;
    }
    status
}

fn peek_data(state: &AciaState) -> u8 {
    if state.rx_pending.is_some() {
        return 0x00;
    }
    state.rx_queue.front().copied().unwrap_or(0x00)
}

fn update_irq(state: &mut AciaState, _config: &AciaConfig, control: u8) {
    if irq_active(state, control) {
        state.irq_latched = true;
    }
}

fn cycles_per_byte(config: &AciaConfig) -> u32 {
    let baud = config.baud.max(1);
    let e_clock = config.e_clock_hz.max(1);
    let cycles_per_bit = e_clock.div_ceil(baud);
    cycles_per_bit.saturating_mul(10).max(1)
}

fn push_tx_history(state: &mut AciaState, byte: u8) {
    state.tx_history.push(byte);
    if state.tx_history.len() > TX_HISTORY_CAP {
        let drain = state.tx_history.len() - TX_HISTORY_CAP;
        state.tx_history.drain(0..drain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_acia() -> Acia6850 {
        Acia6850::new(AciaConfig {
            enabled: true,
            base_addr: 0xFFA0,
            baud: 1_000_000,
            e_clock_hz: 1_000_000,
        })
    }

    #[test]
    fn reset_has_tdre_set() {
        let acia = enabled_acia();
        assert_eq!(acia.read(0xFFA1) & TDRE, TDRE);
    }

    #[test]
    fn transmit_sets_and_clears_tdre_with_timing() {
        let acia = enabled_acia();
        acia.write(0xFFA0, b'X');
        assert_eq!(acia.read(0xFFA1) & TDRE, 0);
        acia.tick(9);
        assert_eq!(acia.read(0xFFA1) & TDRE, 0);
        acia.tick(1);
        assert_eq!(acia.read(0xFFA1) & TDRE, TDRE);
        assert_eq!(acia.terminal_state().tx_text, "X");
    }

    #[test]
    fn receive_and_read_clears_rdrf() {
        let acia = enabled_acia();
        acia.enqueue_rx(b"Z");
        assert_eq!(acia.read(0xFFA1) & RDRF, 0);
        acia.tick(10);
        assert_eq!(acia.read(0xFFA1) & RDRF, RDRF);
        assert_eq!(acia.read(0xFFA0), b'Z');
        assert_eq!(acia.read(0xFFA1) & RDRF, 0);
    }

    #[test]
    fn receive_interrupt_fires() {
        let acia = enabled_acia();
        acia.write(0xFFA1, RIE);
        acia.enqueue_rx(b"A");
        acia.tick(10);
        assert!(acia.poll_irq());
    }

    #[test]
    fn master_reset_clears_state() {
        let acia = enabled_acia();
        acia.enqueue_rx(b"Q");
        acia.tick(10);
        acia.write(0xFFA1, 0xC0);
        assert_eq!(acia.read(0xFFA1) & RDRF, 0);
        assert_eq!(acia.read(0xFFA1) & TDRE, TDRE);
    }
}