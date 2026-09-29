//! Motorola MC6821 PIA core, shared by the CoCo 2 / Dragon 32 boards and the
//! bare-metal [`crate::pia6821::Pia6821`] device.
//!
//! Register select (RS1, RS0):
//! - 0: DDRA (CRA bit 2 = 0) or port A data (CRA bit 2 = 1)
//! - 1: CRA
//! - 2: DDRB (CRB bit 2 = 0) or port B data (CRB bit 2 = 1)
//! - 3: CRB
//!
//! Control register (same layout for CRA and CRB):
//! - b0: C1 interrupt enable (IRQA/IRQB follows flag b7)
//! - b1: C1 active edge: 0 = high-to-low, 1 = low-to-high
//! - b2: 0 = DDR, 1 = data register
//! - b5 = 0 (C2 is an input): b4 = active edge (0 falling, 1 rising),
//!   b3 = C2 interrupt enable (IRQ follows flag b6)
//! - b5 = 1, b4 = 0 (C2 strobe output): CA2 goes low after a CPU read of
//!   port A data, CB2 after a CPU write of port B data; b3 = 0 returns it
//!   high on the next active C1 edge, b3 = 1 after one E cycle
//! - b5 = 1, b4 = 1 (C2 manual output): C2 = b3
//! - b6: IRQ2 flag (C2 active edge), read-only, always 0 in C2 output mode
//! - b7: IRQ1 flag (C1 active edge), read-only
//!
//! Both flags are cleared by a CPU read of the side's data register (not
//! the DDR). Port A reads the pin levels (an output bit can be pulled low
//! by the outside world); port B reads the output latch for output bits.

use serde::{Deserialize, Serialize};

const CR_C1_IRQ_EN: u8 = 0x01;
const CR_C1_RISING: u8 = 0x02;
const CR_DATA_SELECT: u8 = 0x04;
const CR_C2_BIT3: u8 = 0x08;
const CR_C2_BIT4: u8 = 0x10;
const CR_C2_OUTPUT: u8 = 0x20;
const CR_IRQ2: u8 = 0x40;
const CR_IRQ1: u8 = 0x80;
/// b5..b3 = 1 0 0: C2 strobe output restored by the next active C1 edge.
const C2_STROBE_C1_RESTORE: u8 = CR_C2_OUTPUT;
/// b5..b4 = 1 0: C2 strobe output (either restore mode).
const C2_STROBE_MASK: u8 = CR_C2_OUTPUT | CR_C2_BIT4;

fn high() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardPia {
    pub ddra: u8,
    pub ddrb: u8,
    pub ora: u8,
    pub orb: u8,
    /// External drive on the port A pins (1 = released, 0 = pulled low).
    /// Port A has internal pull-ups, so an undriven pin reads high.
    pub ira: u8,
    /// External levels on the port B pins, seen where DDRB = 0.
    pub irb: u8,
    /// CRA: bits 0-5 as written, bit 6 = IRQA2 flag, bit 7 = IRQA1 flag.
    pub cra: u8,
    /// CRB: bits 0-5 as written, bit 6 = IRQB2 flag, bit 7 = IRQB1 flag.
    pub crb: u8,
    /// CA1 input level.
    #[serde(default = "high")]
    ca1: bool,
    /// CB1 input level.
    #[serde(default = "high")]
    cb1: bool,
    /// CA2 input level (only edges in C2 input mode set IRQA2).
    #[serde(default = "high")]
    ca2_in: bool,
    /// CB2 input level.
    #[serde(default = "high")]
    cb2_in: bool,
    /// CA2 output driver level (meaningful while CRA b5 = 1).
    #[serde(default = "high")]
    ca2_out: bool,
    /// CB2 output driver level (meaningful while CRB b5 = 1).
    #[serde(default = "high")]
    cb2_out: bool,
    /// Number of CA2 strobe pulses (read strobe mode), wrapping.
    #[serde(default)]
    ca2_strobes: u32,
    /// Number of CB2 strobe pulses (write strobe mode), wrapping.
    #[serde(default)]
    cb2_strobes: u32,
}

impl Default for BoardPia {
    fn default() -> Self {
        Self::new()
    }
}

impl BoardPia {
    pub fn new() -> Self {
        Self {
            ddra: 0,
            ddrb: 0,
            ora: 0,
            orb: 0,
            // Nothing connected: port A pull-ups, port B reads high.
            ira: 0xFF,
            irb: 0xFF,
            cra: 0,
            crb: 0,
            ca1: true,
            cb1: true,
            ca2_in: true,
            cb2_in: true,
            ca2_out: true,
            cb2_out: true,
            ca2_strobes: 0,
            cb2_strobes: 0,
        }
    }

    /// Hardware RESET: all registers cleared (both ports inputs, DDRs
    /// selected, interrupts disabled, flags cleared, C1/C2 inputs). The
    /// levels driven into the pins from outside are not affected.
    pub fn reset(&mut self) {
        self.ddra = 0;
        self.ddrb = 0;
        self.ora = 0;
        self.orb = 0;
        self.cra = 0;
        self.crb = 0;
        self.ca2_out = true;
        self.cb2_out = true;
        self.ca2_strobes = 0;
        self.cb2_strobes = 0;
    }

    /// CPU read (side effects: data reads clear the side's IRQ flags; a
    /// port A data read triggers the CA2 read strobe in strobe mode).
    pub fn read(&mut self, offset: u8) -> u8 {
        match offset & 3 {
            0 => {
                if self.cra & CR_DATA_SELECT == 0 {
                    return self.ddra;
                }
                let value = self.port_a_value();
                self.cra &= !(CR_IRQ1 | CR_IRQ2);
                if self.cra & C2_STROBE_MASK == CR_C2_OUTPUT {
                    // Read strobe: CA2 low after the read.
                    self.ca2_out = false;
                    self.ca2_strobes = self.ca2_strobes.wrapping_add(1);
                    if self.cra & CR_C2_BIT3 != 0 {
                        // E restore: high again one E cycle later.
                        self.ca2_out = true;
                    }
                }
                value
            }
            1 => self.cra,
            2 => {
                if self.crb & CR_DATA_SELECT == 0 {
                    return self.ddrb;
                }
                let value = self.port_b_value();
                self.crb &= !(CR_IRQ1 | CR_IRQ2);
                value
            }
            _ => self.crb,
        }
    }

    /// Side-effect-free register view (debugger): data reads do not clear
    /// flags and do not strobe CA2.
    pub fn peek(&self, offset: u8) -> u8 {
        match offset & 3 {
            0 => {
                if self.cra & CR_DATA_SELECT == 0 {
                    self.ddra
                } else {
                    self.port_a_value()
                }
            }
            1 => self.cra,
            2 => {
                if self.crb & CR_DATA_SELECT == 0 {
                    self.ddrb
                } else {
                    self.port_b_value()
                }
            }
            _ => self.crb,
        }
    }

    pub fn write(&mut self, offset: u8, value: u8) {
        match offset & 3 {
            0 => {
                if self.cra & CR_DATA_SELECT == 0 {
                    self.ddra = value;
                } else {
                    self.ora = value;
                }
            }
            1 => {
                self.cra = Self::write_cr(self.cra, value);
                self.ca2_out = Self::c2_level_after_cr_write(value);
            }
            2 => {
                if self.crb & CR_DATA_SELECT == 0 {
                    self.ddrb = value;
                } else {
                    self.orb = value;
                    if self.crb & C2_STROBE_MASK == CR_C2_OUTPUT {
                        // Write strobe: CB2 low after the write.
                        self.cb2_out = false;
                        self.cb2_strobes = self.cb2_strobes.wrapping_add(1);
                        if self.crb & CR_C2_BIT3 != 0 {
                            // E restore: high again one E cycle later.
                            self.cb2_out = true;
                        }
                    }
                }
            }
            _ => {
                self.crb = Self::write_cr(self.crb, value);
                self.cb2_out = Self::c2_level_after_cr_write(value);
            }
        }
    }

    /// New control register value: bits 6-7 are read-only flags; IRQ2 is
    /// cleared (and stays clear) while C2 is an output.
    fn write_cr(old: u8, value: u8) -> u8 {
        let mut cr = (old & (CR_IRQ1 | CR_IRQ2)) | (value & 0x3F);
        if cr & CR_C2_OUTPUT != 0 {
            cr &= !CR_IRQ2;
        }
        cr
    }

    /// C2 output driver level right after a control register write:
    /// manual mode drives bit 3, strobe mode idles high (a pending strobe
    /// is cancelled), input mode releases the pin (pulled high).
    fn c2_level_after_cr_write(value: u8) -> bool {
        if value & (CR_C2_OUTPUT | CR_C2_BIT4) == CR_C2_OUTPUT | CR_C2_BIT4 {
            value & CR_C2_BIT3 != 0
        } else {
            true
        }
    }

    /// Port A as read by the CPU: pin levels (the output latch where DDRA = 1,
    /// the pull-up otherwise) ANDed with whatever pulls the pins low.
    fn port_a_value(&self) -> u8 {
        self.port_a_read(self.ira)
    }

    /// Port B as read by the CPU: output latch where DDRB = 1, pin level where 0.
    fn port_b_value(&self) -> u8 {
        self.port_b_read(self.irb)
    }

    /// Port A data value for the external drive `ira` (pure).
    pub fn port_a_read(&self, ira: u8) -> u8 {
        (self.ora | !self.ddra) & ira
    }

    /// Port B data value for the external levels `irb` (pure).
    pub fn port_b_read(&self, irb: u8) -> u8 {
        (self.orb & self.ddrb) | (irb & !self.ddrb)
    }

    /// Side-effect-free register view with explicit external pin levels.
    pub fn peek_with_inputs(&self, offset: u8, ira: u8, irb: u8) -> u8 {
        match offset & 3 {
            0 if self.cra & CR_DATA_SELECT != 0 => self.port_a_read(ira),
            2 if self.crb & CR_DATA_SELECT != 0 => self.port_b_read(irb),
            other => self.peek(other),
        }
    }

    /// Levels the PIA drives onto port A (inputs float high via the pull-ups).
    pub fn port_a_pins(&self) -> u8 {
        self.ora | !self.ddra
    }

    /// Levels the PIA drives onto port B (inputs reported high).
    pub fn port_b_pins(&self) -> u8 {
        self.orb | !self.ddrb
    }

    /// Port A pins actively driven low by the PIA.
    pub fn port_a_driven_low(&self) -> u8 {
        self.ddra & !self.ora
    }

    /// Port B pins actively driven low by the PIA.
    pub fn port_b_driven_low(&self) -> u8 {
        self.ddrb & !self.orb
    }

    /// Port B pins actively driven high by the PIA.
    pub fn port_b_driven_high(&self) -> u8 {
        self.ddrb & self.orb
    }

    /// Port B output latch on output pins; three-state inputs read as 0
    /// (what a load without pull-ups such as the MC6847 mode inputs sees).
    pub fn port_b_outputs(&self) -> u8 {
        self.orb & self.ddrb
    }

    /// Set external port-A pin drive (1 = released, 0 = pulled low).
    pub fn set_ira(&mut self, value: u8) {
        self.ira = value;
    }

    /// Set external port-B pin levels.
    pub fn set_irb(&mut self, value: u8) {
        self.irb = value;
    }

    /// Drive the CA1 input; returns true when an active edge set IRQA1.
    pub fn set_ca1(&mut self, level: bool) -> bool {
        if level == self.ca1 {
            return false;
        }
        self.ca1 = level;
        if (self.cra & CR_C1_RISING != 0) != level {
            return false;
        }
        self.cra |= CR_IRQ1;
        if self.cra & (C2_STROBE_MASK | CR_C2_BIT3) == C2_STROBE_C1_RESTORE {
            // Read strobe with CA1 restore.
            self.ca2_out = true;
        }
        true
    }

    /// Drive the CB1 input; returns true when an active edge set IRQB1.
    pub fn set_cb1(&mut self, level: bool) -> bool {
        if level == self.cb1 {
            return false;
        }
        self.cb1 = level;
        if (self.crb & CR_C1_RISING != 0) != level {
            return false;
        }
        self.crb |= CR_IRQ1;
        if self.crb & (C2_STROBE_MASK | CR_C2_BIT3) == C2_STROBE_C1_RESTORE {
            // Write strobe with CB1 restore.
            self.cb2_out = true;
        }
        true
    }

    /// Drive the CA2 pin from outside; in C2 input mode an active edge sets
    /// IRQA2 (returns true). Ignored as an event while CA2 is an output.
    pub fn set_ca2(&mut self, level: bool) -> bool {
        if level == self.ca2_in {
            return false;
        }
        self.ca2_in = level;
        if self.cra & CR_C2_OUTPUT != 0 || (self.cra & CR_C2_BIT4 != 0) != level {
            return false;
        }
        self.cra |= CR_IRQ2;
        true
    }

    /// Drive the CB2 pin from outside (see [`Self::set_ca2`]).
    pub fn set_cb2(&mut self, level: bool) -> bool {
        if level == self.cb2_in {
            return false;
        }
        self.cb2_in = level;
        if self.crb & CR_C2_OUTPUT != 0 || (self.crb & CR_C2_BIT4 != 0) != level {
            return false;
        }
        self.crb |= CR_IRQ2;
        true
    }

    fn irq_level(cr: u8) -> bool {
        (cr & CR_IRQ1 != 0 && cr & CR_C1_IRQ_EN != 0)
            || (cr & CR_IRQ2 != 0 && cr & (CR_C2_OUTPUT | CR_C2_BIT3) == CR_C2_BIT3)
    }

    /// Level of the /IRQA output (true = asserted).
    pub fn irq_a(&self) -> bool {
        Self::irq_level(self.cra)
    }

    /// Level of the /IRQB output (true = asserted).
    pub fn irq_b(&self) -> bool {
        Self::irq_level(self.crb)
    }

    /// True when this PIA asserts /IRQA or /IRQB.
    pub fn irq_asserted(&self) -> bool {
        self.irq_a() || self.irq_b()
    }

    pub fn irqa1_flag(&self) -> bool {
        self.cra & CR_IRQ1 != 0
    }

    pub fn irqa2_flag(&self) -> bool {
        self.cra & CR_IRQ2 != 0
    }

    pub fn irqb1_flag(&self) -> bool {
        self.crb & CR_IRQ1 != 0
    }

    pub fn irqb2_flag(&self) -> bool {
        self.crb & CR_IRQ2 != 0
    }

    pub fn ca1_level(&self) -> bool {
        self.ca1
    }

    pub fn cb1_level(&self) -> bool {
        self.cb1
    }

    /// CA2 input level (as driven from outside).
    pub fn ca2_input(&self) -> bool {
        self.ca2_in
    }

    /// CB2 input level (as driven from outside).
    pub fn cb2_input(&self) -> bool {
        self.cb2_in
    }

    pub fn ca2_is_output(&self) -> bool {
        self.cra & CR_C2_OUTPUT != 0
    }

    pub fn cb2_is_output(&self) -> bool {
        self.crb & CR_C2_OUTPUT != 0
    }

    /// CA2 level driven by the PIA: the output driver level in output
    /// mode, released (pulled high) in input mode.
    pub fn ca2_output(&self) -> bool {
        !self.ca2_is_output() || self.ca2_out
    }

    /// CB2 level driven by the PIA (see [`Self::ca2_output`]).
    pub fn cb2_output(&self) -> bool {
        !self.cb2_is_output() || self.cb2_out
    }

    pub fn ca2_strobe_count(&self) -> u32 {
        self.ca2_strobes
    }

    pub fn cb2_strobe_count(&self) -> u32 {
        self.cb2_strobes
    }

    /// Set the C1/C2 input levels without generating edges (power-up wiring).
    pub fn preset_control_inputs(&mut self, ca1: bool, cb1: bool) {
        self.ca1 = ca1;
        self.cb1 = cb1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddr_select_and_data() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x00); // CRA: access DDRA
        pia.write(0, 0xF0);
        assert_eq!(pia.read(0), 0xF0);
        pia.write(1, 0x04); // CRA: access ORA
        pia.write(0, 0x0A);
        pia.set_ira(0x05);
        // outputs 0xF0 mask → high nibble from ORA (0), low from pins
        assert_eq!(pia.read(0) & 0x0F, 0x05);
    }

    #[test]
    fn cb1_falling_edge_sets_irq() {
        let mut pia = BoardPia::new();
        pia.write(3, 0x01); // enable CB1 IRQ, falling edge
        assert!(pia.set_cb1(false));
        assert!(pia.irq_asserted());
    }

    #[test]
    fn reset_clears_registers_but_keeps_pin_levels() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x3F);
        pia.write(0, 0x12);
        pia.set_ira(0x7F);
        pia.set_ca1(false);
        pia.reset();
        assert_eq!((pia.cra, pia.crb, pia.ddra, pia.ddrb, pia.ora, pia.orb), (0, 0, 0, 0, 0, 0));
        assert_eq!(pia.ira, 0x7F);
        assert!(!pia.ca1_level());
        assert!(!pia.irq_asserted());
    }

    #[test]
    fn port_a_reads_pin_levels_output_can_be_pulled_low() {
        let mut pia = BoardPia::new();
        pia.write(0, 0xFF); // DDRA all outputs
        pia.write(1, 0x04);
        pia.write(0, 0xFF); // drive all high
        pia.set_ira(0xFE); // something pulls PA0 low
        assert_eq!(pia.read(0), 0xFE);
    }

    #[test]
    fn port_b_reads_output_latch_for_output_bits() {
        let mut pia = BoardPia::new();
        pia.write(2, 0xF0); // DDRB high nibble outputs
        pia.write(3, 0x04);
        pia.write(2, 0xA5);
        pia.set_irb(0x0F & 0x03);
        // Output bits come from ORB even when the pins are pulled low.
        assert_eq!(pia.read(2), 0xA0 | 0x03);
    }

    #[test]
    fn data_read_clears_flags_ddr_read_does_not() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x00); // DDR selected, CA1 falling, IRQ disabled
        pia.set_ca1(false);
        assert_eq!(pia.read(1) & 0x80, 0x80);
        let _ = pia.read(0); // DDRA read
        assert_eq!(pia.read(1) & 0x80, 0x80, "DDR read must not clear IRQA1");
        pia.write(1, 0x04);
        assert_eq!(pia.read(1) & 0x80, 0x80);
        let _ = pia.read(0); // data read
        assert_eq!(pia.read(1) & 0x80, 0x00);
    }

    #[test]
    fn peek_is_side_effect_free() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x2C); // data, CA2 read strobe with E restore... (b5=1,b4=0,b3=1)
        pia.set_ca1(false);
        let before = pia.clone();
        let _ = pia.peek(0);
        let _ = pia.peek(1);
        assert_eq!(pia.cra, before.cra);
        assert_eq!(pia.ca2_strobe_count(), before.ca2_strobe_count());
    }

    #[test]
    fn c1_irq_gated_by_enable_and_level_follows_flag() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x04); // IRQ disabled, falling edge
        assert!(pia.set_ca1(false));
        assert!(!pia.irq_a(), "flag set but IRQ disabled");
        pia.write(1, 0x05); // enabling with the flag set asserts IRQ
        assert!(pia.irq_a());
        let _ = pia.read(0);
        assert!(!pia.irq_a(), "acknowledged by the data read");
        // Rising edge selected: falling transitions are ignored.
        pia.write(1, 0x07);
        pia.set_ca1(true);
        assert!(pia.irq_a());
    }

    #[test]
    fn ca2_input_edges_set_irq2_only_in_input_mode() {
        let mut pia = BoardPia::new();
        // Input mode, falling edge, IRQ2 enabled.
        pia.write(1, 0x0C);
        assert!(pia.set_ca2(false));
        assert_eq!(pia.read(1) & 0x40, 0x40);
        assert!(pia.irq_a());
        let _ = pia.read(0);
        assert!(!pia.irq_a());
        // Rising edge select.
        pia.write(1, 0x1C);
        assert!(pia.set_ca2(true));
        assert!(pia.irq_a());
        let _ = pia.read(0);
        // IRQ2 disabled (b3 = 0): flag set, no IRQ.
        pia.write(1, 0x14);
        pia.set_ca2(false);
        assert!(pia.set_ca2(true));
        assert_eq!(pia.read(1) & 0x40, 0x40);
        assert!(!pia.irq_a());
    }

    #[test]
    fn c2_output_mode_bit3_is_level_not_irq_enable() {
        let mut pia = BoardPia::new();
        pia.write(3, 0x0C); // CB2 input, IRQ2 enabled
        pia.set_cb2(false);
        assert!(pia.irq_b());
        // Switching to manual output with b3 = 1 clears IRQB2, drives CB2 high,
        // and b3 no longer acts as an IRQ enable.
        pia.write(3, 0x3C);
        assert!(!pia.irq_b());
        assert_eq!(pia.read(3) & 0x40, 0);
        assert!(pia.cb2_is_output());
        assert!(pia.cb2_output());
        pia.set_cb2(true);
        pia.set_cb2(false);
        assert!(!pia.irq_b(), "C2 edges ignored in output mode");
        pia.write(3, 0x34);
        assert!(!pia.cb2_output(), "manual output low");
        pia.write(1, 0x3C);
        assert!(pia.ca2_output());
        pia.write(1, 0x34);
        assert!(!pia.ca2_output());
    }

    #[test]
    fn ca2_read_strobe_with_ca1_restore() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x24); // data select, CA2 strobe, CA1 restore, CA1 falling
        assert!(pia.ca2_output());
        let _ = pia.read(0);
        assert!(!pia.ca2_output(), "low after port A read");
        assert_eq!(pia.ca2_strobe_count(), 1);
        pia.set_ca1(true); // inactive edge
        assert!(!pia.ca2_output());
        pia.set_ca1(false); // active edge restores CA2
        assert!(pia.ca2_output());
        // Port B reads never strobe CA2.
        pia.write(3, 0x04);
        let _ = pia.read(2);
        assert!(pia.ca2_output());
    }

    #[test]
    fn ca2_read_strobe_with_e_restore_is_one_cycle_pulse() {
        let mut pia = BoardPia::new();
        pia.write(1, 0x2C);
        let _ = pia.read(0);
        assert!(pia.ca2_output(), "back high after one E cycle");
        assert_eq!(pia.ca2_strobe_count(), 1);
    }

    #[test]
    fn cb2_write_strobe_with_cb1_restore() {
        let mut pia = BoardPia::new();
        pia.write(2, 0xFF);
        pia.write(3, 0x26); // data, CB2 strobe, CB1 restore, CB1 rising
        assert!(pia.cb2_output());
        let _ = pia.read(2);
        assert!(pia.cb2_output(), "reads do not strobe CB2");
        pia.write(2, 0x55);
        assert!(!pia.cb2_output(), "low after port B write");
        pia.set_cb1(false); // inactive (falling) edge
        assert!(!pia.cb2_output());
        pia.set_cb1(true); // active rising edge restores
        assert!(pia.cb2_output());
        assert_eq!(pia.cb2_strobe_count(), 1);
        // E-restore variant.
        pia.write(3, 0x2E);
        pia.write(2, 0xAA);
        assert!(pia.cb2_output());
        assert_eq!(pia.cb2_strobe_count(), 2);
    }

    #[test]
    fn cr_write_keeps_flags_read_only() {
        let mut pia = BoardPia::new();
        pia.write(3, 0xC0); // attempt to set flags
        assert_eq!(pia.read(3), 0x00);
        pia.set_cb1(false);
        pia.write(3, 0x05);
        assert_eq!(pia.read(3), 0x85);
    }
}
