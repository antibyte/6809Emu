//! Minimal 6821 PIA chip model for CoCo / Dragon board I/O.
//! Supports DDR vs data select, external inputs, and CA1/CB1 IRQ edges.

use serde::{Deserialize, Serialize};

const CR_DDR_SELECT: u8 = 0x04;
const CR_C1_IRQ_EN: u8 = 0x01;
const CR_C1_EDGE: u8 = 0x02; // 0 = falling (high→low), 1 = rising
const CR_IRQ1: u8 = 0x80;
const CR_IRQ2: u8 = 0x40;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoardPia {
    pub ddra: u8,
    pub ddrb: u8,
    pub ora: u8,
    pub orb: u8,
    /// External input levels for port A (1 = high).
    pub ira: u8,
    /// External input levels for port B (1 = high).
    pub irb: u8,
    pub cra: u8,
    pub crb: u8,
    ca1: bool,
    cb1: bool,
}

impl BoardPia {
    pub fn new() -> Self {
        Self {
            // Pull-ups on inputs default high (no keys, no joystick).
            ira: 0xFF,
            irb: 0xFF,
            ..Default::default()
        }
    }

    pub fn read(&mut self, offset: u8) -> u8 {
        match offset & 3 {
            0 => {
                if self.cra & CR_DDR_SELECT == 0 {
                    self.ddra
                } else {
                    // Reading peripheral register clears IRQA flags.
                    self.cra &= !(CR_IRQ1 | CR_IRQ2);
                    port_read(self.ora, self.ira, self.ddra)
                }
            }
            1 => self.cra,
            2 => {
                if self.crb & CR_DDR_SELECT == 0 {
                    self.ddrb
                } else {
                    self.crb &= !(CR_IRQ1 | CR_IRQ2);
                    port_read(self.orb, self.irb, self.ddrb)
                }
            }
            3 => self.crb,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, offset: u8, value: u8) {
        match offset & 3 {
            0 => {
                if self.cra & CR_DDR_SELECT == 0 {
                    self.ddra = value;
                } else {
                    self.ora = value;
                }
            }
            1 => {
                // Bits 6–7 are read-only IRQ flags.
                self.cra = (self.cra & (CR_IRQ1 | CR_IRQ2)) | (value & 0x3F);
            }
            2 => {
                if self.crb & CR_DDR_SELECT == 0 {
                    self.ddrb = value;
                } else {
                    self.orb = value;
                }
            }
            3 => {
                self.crb = (self.crb & (CR_IRQ1 | CR_IRQ2)) | (value & 0x3F);
            }
            _ => {}
        }
    }

    /// Output latch value on port B (keyboard column drive on CoCo).
    pub fn output_b(&self) -> u8 {
        (self.orb & self.ddrb) | (!self.ddrb)
    }

    /// Set external port-A input bits (keyboard rows, etc.).
    pub fn set_ira(&mut self, value: u8) {
        self.ira = value;
    }

    /// Set external port-B input bits.
    pub fn set_irb(&mut self, value: u8) {
        self.irb = value;
    }

    /// Drive CA1 control line; returns true if a new IRQ edge was latched.
    pub fn set_ca1(&mut self, level: bool) -> bool {
        let edge = edge_active(self.ca1, level, self.cra);
        self.ca1 = level;
        if edge {
            self.cra |= CR_IRQ1;
            return self.cra & CR_C1_IRQ_EN != 0;
        }
        false
    }

    /// Drive CB1 control line; returns true if a new IRQ edge was latched.
    pub fn set_cb1(&mut self, level: bool) -> bool {
        let edge = edge_active(self.cb1, level, self.crb);
        self.cb1 = level;
        if edge {
            self.crb |= CR_IRQ1;
            return self.crb & CR_C1_IRQ_EN != 0;
        }
        false
    }

    /// True when this PIA asserts /IRQ (active low on real hardware).
    pub fn irq_asserted(&self) -> bool {
        let a = (self.cra & CR_IRQ1 != 0 && self.cra & CR_C1_IRQ_EN != 0)
            || (self.cra & CR_IRQ2 != 0 && self.cra & 0x08 != 0);
        let b = (self.crb & CR_IRQ1 != 0 && self.crb & CR_C1_IRQ_EN != 0)
            || (self.crb & CR_IRQ2 != 0 && self.crb & 0x08 != 0);
        a || b
    }
}

fn port_read(or_reg: u8, ir_reg: u8, ddr: u8) -> u8 {
    (or_reg & ddr) | (ir_reg & !ddr)
}

fn edge_active(prev: bool, now: bool, cr: u8) -> bool {
    if prev == now {
        return false;
    }
    let rising = cr & CR_C1_EDGE != 0;
    if rising {
        !prev && now
    } else {
        prev && !now
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
        // outputs 0xF0 mask → high nibble from ORA (0), low from IRA
        assert_eq!(pia.read(0) & 0x0F, 0x05);
    }

    #[test]
    fn cb1_falling_edge_sets_irq() {
        let mut pia = BoardPia::new();
        pia.write(3, 0x01); // enable CB1 IRQ, falling edge
        pia.cb1 = true;
        assert!(pia.set_cb1(false));
        assert!(pia.irq_asserted());
    }
}
