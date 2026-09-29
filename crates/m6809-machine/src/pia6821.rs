//! Bare-metal MC6821 PIA at a configurable base address (4 registers).
//!
//! The chip logic lives in [`BoardPia`] (shared with the CoCo / Dragon
//! boards). Here the port pins and the CA1/CA2/CB1/CB2 control lines are
//! driven from the UI; the combined /IRQA + /IRQB output is reported by
//! [`Pia6821::poll_irq`] and wired to the CPU /IRQ line by the machine.

use std::cell::RefCell;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

use crate::board_pia::BoardPia;

pub const DEFAULT_BASE_ADDR: u16 = 0xFF10;

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
    /// External drive on the port A pins (1 = released / high).
    pub ira: u8,
    /// External levels on the port B pins.
    pub irb: u8,
    /// CRA as the CPU reads it (bit 7 = IRQA1 flag, bit 6 = IRQA2 flag).
    pub cra: u8,
    /// CRB as the CPU reads it (bit 7 = IRQB1 flag, bit 6 = IRQB2 flag).
    pub crb: u8,
    /// Port A value the CPU would read (pin levels).
    pub port_a_read: u8,
    /// Port B value the CPU would read (latch for outputs, pins for inputs).
    pub port_b_read: u8,
    /// /IRQA output asserted (flag set and enabled).
    pub irq_a: bool,
    /// /IRQB output asserted (flag set and enabled).
    pub irq_b: bool,
    /// IRQ flags (CRA/CRB bits 7 and 6).
    #[serde(default)]
    pub irqa1: bool,
    #[serde(default)]
    pub irqa2: bool,
    #[serde(default)]
    pub irqb1: bool,
    #[serde(default)]
    pub irqb2: bool,
    /// Control line input levels as driven from outside (UI).
    #[serde(default)]
    pub ca1: bool,
    #[serde(default)]
    pub ca2: bool,
    #[serde(default)]
    pub cb1: bool,
    #[serde(default)]
    pub cb2: bool,
    /// CA2 / CB2 configured as outputs (CRx bit 5).
    #[serde(default)]
    pub ca2_is_output: bool,
    #[serde(default)]
    pub cb2_is_output: bool,
    /// Level the PIA drives on CA2 / CB2 (high when the pin is an input).
    #[serde(default)]
    pub ca2_out: bool,
    #[serde(default)]
    pub cb2_out: bool,
    /// Strobe pulses produced on CA2 (read strobe) / CB2 (write strobe).
    #[serde(default)]
    pub ca2_strobes: u32,
    #[serde(default)]
    pub cb2_strobes: u32,
}

#[derive(Debug)]
pub struct Pia6821 {
    config: PiaConfig,
    state: RefCell<BoardPia>,
}

#[derive(Serialize, Deserialize)]
struct PiaSnapshot {
    config: PiaConfig,
    state: BoardPia,
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
            state: RefCell::new(BoardPia::new()),
        }
    }

    pub fn config(&self) -> PiaConfig {
        self.config
    }

    pub fn set_config(&mut self, config: PiaConfig) {
        let was_enabled = self.config.enabled;
        self.config = config;
        if config.enabled && !was_enabled {
            *self.state.borrow_mut() = BoardPia::new();
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled && addr.wrapping_sub(self.config.base_addr) < 4
    }

    fn offset(&self, addr: u16) -> u8 {
        (addr.wrapping_sub(self.config.base_addr) & 3) as u8
    }

    /// Set an input pin on port A (bit 0-7): `on` = released/high, off = pulled low.
    pub fn set_input_a(&self, bit: u8, on: bool) {
        if bit < 8 {
            let mut state = self.state.borrow_mut();
            let ira = if on {
                state.ira | (1 << bit)
            } else {
                state.ira & !(1 << bit)
            };
            state.set_ira(ira);
        }
    }

    /// Set an input pin on port B (bit 0-7).
    pub fn set_input_b(&self, bit: u8, on: bool) {
        if bit < 8 {
            let mut state = self.state.borrow_mut();
            let irb = if on {
                state.irb | (1 << bit)
            } else {
                state.irb & !(1 << bit)
            };
            state.set_irb(irb);
        }
    }

    /// Snapshot of full PIA state for the UI (side-effect free).
    pub fn state_snapshot(&self) -> PiaStateDto {
        let state = self.state.borrow();
        PiaStateDto {
            config: self.config,
            ddra: state.ddra,
            ddrb: state.ddrb,
            ora: state.ora,
            orb: state.orb,
            ira: state.ira,
            irb: state.irb,
            cra: state.peek(1),
            crb: state.peek(3),
            port_a_read: state.port_a_read(state.ira),
            port_b_read: state.port_b_read(state.irb),
            irq_a: state.irq_a(),
            irq_b: state.irq_b(),
            irqa1: state.irqa1_flag(),
            irqa2: state.irqa2_flag(),
            irqb1: state.irqb1_flag(),
            irqb2: state.irqb2_flag(),
            ca1: state.ca1_level(),
            ca2: state.ca2_input(),
            cb1: state.cb1_level(),
            cb2: state.cb2_input(),
            ca2_is_output: state.ca2_is_output(),
            cb2_is_output: state.cb2_is_output(),
            ca2_out: state.ca2_output(),
            cb2_out: state.cb2_output(),
            ca2_strobes: state.ca2_strobe_count(),
            cb2_strobes: state.cb2_strobe_count(),
        }
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        let base = self.config.base_addr;
        vec![
            IoRegisterView {
                address: base,
                name: format!("PIA PA (DDRA ${:02X})", state.ddra),
                value: state.port_a_read(state.ira),
            },
            IoRegisterView {
                address: base.wrapping_add(1),
                name: "PIA CRA".into(),
                value: state.peek(1),
            },
            IoRegisterView {
                address: base.wrapping_add(2),
                name: format!("PIA PB (DDRB ${:02X})", state.ddrb),
                value: state.port_b_read(state.irb),
            },
            IoRegisterView {
                address: base.wrapping_add(3),
                name: "PIA CRB".into(),
                value: state.peek(3),
            },
        ]
    }

    /// CPU read: data reads clear the IRQ flags (and strobe CA2 in strobe mode).
    pub fn read(&self, addr: u16) -> u8 {
        let offset = self.offset(addr);
        self.state.borrow_mut().read(offset)
    }

    /// Side-effect-free register view for the debugger.
    pub fn peek(&self, addr: u16) -> u8 {
        let offset = self.offset(addr);
        self.state.borrow().peek(offset)
    }

    /// Drive a control line from the UI: `line` is "ca1", "ca2", "cb1" or "cb2".
    pub fn set_control_line(&self, line: &str, level: bool) {
        let mut state = self.state.borrow_mut();
        match line.to_ascii_lowercase().as_str() {
            "ca1" => {
                state.set_ca1(level);
            }
            "ca2" => {
                state.set_ca2(level);
            }
            "cb1" => {
                state.set_cb1(level);
            }
            "cb2" => {
                state.set_cb2(level);
            }
            _ => {}
        }
    }

    /// Level of the combined /IRQA + /IRQB output.
    pub fn poll_irq(&self) -> bool {
        self.state.borrow().irq_asserted()
    }

    /// Hardware RESET: all registers cleared; pin levels driven from outside stay.
    pub fn reset(&mut self) {
        self.state.borrow_mut().reset();
    }

    pub fn write(&self, addr: u16, value: u8) {
        let offset = self.offset(addr);
        self.state.borrow_mut().write(offset, value);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pia() -> Pia6821 {
        Pia6821::new(PiaConfig {
            enabled: true,
            base_addr: 0xFF10,
        })
    }

    #[test]
    fn ca1_edge_raises_irq_level_until_data_read() {
        let pia = pia();
        pia.write(0xFF11, 0x05); // data select, CA1 IRQ enabled, falling edge
        assert!(!pia.poll_irq());
        pia.set_control_line("ca1", false);
        assert!(pia.poll_irq());
        // Level stays asserted until acknowledged.
        assert!(pia.poll_irq());
        assert_eq!(pia.peek(0xFF11) & 0x80, 0x80);
        let _ = pia.peek(0xFF10);
        assert!(pia.poll_irq(), "peek must not acknowledge");
        let _ = pia.read(0xFF10);
        assert!(!pia.poll_irq());
    }

    #[test]
    fn cb2_input_edge_irq_and_output_modes() {
        let pia = pia();
        pia.write(0xFF13, 0x1C); // CB2 input, rising edge, IRQ2 enabled, data select
        pia.set_control_line("cb2", false);
        assert!(!pia.poll_irq());
        pia.set_control_line("cb2", true);
        assert!(pia.poll_irq());
        let st = pia.state_snapshot();
        assert!(st.irqb2 && st.irq_b && !st.cb2_is_output);
        let _ = pia.read(0xFF12);
        assert!(!pia.poll_irq());
        // Manual output: level = bit 3.
        pia.write(0xFF13, 0x34);
        let st = pia.state_snapshot();
        assert!(st.cb2_is_output && !st.cb2_out);
        pia.write(0xFF13, 0x3C);
        assert!(pia.state_snapshot().cb2_out);
        assert!(!pia.poll_irq(), "b3 is a level in output mode, not an IRQ enable");
    }

    #[test]
    fn port_inputs_and_reset() {
        let mut pia = pia();
        pia.write(0xFF11, 0x04);
        pia.set_input_a(3, false);
        assert_eq!(pia.read(0xFF10), 0xF7);
        pia.write(0xFF13, 0x04);
        pia.set_input_b(0, false);
        assert_eq!(pia.read(0xFF12), 0xFE);
        pia.set_control_line("ca1", false);
        pia.write(0xFF11, 0x05);
        assert!(pia.poll_irq());
        pia.reset();
        assert!(!pia.poll_irq());
        assert_eq!(pia.read(0xFF11), 0x00);
        // External pin levels survive RESET.
        assert_eq!(pia.state_snapshot().ira, 0xF7);
    }

    #[test]
    fn snapshot_roundtrip_and_old_format() {
        let pia = pia();
        pia.write(0xFF11, 0x3C);
        let snap = pia.snapshot();
        let mut other = Pia6821::new(PiaConfig::default());
        other.restore(&snap);
        assert_eq!(other.state_snapshot().cra, 0x3C);
        // Pre-6821-core snapshots only had the plain registers.
        let old = serde_json::json!({
            "config": { "enabled": true, "base_addr": 0xFF10 },
            "state": { "ddra": 1, "ddrb": 2, "ora": 3, "orb": 4, "ira": 5, "irb": 6, "cra": 4, "crb": 4 }
        });
        other.restore(&old);
        let st = other.state_snapshot();
        assert_eq!((st.ddra, st.ddrb, st.ora, st.orb), (1, 2, 3, 4));
        assert!(st.ca1 && st.cb1);
    }
}
