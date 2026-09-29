//! Regression tests for the MC6809 core findings of the emulation audit
//! (MC6809 datasheet, MAME m6809, XRoar).

use std::cell::Cell;

use m6809_core::{
    CpuVariant, Emulator, Flags, IoRegisterView, IoWriteResult, MemoryIo, Trap,
};

fn emu_at(prog: &[u8]) -> Emulator {
    let mut emu = Emulator::new();
    emu.load_and_reset(0x0100, prog, 0x0100).unwrap();
    emu.cpu.s = 0x1000;
    emu.cpu.lds_encountered = true;
    emu
}

fn cycles(prog: &[u8], setup: impl FnOnce(&mut Emulator)) -> u32 {
    let mut emu = emu_at(prog);
    setup(&mut emu);
    emu.step().cycles
}

/// Minimal device: `$E000` status read acknowledges the interrupt (like a PIA
/// data-register read), `$E001` write raises it; counts bus reads of `$E002`.
#[derive(Debug, Default)]
struct IrqDevice {
    asserted: Cell<bool>,
    reads_e002: Cell<u32>,
    firq: bool,
}

impl MemoryIo for IrqDevice {
    fn kind_id(&self) -> &str {
        "irq-test"
    }
    fn read(&self, addr: u16, _ram: &[u8; 0x10000]) -> Option<u8> {
        match addr {
            0xE000 => {
                let v = u8::from(self.asserted.get());
                self.asserted.set(false);
                Some(v)
            }
            0xE002 => {
                self.reads_e002.set(self.reads_e002.get() + 1);
                Some(0x55)
            }
            _ => None,
        }
    }
    fn peek(&self, addr: u16, _ram: &[u8; 0x10000]) -> Option<u8> {
        match addr {
            0xE000 => Some(u8::from(self.asserted.get())),
            0xE002 => Some(0x55),
            _ => None,
        }
    }
    fn write(&mut self, addr: u16, _value: u8, _ram: &mut [u8; 0x10000]) -> IoWriteResult {
        match addr {
            0xE001 => {
                self.asserted.set(true);
                IoWriteResult::Consumed
            }
            0xE002 => IoWriteResult::Consumed,
            _ => IoWriteResult::PassThrough,
        }
    }
    fn clone_box(&self) -> Box<dyn MemoryIo> {
        Box::new(IrqDevice {
            asserted: Cell::new(self.asserted.get()),
            reads_e002: Cell::new(self.reads_e002.get()),
            firq: self.firq,
        })
    }
    fn snapshot(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn restore(&mut self, _snapshot: &serde_json::Value) {}
    fn io_registers(&self) -> Vec<IoRegisterView> {
        Vec::new()
    }
    fn poll_irq(&mut self) -> bool {
        !self.firq && self.asserted.get()
    }
    fn poll_firq(&mut self) -> bool {
        self.firq && self.asserted.get()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn device(emu: &Emulator) -> &IrqDevice {
    emu.memory
        .io
        .as_ref()
        .unwrap()
        .as_any()
        .downcast_ref::<IrqDevice>()
        .unwrap()
}

// ── Interrupt entry / masks ──────────────────────────────────────────

#[test]
fn swi2_and_swi3_leave_interrupt_masks_unchanged() {
    for (prog, vector) in [([0x10u8, 0x3F], 0xFFF4u16), ([0x11, 0x3F], 0xFFF2)] {
        let mut emu = emu_at(&prog);
        emu.memory.write16(vector, 0x0300);
        emu.cpu.cc = Flags::empty();
        let step = emu.step();
        assert_eq!(emu.cpu.pc, 0x0300);
        assert_eq!(step.cycles, 20);
        assert!(!emu.cpu.cc.contains(Flags::I));
        assert!(!emu.cpu.cc.contains(Flags::F));
        assert!(emu.cpu.cc.contains(Flags::E));
    }
}

#[test]
fn swi_sets_both_masks_and_only_traps_without_vector() {
    let mut emu = emu_at(&[0x3F]);
    emu.memory.write16(0xFFFA, 0x0300);
    emu.cpu.cc = Flags::empty();
    let step = emu.step();
    assert!(emu.cpu.cc.contains(Flags::I | Flags::F));
    assert!(step.trap.is_none(), "a real SWI handler must not stop a run");

    let mut emu = emu_at(&[0x3F]);
    let step = emu.step();
    assert_eq!(step.trap, Some(Trap::Swi), "SWI through an empty vector stops");
}

#[test]
fn irq_entry_sets_i_but_not_f() {
    let mut emu = emu_at(&[0x12]);
    emu.memory.write16(0xFFF8, 0x0300);
    emu.cpu.cc = Flags::empty();
    emu.trigger_irq();
    let step = emu.step();
    assert_eq!(step.mnemonic, "IRQ");
    assert_eq!(step.cycles, 19);
    assert!(emu.cpu.cc.contains(Flags::I));
    assert!(!emu.cpu.cc.contains(Flags::F), "FIRQ must stay possible inside an IRQ handler");
}

#[test]
fn firq_after_rti_from_irq_stacks_cc_with_e_clear() {
    // IRQ handler $0300: RTI; FIRQ handler $0400: RTI.
    let mut prog = vec![0x1C, 0xAF]; // ANDCC #$AF
    prog.extend(std::iter::repeat_n(0x12, 8));
    let mut emu = emu_at(&prog);
    emu.memory.write8(0x0300, 0x3B);
    emu.memory.write8(0x0400, 0x3B);
    emu.memory.write16(0xFFF8, 0x0300);
    emu.memory.write16(0xFFF6, 0x0400);
    emu.cpu.x = 0x1111;
    emu.step(); // ANDCC
    emu.trigger_irq();
    emu.step(); // IRQ
    emu.step(); // RTI (CC restored with E=1)
    assert!(emu.cpu.cc.contains(Flags::E));
    emu.step(); // NOP
    let (pc, s) = (emu.cpu.pc, emu.cpu.s);
    emu.trigger_firq();
    let entry = emu.step();
    assert_eq!(entry.cycles, 10, "fast FIRQ entry");
    assert_eq!(emu.memory.read8(emu.cpu.s) & 0x80, 0, "stacked CC must have E=0");
    emu.step(); // RTI
    assert_eq!(emu.cpu.pc, pc);
    assert_eq!(emu.cpu.s, s);
    assert_eq!(emu.cpu.x, 0x1111);
}

// ── Level-sensitive hardware interrupt lines ──────────────────────────

fn install_device(emu: &mut Emulator, firq: bool) {
    emu.memory.io = Some(Box::new(IrqDevice {
        firq,
        ..Default::default()
    }));
}

#[test]
fn acknowledged_irq_is_taken_exactly_once() {
    // Main: ANDCC #$EF ; STA $E001 (raise) ; NOP ... Handler: INC $0600 ; LDA $E000 (ack) ; RTI
    let mut prog = vec![0x1C, 0xEF, 0xB7, 0xE0, 0x01];
    prog.extend(std::iter::repeat_n(0x12, 20));
    let mut emu = emu_at(&prog);
    install_device(&mut emu, false);
    for (i, b) in [0x7C, 0x06, 0x00, 0xB6, 0xE0, 0x00, 0x3B].iter().enumerate() {
        emu.memory.write8(0x0300 + i as u16, *b);
    }
    emu.memory.write16(0xFFF8, 0x0300);
    for _ in 0..30 {
        emu.step();
    }
    assert_eq!(emu.memory.read8(0x0600), 1, "one IRQ per device event");
}

#[test]
fn irq_released_while_masked_is_not_taken_later() {
    // STA $E001 (raise, I set) ; LDA $E000 (poll + acknowledge) ; ANDCC #$EF ; NOP ; NOP
    let mut emu = emu_at(&[0xB7, 0xE0, 0x01, 0xB6, 0xE0, 0x00, 0x1C, 0xEF, 0x12, 0x12]);
    install_device(&mut emu, false);
    emu.memory.write16(0xFFF8, 0x0300);
    for _ in 0..5 {
        let step = emu.step();
        assert_ne!(step.mnemonic, "IRQ", "no stale IRQ after the device released the line");
    }
}

#[test]
fn unacknowledged_irq_line_reenters_after_rti() {
    // Handler does not acknowledge: level-sensitive IRQ is taken again after RTI.
    let mut prog = vec![0x1C, 0xEF, 0xB7, 0xE0, 0x01];
    prog.extend(std::iter::repeat_n(0x12, 20));
    let mut emu = emu_at(&prog);
    install_device(&mut emu, false);
    emu.memory.write8(0x0300, 0x3B);
    emu.memory.write16(0xFFF8, 0x0300);
    let irqs = (0..20).filter(|_| emu.step().mnemonic == "IRQ").count();
    assert!(irqs >= 3, "line still asserted → IRQ repeats ({irqs})");
}

#[test]
fn device_firq_line_is_level_sensitive() {
    let mut prog = vec![0x1C, 0xBF, 0xB7, 0xE0, 0x01];
    prog.extend(std::iter::repeat_n(0x12, 20));
    let mut emu = emu_at(&prog);
    install_device(&mut emu, true);
    for (i, b) in [0x7C, 0x06, 0x00, 0xB6, 0xE0, 0x00, 0x3B].iter().enumerate() {
        emu.memory.write8(0x0300 + i as u16, *b);
    }
    emu.memory.write16(0xFFF6, 0x0300);
    for _ in 0..30 {
        emu.step();
    }
    assert_eq!(emu.memory.read8(0x0600), 1);
}

#[test]
fn sync_is_released_by_a_device_line_while_time_runs() {
    // ORCC #$50 ; SYNC ; LDA #$42 — the device raises its line after a while.
    let mut emu = emu_at(&[0x1A, 0x50, 0x13, 0x86, 0x42]);
    install_device(&mut emu, false);
    emu.step();
    emu.step(); // SYNC
    let before = emu.cpu.total_cycles;
    for _ in 0..50 {
        emu.step();
    }
    assert!(emu.cpu.total_cycles >= before + 200, "waiting consumes time");
    assert_eq!(emu.cpu.a, 0);
    emu.memory.write8(0xE001, 0); // raise (masked) IRQ
    emu.step(); // line sampled after this step
    emu.step(); // SYNC released → next instruction
    assert_eq!(emu.cpu.a, 0x42);
    assert_eq!(emu.run(0), 0);
}

#[test]
fn run_keeps_going_while_cpu_waits() {
    let mut emu = emu_at(&[0x13]);
    let consumed = emu.run(1000);
    assert!(consumed >= 1000, "Emulator::run must not stop in SYNC ({consumed})");
}

// ── Cycle counts ─────────────────────────────────────────────────────

#[test]
fn datasheet_cycle_counts() {
    let x = |e: &mut Emulator| e.cpu.x = 0x2000;
    assert_eq!(cycles(&[0x26, 0x05], |e| e.cpu.cc.insert(Flags::Z)), 3, "BNE not taken");
    assert_eq!(cycles(&[0x21, 0x05], |_| {}), 3, "BRN");
    assert_eq!(cycles(&[0x7E, 0x01, 0x00], |_| {}), 4, "JMP ext");
    assert_eq!(cycles(&[0x1D], |_| {}), 2, "SEX");
    assert_eq!(cycles(&[0x13], |e| e.cpu.irq_pending = true), 4, "SYNC with line asserted");
    assert_eq!(cycles(&[0xA6, 0x01], x), 5, "LDA 1,X");
    assert_eq!(cycles(&[0xA6, 0x84], x), 4, "LDA ,X");
    assert_eq!(cycles(&[0xA6, 0x88, 0x10], x), 5, "LDA n8,X");
    assert_eq!(cycles(&[0xA6, 0x89, 0x10, 0x00], x), 8, "LDA n16,X");
    assert_eq!(cycles(&[0xA6, 0x8B], x), 8, "LDA D,X");
    assert_eq!(cycles(&[0xA6, 0x8C, 0x10], |_| {}), 5, "LDA n8,PCR");
    assert_eq!(cycles(&[0xA6, 0x8D, 0x10, 0x00], |_| {}), 9, "LDA n16,PCR");
    assert_eq!(cycles(&[0xA6, 0x94], x), 7, "LDA [,X]");
    assert_eq!(cycles(&[0xA6, 0x99, 0x10, 0x00], x), 11, "LDA [n16,X]");
    assert_eq!(cycles(&[0xA6, 0x9B], x), 11, "LDA [D,X]");
    assert_eq!(cycles(&[0xA6, 0x9F, 0x20, 0x00], |_| {}), 9, "LDA [n16]");
    assert_eq!(cycles(&[0x30, 0x01], |_| {}), 5, "LEAX 1,X");
    assert_eq!(cycles(&[0x10, 0x10, 0x8E, 0x12, 0x34], |_| {}), 5, "LDY # after a repeated prefix");
    assert_eq!(cycles(&[0x10, 0x86, 0x42], |_| {}), 3, "undefined page-2 opcode runs as page 1 + 1");
}

// ── Flags ────────────────────────────────────────────────────────────

#[test]
fn mul_only_affects_z_and_c() {
    let mut emu = emu_at(&[0x3D]);
    emu.cpu.a = 1;
    emu.cpu.b = 1;
    emu.cpu.cc = Flags::N | Flags::V | Flags::H;
    emu.step();
    assert!(emu.cpu.cc.contains(Flags::N | Flags::V | Flags::H));
    assert!(!emu.cpu.cc.contains(Flags::Z));
    assert!(!emu.cpu.cc.contains(Flags::C));
}

#[test]
fn daa_and_sub_leave_h_alone() {
    let mut emu = emu_at(&[0x19]);
    emu.cpu.a = 0x12;
    emu.cpu.cc = Flags::H;
    emu.step();
    assert!(emu.cpu.cc.contains(Flags::H));
    assert_eq!(emu.cpu.a, 0x18);

    let mut emu = emu_at(&[0x80, 0x01]);
    emu.cpu.a = 0x10;
    emu.cpu.cc = Flags::empty();
    emu.step();
    assert!(!emu.cpu.cc.contains(Flags::H));
}

// ── Bus behaviour ────────────────────────────────────────────────────

#[test]
fn clr_reads_before_writing() {
    let mut emu = emu_at(&[0x7F, 0xE0, 0x02]); // CLR $E002
    install_device(&mut emu, false);
    emu.step();
    assert_eq!(device(&emu).reads_e002.get(), 1, "CLR is read-modify-write on the 6809");
}

#[test]
fn watchpoint_fires_on_io_register_writes() {
    let mut emu = emu_at(&[0xB7, 0xE0, 0x02]); // STA $E002 (device register)
    install_device(&mut emu, false);
    emu.set_watchpoint(0xE002);
    let step = emu.step();
    assert_eq!(step.trap, Some(Trap::Watchpoint));
}

#[test]
fn peek_has_no_side_effects() {
    let mut emu = emu_at(&[0x12]);
    install_device(&mut emu, false);
    emu.memory.write8(0xE001, 0);
    assert_eq!(emu.memory.peek8(0xE000), 1);
    assert_eq!(emu.memory.peek8(0xE000), 1, "peek must not acknowledge");
    assert_eq!(emu.memory.read8(0xE000), 1);
    assert_eq!(emu.memory.peek8(0xE000), 0, "bus read acknowledged");
}

// ── Trace / disassembly bytes ────────────────────────────────────────

#[test]
fn indexed_offsets_are_part_of_the_instruction_bytes() {
    let mut emu = emu_at(&[0xA6, 0x88, 0x10]);
    emu.cpu.x = 0x2000;
    let step = emu.step();
    assert_eq!(step.bytes, vec![0xA6, 0x88, 0x10]);
    assert_eq!(step.operands, "16,X");

    let mut emu = emu_at(&[0xA6, 0x89, 0x12, 0x34]);
    let step = emu.step();
    assert_eq!(step.bytes, vec![0xA6, 0x89, 0x12, 0x34]);
    assert_eq!(step.operands, "4660,X");

    let mut emu = emu_at(&[0xAE, 0x9F, 0x12, 0x34]);
    let step = emu.step();
    assert_eq!(step.bytes.len(), 4);
    assert_eq!(step.operands, "[$1234]");

    // PCR operands show the target address (what the assembler expects back).
    let mut emu = emu_at(&[0x30, 0x8C, 0x10]);
    let step = emu.step();
    assert_eq!(step.operands, "$0113,PCR");
    assert_eq!(emu.cpu.x, 0x0113);
}

// ── HCF ──────────────────────────────────────────────────────────────

#[test]
fn hcf_ignores_interrupts_until_reset() {
    let mut emu = emu_at(&[0x14]);
    emu.memory.write16(0xFFFC, 0x0300);
    emu.step();
    emu.trigger_nmi();
    for _ in 0..10 {
        let step = emu.step();
        assert_eq!(step.mnemonic, "HCF");
    }
    emu.reset();
    assert!(!emu.cpu.free_run);
}

// ── HD6309 illegal indexed postbyte ──────────────────────────────────

#[test]
fn hd6309_illegal_postbyte_aborts_instruction_and_traps() {
    let mut emu = Emulator::new();
    emu.set_variant(CpuVariant::Hd6309);
    // STA [,-X] ($A7 $92): [,-R] is not a valid 6309 indexed mode.
    emu.load_and_reset(0x0100, &[0xA7, 0x92, 0x12], 0x0100).unwrap();
    emu.memory.write16(0xFFF0, 0x0300);
    emu.cpu.s = 0x1000;
    emu.cpu.a = 0x5A;
    emu.cpu.x = 0x2002;
    emu.memory.write16(0x2000, 0x4000); // pointer a [,-X] would have used
    let step = emu.step();
    assert_eq!(step.trap, Some(Trap::IllegalOpcode));
    assert_eq!(emu.cpu.pc, 0x0300);
    assert_ne!(emu.cpu.mode_reg & 0x40, 0, "MD bit 6 = illegal instruction");
    assert_eq!(emu.cpu.x, 0x2002, "no auto-decrement of the aborted instruction");
    assert_eq!(emu.memory.read8(0x4000), 0x00, "no store of the aborted instruction");
    assert_eq!(emu.memory.read16(0x1000 - 2), 0x0102, "stacked PC is past the postbyte");
    // Writes work again afterwards (the inhibit is released).
    emu.memory.write8(0x4000, 0x77);
    assert_eq!(emu.memory.read8(0x4000), 0x77);

    // The same bytes on an MC6809 are a (legal-looking) indirect store, no trap.
    let mut emu = Emulator::new();
    emu.load_and_reset(0x0100, &[0xA7, 0x92], 0x0100).unwrap();
    let step = emu.step();
    assert_ne!(step.trap, Some(Trap::IllegalOpcode));
}
