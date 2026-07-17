//! TRS-80 Color Computer 2 board: dual 6821 PIA, SAM, keyboard, VSYNC IRQ.

use std::cell::RefCell;

use crate::board_pia::BoardPia;
use crate::keyboard::KeyboardMatrix;
use crate::sam::Sam;
use m6809_core::{IoRegisterView, IoWriteResult, MemoryIo};
use serde::{Deserialize, Serialize};

/// Approx. CoCo E-clock cycles per NTSC field (~60 Hz at 0.89 MHz).
const CYCLES_PER_FRAME: u32 = 14_940;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Coco2Inner {
    pia0: BoardPia,
    pia1: BoardPia,
    sam: Sam,
    keyboard: KeyboardMatrix,
    cycle_acc: u32,
    irq_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coco2Machine {
    inner: RefCell<Coco2Inner>,
}

impl Default for Coco2Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Coco2Machine {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Coco2Inner {
                pia0: BoardPia::new(),
                pia1: BoardPia::new(),
                sam: Sam::with_coco_default(),
                keyboard: KeyboardMatrix::default(),
                cycle_acc: 0,
                irq_pending: false,
            }),
        }
    }

    /// PIA0 is partially decoded and mirrored across `$FF00–$FF1F`.
    fn is_pia0(addr: u16) -> bool {
        (0xFF00..=0xFF1F).contains(&addr)
    }

    /// PIA1 is partially decoded and mirrored across `$FF20–$FF3F`.
    fn is_pia1(addr: u16) -> bool {
        (0xFF20..=0xFF3F).contains(&addr)
    }

    /// ROM region: Extended BASIC + Color BASIC + cart ($8000–$FEFF).
    fn is_rom(addr: u16) -> bool {
        (0x8000..=0xFEFF).contains(&addr)
    }

    fn sync_keyboard(inner: &mut Coco2Inner) {
        let cols = inner.pia0.output_b();
        let rows = inner.keyboard.read_rows(cols);
        inner.pia0.set_ira(rows);
        inner.pia0.set_irb(0xFF);
    }

    pub fn host_key(&mut self, code: &str, down: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.keyboard.host_key(code, down);
        Self::sync_keyboard(&mut inner);
    }

    pub fn clear_keys(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.keyboard.clear();
        Self::sync_keyboard(&mut inner);
    }

    pub fn board_tick(&mut self, cycles: u32) {
        let mut inner = self.inner.borrow_mut();
        inner.cycle_acc = inner.cycle_acc.saturating_add(cycles);
        while inner.cycle_acc >= CYCLES_PER_FRAME {
            inner.cycle_acc -= CYCLES_PER_FRAME;
            if inner.pia0.set_cb1(false) {
                inner.irq_pending = true;
            }
            if inner.pia0.set_cb1(true) {
                inner.irq_pending = true;
            }
            let _ = inner.pia0.set_ca1(false);
            let _ = inner.pia0.set_ca1(true);
        }
        if inner.pia0.irq_asserted() || inner.pia1.irq_asserted() {
            inner.irq_pending = true;
        }
    }

    pub fn board_poll_irq(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let pending =
            inner.irq_pending || inner.pia0.irq_asserted() || inner.pia1.irq_asserted();
        inner.irq_pending = false;
        pending
    }
}

impl MemoryIo for Coco2Machine {
    fn kind_id(&self) -> &str {
        "coco2"
    }

    fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        let mut inner = self.inner.borrow_mut();
        if Self::is_pia0(addr) {
            Self::sync_keyboard(&mut inner);
            return Some(inner.pia0.read((addr & 3) as u8));
        }
        if Self::is_pia1(addr) {
            return Some(inner.pia1.read((addr & 3) as u8));
        }
        if inner.sam.is_mapped(addr) {
            return Some(inner.sam.read(addr));
        }
        if Self::is_rom(addr) {
            return Some(ram[addr as usize]);
        }
        None
    }

    fn write(&mut self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        let _ = ram;
        let mut inner = self.inner.borrow_mut();
        if Self::is_pia0(addr) {
            inner.pia0.write((addr & 3) as u8, value);
            Self::sync_keyboard(&mut inner);
            return IoWriteResult::Consumed;
        }
        if Self::is_pia1(addr) {
            inner.pia1.write((addr & 3) as u8, value);
            return IoWriteResult::Consumed;
        }
        if inner.sam.is_mapped(addr) {
            inner.sam.write(addr);
            return IoWriteResult::Consumed;
        }
        if Self::is_rom(addr) {
            return IoWriteResult::Ignored;
        }
        IoWriteResult::PassThrough
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn io_registers(&self) -> Vec<IoRegisterView> {
        let inner = self.inner.borrow();
        vec![
            IoRegisterView {
                address: 0xFF00,
                name: "PIA0 ORA/IRA (Kbd Rows)".into(),
                value: {
                    let cols = inner.pia0.output_b();
                    let rows = inner.keyboard.read_rows(cols);
                    if inner.pia0.cra & 0x04 == 0 {
                        inner.pia0.ddra
                    } else {
                        (inner.pia0.ora & inner.pia0.ddra) | (rows & !inner.pia0.ddra)
                    }
                },
            },
            IoRegisterView {
                address: 0xFF01,
                name: "PIA0 CRA".into(),
                value: inner.pia0.cra,
            },
            IoRegisterView {
                address: 0xFF02,
                name: "PIA0 ORB (Kbd Cols)".into(),
                value: inner.pia0.orb,
            },
            IoRegisterView {
                address: 0xFF03,
                name: "PIA0 CRB".into(),
                value: inner.pia0.crb,
            },
            IoRegisterView {
                address: 0xFF20,
                name: "PIA1 ORA".into(),
                value: inner.pia1.ora,
            },
            IoRegisterView {
                address: 0xFF21,
                name: "PIA1 CRA".into(),
                value: inner.pia1.cra,
            },
            IoRegisterView {
                address: 0xFF22,
                name: "PIA1 ORB (VDG)".into(),
                value: inner.pia1.orb,
            },
            IoRegisterView {
                address: 0xFF23,
                name: "PIA1 CRB".into(),
                value: inner.pia1.crb,
            },
            IoRegisterView {
                address: 0xFFC0,
                name: "SAM V0".into(),
                value: (inner.sam.bits() & 0x01) as u8,
            },
        ]
    }

    fn tick(&mut self, cycles: u32) {
        self.board_tick(cycles);
    }

    fn poll_irq(&mut self) -> bool {
        self.board_poll_irq()
    }
}
