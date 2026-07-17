use std::cell::RefCell;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_ADDR: u16 = 0xFF10;

// Control register bits
const CRA_CA1_IRQ: u8 = 0x80; // IRQ flag for CA1
const CRA_CA2_IRQ: u8 = 0x40; // IRQ flag for CA2
const CRA_DDR_SELECT: u8 = 0x04; // 0 = DDR, 1 = peripheral register
const CRB_CB1_IRQ: u8 = 0x80; // IRQ flag for CB1
const CRB_CB2_IRQ: u8 = 0x40; // IRQ flag for CB2
const CRB_DDR_SELECT: u8 = 0x04; // 0 = DDR, 1 = peripheral register

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PiaConfig {
    pub enabled: bool,
    pub base_addr: u16,
}

impl Default for PiaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_addr: DEFAULT_BASE_ADDR,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiaStateDto {
    pub config: PiaConfig,
    pub ddra: u8,
    pub ddrb: u8,
    pub ora: u8,
    pub orb: u8,
    pub ira: u8,
    pub irb: u8,
    pub cra: u8,
    pub crb: u8,
    pub port_a_read: u8,
    pub port_b_read: u8,
    pub irq_a: bool,
    pub irq_b: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PiaState {
    ddra: u8,
    ddrb: u8,
    ora: u8,
    orb: u8,
    ira: u8,
    irb: u8,
    cra: u8,
    crb: u8,
}

#[derive(Debug)]
pub struct Pia6821 {
    config: PiaConfig,
    state: RefCell<PiaState>,
}

#[derive(Serialize, Deserialize)]
struct PiaSnapshot {
    config: PiaConfig,
    state: PiaState,
}

impl Serialize for Pia6821 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PiaSnapshot {
            config: self.config,
            state: self.state.borrow().clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Pia6821 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let snap = PiaSnapshot::deserialize(deserializer)?;
        Ok(Self {
            config: snap.config,
            state: RefCell::new(snap.state),
        })
    }
}

impl Clone for Pia6821 {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            state: RefCell::new(self.state.borrow().clone()),
        }
    }
}

impl Pia6821 {
    pub fn new(config: PiaConfig) -> Self {
        Self {
            config,
            state: RefCell::new(PiaState::default()),
        }
    }

    pub fn config(&self) -> PiaConfig {
        self.config
    }

    pub fn set_config(&mut self, config: PiaConfig) {
        let was_enabled = self.config.enabled;
        self.config = config;
        if config.enabled && !was_enabled {
            *self.state.borrow_mut() = PiaState::default();
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled && addr.wrapping_sub(self.config.base_addr) < 4
    }

    /// Set an input pin on port A (bit 0-7).
    pub fn set_input_a(&self, bit: u8, on: bool) {
        let mut state = self.state.borrow_mut();
        if bit < 8 {
            if on {
                state.ira |= 1 << bit;
            } else {
                state.ira &= !(1 << bit);
            }
        }
    }

    /// Set an input pin on port B (bit 0-7).
    pub fn set_input_b(&self, bit: u8, on: bool) {
        let mut state = self.state.borrow_mut();
        if bit < 8 {
            if on {
                state.irb |= 1 << bit;
            } else {
                state.irb &= !(1 << bit);
            }
        }
    }

    /// Snapshot of full PIA state for the UI.
    pub fn state_snapshot(&self) -> PiaStateDto {
        let state = self.state.borrow();
        let port_a_read = port_read(state.ora, state.ira, state.ddra);
        let port_b_read = port_read(state.orb, state.irb, state.ddrb);
        PiaStateDto {
            config: self.config,
            ddra: state.ddra,
            ddrb: state.ddrb,
            ora: state.ora,
            orb: state.orb,
            ira: state.ira,
            irb: state.irb,
            cra: state.cra,
            crb: state.crb,
            port_a_read,
            port_b_read,
            irq_a: state.cra & (CRA_CA1_IRQ | CRA_CA2_IRQ) != 0,
            irq_b: state.crb & (CRB_CB1_IRQ | CRB_CB2_IRQ) != 0,
        }
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        let port_a_read = port_read(state.ora, state.ira, state.ddra);
        let port_b_read = port_read(state.orb, state.irb, state.ddrb);
        vec![
            IoRegisterView {
                address: self.config.base_addr,
                name: "PIA ORA/IRA".into(),
                value: port_a_read,
            },
            IoRegisterView {
                address: self.config.base_addr.wrapping_add(1),
                name: "PIA CRA".into(),
                value: state.cra,
            },
            IoRegisterView {
                address: self.config.base_addr.wrapping_add(2),
                name: "PIA ORB/IRB".into(),
                value: port_b_read,
            },
            IoRegisterView {
                address: self.config.base_addr.wrapping_add(3),
                name: "PIA CRB".into(),
                value: state.crb,
            },
        ]
    }

    pub fn read(&self, addr: u16) -> u8 {
        let offset = addr.wrapping_sub(self.config.base_addr);
        let state = self.state.borrow();
        match offset {
            0 => {
                // Port A: CRA bit 2 selects DDR vs data
                if state.cra & CRA_DDR_SELECT == 0 {
                    state.ddra
                } else {
                    // Output bits from ORA, input bits from IRA
                    port_read(state.ora, state.ira, state.ddra)
                }
            }
            1 => state.cra,
            2 => {
                // Port B: CRB bit 2 selects DDR vs data
                if state.crb & CRB_DDR_SELECT == 0 {
                    state.ddrb
                } else {
                    port_read(state.orb, state.irb, state.ddrb)
                }
            }
            3 => state.crb,
            _ => 0xFF,
        }
    }

    pub fn write(&self, addr: u16, value: u8) {
        let offset = addr.wrapping_sub(self.config.base_addr);
        let mut state = self.state.borrow_mut();
        match offset {
            0 => {
                if state.cra & CRA_DDR_SELECT == 0 {
                    state.ddra = value;
                } else {
                    state.ora = value;
                }
            }
            1 => {
                // Control register: only bits 0-5 are writable (IRQ flags are read-only)
                state.cra = (state.cra & (CRA_CA1_IRQ | CRA_CA2_IRQ)) | (value & 0x3F);
            }
            2 => {
                if state.crb & CRB_DDR_SELECT == 0 {
                    state.ddrb = value;
                } else {
                    state.orb = value;
                }
            }
            3 => {
                state.crb = (state.crb & (CRB_CB1_IRQ | CRB_CB2_IRQ)) | (value & 0x3F);
            }
            _ => {}
        }
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(PiaSnapshot {
            config: self.config,
            state: self.state.borrow().clone(),
        })
        .unwrap_or_default()
    }

    pub fn restore(&mut self, snapshot: &serde_json::Value) {
        if let Ok(snap) = serde_json::from_value::<PiaSnapshot>(snapshot.clone()) {
            self.config = snap.config;
            *self.state.borrow_mut() = snap.state;
        }
    }
}

/// Compute the value read from a port: output bits come from OR, input bits from IR.
fn port_read(ora: u8, ira: u8, ddra: u8) -> u8 {
    (ora & ddra) | (ira & !ddra)
}
