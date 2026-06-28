use m6809_core::{Emulator, Flags};

fn run_program(program: &[u8]) -> Emulator {
    let mut emu = Emulator::new();
    emu.load_and_reset(0x0100, program, 0x0100).unwrap();
    emu
}

// ── ALU8 flags ──────────────────────────────────────────────────────

#[test]
fn mame_adda_overflow() {
    let mut emu = run_program(&[0x86, 0x7F, 0x8B, 0x01]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x80);
    assert!(emu.cpu.cc.contains(Flags::V));
}

#[test]
fn mame_adda_carry() {
    let mut emu = run_program(&[0x86, 0xFF, 0x8B, 0x01]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x00);
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_adda_zero_result() {
    let mut emu = run_program(&[0x86, 0x80, 0x8B, 0x80]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x00);
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_cmpa_equal() {
    let mut emu = run_program(&[0x86, 0x42, 0x81, 0x42]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x42);
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_cmpa_less_than() {
    let mut emu = run_program(&[0x86, 0x42, 0x81, 0x43]);
    emu.step(); emu.step();
    assert!(emu.cpu.cc.contains(Flags::N));
    assert!(!emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_cmpa_greater_than() {
    let mut emu = run_program(&[0x86, 0x43, 0x81, 0x42]);
    emu.step(); emu.step();
    assert!(!emu.cpu.cc.contains(Flags::N));
    assert!(!emu.cpu.cc.contains(Flags::Z));
}

// ── MUL ─────────────────────────────────────────────────────────────

#[test]
fn mame_mul_no_carry() {
    let mut emu = run_program(&[0x86, 0x07, 0xC6, 0x08, 0x3D]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.b, 0x38);
    assert_eq!(emu.cpu.a, 0x00);
    assert!(!emu.cpu.cc.contains(Flags::C));
}

// ── INC/DEC overflow ────────────────────────────────────────────────

#[test]
fn mame_inc_overflow() {
    let mut emu = run_program(&[0x86, 0x7F, 0x4C]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x80);
    assert!(emu.cpu.cc.contains(Flags::V));
    assert!(emu.cpu.cc.contains(Flags::N));
}

// ── ASL/ASR/LSR ─────────────────────────────────────────────────────

#[test]
fn mame_asla_sets_carry() {
    let mut emu = run_program(&[0x86, 0x81, 0x48]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x02);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mame_lsra_sets_carry() {
    let mut emu = run_program(&[0x86, 0x01, 0x44]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x00);
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_asra_preserves_sign() {
    let mut emu = run_program(&[0x86, 0x80, 0x47]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0xC0);
    assert!(!emu.cpu.cc.contains(Flags::C));
}

// ── DAA ─────────────────────────────────────────────────────────────

#[test]
fn mame_daa_low_nibble_adjust() {
    let mut emu = run_program(&[0x86, 0x09, 0x8B, 0x01, 0x19]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x10);
}

#[test]
fn mame_daa_no_adjust_needed() {
    let mut emu = run_program(&[0x86, 0x00, 0x19]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x00);
}

#[test]
fn mame_daa_high_nibble_adjust() {
    // A=$98, C=1 → DAA adds $60 → $F8 (MAME algorithm)
    let mut emu = run_program(&[0x86, 0x98, 0x1A, 0x01, 0x19]);
    emu.step();
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0xF8);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mame_cwai_waits_when_irq_masked() {
    let mut emu = run_program(&[0x3C, 0xFF]);
    emu.memory.write16(0xFFF8, 0x0200);
    emu.cpu.irq_pending = true;
    emu.cpu.cc.insert(Flags::I);
    let step = emu.step();
    assert_eq!(step.mnemonic, "CWAI");
    assert!(emu.cpu.halted);
    assert!(emu.cpu.cwai_waiting);
}

#[test]
fn mame_sync_wakes_on_masked_irq_line() {
    let mut emu = run_program(&[0x13, 0x12]);
    emu.step();
    assert!(emu.cpu.halted);
    emu.cpu.irq_pending = true;
    emu.cpu.cc.insert(Flags::I);
    let step = emu.step();
    assert_eq!(step.mnemonic, "SYNC");
    assert_eq!(emu.cpu.pc, 0x0101);
    let step2 = emu.step();
    assert_eq!(step2.mnemonic, "NOP");
}

// ── ORCC/ANDCC ──────────────────────────────────────────────────────

#[test]
fn mame_orcc_sets_and_andcc_clears() {
    let mut emu = run_program(&[0x1A, 0xFF, 0x1C, 0x00]);
    emu.step();
    assert_eq!(emu.cpu.cc.bits(), 0xFF);
    emu.step();
    assert_eq!(emu.cpu.cc.bits(), 0x00);
}

#[test]
fn mame_orcc_selective_set() {
    let mut emu = run_program(&[0x1A, 0x01]);
    emu.step();
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(!emu.cpu.cc.contains(Flags::Z));
    assert!(!emu.cpu.cc.contains(Flags::N));
}

#[test]
fn mame_andcc_selective_clear() {
    let mut emu = run_program(&[0x1A, 0xFF, 0x1C, 0xFE]);
    emu.step(); emu.step();
    assert!(!emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::Z));
    assert!(emu.cpu.cc.contains(Flags::N));
}

// ── TFR/EXG ────────────────────────────────────────────────────────

#[test]
fn mame_tfr_copy() {
    let mut emu = run_program(&[0x86, 0x42, 0xC6, 0x37, 0x1F, 0x89]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x42);
    assert_eq!(emu.cpu.b, 0x42);
}

#[test]
fn mame_exg_swap() {
    let mut emu = run_program(&[0x86, 0x42, 0xC6, 0x37, 0x1E, 0x89]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x37);
    assert_eq!(emu.cpu.b, 0x42);
}

#[test]
fn mame_tfr_16bit() {
    let mut emu = run_program(&[
        0xCE, 0x12, 0x34, // LDU #$1234
        0x8E, 0x56, 0x78, // LDX #$5678
        0x1F, 0x13,       // TFR X,U
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.u, 0x5678);
}

// ── Indexed addressing ──────────────────────────────────────────────

#[test]
fn mame_indexed_zero_offset() {
    let mut emu = run_program(&[
        0xCE, 0x02, 0x00,
        0x86, 0x42,
        0xA7, 0xC4,
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0200), 0x42);
}

#[test]
fn mame_indexed_auto_inc_1() {
    let mut emu = run_program(&[
        0xCE, 0x02, 0x00,
        0x86, 0x42,
        0xA7, 0xC0,
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0200), 0x42);
    assert_eq!(emu.cpu.u, 0x0201);
}

#[test]
fn mame_indexed_auto_dec_2() {
    let mut emu = run_program(&[
        0xCE, 0x02, 0x02,
        0x86, 0x42,
        0xA7, 0xC3,
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0200), 0x42);
    assert_eq!(emu.cpu.u, 0x0200);
}

#[test]
fn mame_indexed_const5_offset() {
    let mut emu = run_program(&[
        0xCE, 0x02, 0x00,
        0x86, 0x42,
        0xA7, 0x45,
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0205), 0x42);
}

#[test]
fn mame_indexed_acc_a_offset() {
    let mut emu = run_program(&[
        0xCE, 0x02, 0x00,
        0x86, 0x0A,
        0xC6, 0x42,
        0xE7, 0xC6,
    ]);
    emu.step(); emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x020A), 0x42);
}

// ── Cycle counts ────────────────────────────────────────────────────

#[test]
fn mame_cycle_count_nop() {
    let mut emu = run_program(&[0x12]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 2);
}

#[test]
fn mame_cycle_count_lda_imm() {
    let mut emu = run_program(&[0x86, 0x42]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 2);
}

#[test]
fn mame_cycle_count_jsr_ext() {
    let mut emu = run_program(&[0xBD, 0x01, 0x05, 0x12, 0x39]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 8);
}

#[test]
fn mame_cycle_count_bra_taken() {
    let mut emu = run_program(&[0x20, 0x02, 0x12, 0x12]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 3);
}

// ── LEA doesn't affect CC ───────────────────────────────────────────

#[test]
fn mame_leax_leay_no_flags() {
    let mut emu = run_program(&[
        0x86, 0x00,
        0xCE, 0x00, 0x00,
        0x30, 0x45,
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.x, 5);
    assert!(emu.cpu.cc.contains(Flags::Z));
}

// ── SWI vector and push ─────────────────────────────────────────────

#[test]
fn mame_swi_vectors_correctly() {
    let mut emu = Emulator::new();
    emu.memory.write16(0xFFFA, 0x0200);
    emu.memory.write8(0x0100, 0x3F);
    emu.memory.write8(0x0200, 0x3B);
    emu.cpu.pc = 0x0100;
    emu.cpu.s = 0x0400;

    emu.step();
    assert_eq!(emu.cpu.pc, 0x0200);
}

#[test]
fn mame_swi_pushes_12_bytes() {
    let mut emu = Emulator::new();
    emu.memory.write16(0xFFFA, 0x0200);
    emu.memory.write8(0x0100, 0x3F);
    emu.memory.write8(0x0200, 0x3B);
    emu.cpu.pc = 0x0100;
    emu.cpu.s = 0x0400;

    emu.step();
    assert_eq!(emu.cpu.s, 0x03F4);
}

#[test]
fn mame_rti_restores_state() {
    let mut emu = Emulator::new();
    emu.memory.write16(0xFFFA, 0x0200);
    emu.memory.write8(0x0100, 0x3F); // SWI
    emu.memory.write8(0x0200, 0x3B); // RTI
    emu.cpu.pc = 0x0100;
    emu.cpu.s = 0x0400;
    emu.cpu.a = 0x42;

    emu.step(); // SWI
    emu.step(); // RTI
    assert_eq!(emu.cpu.pc, 0x0101);
    assert_eq!(emu.cpu.a, 0x42);
}

// ── SUB/CMP carry (borrow) ──────────────────────────────────────────

#[test]
fn mame_suba_borrow_sets_carry() {
    // SUBA #$01 from A=#$00 → $FF, C=1 (borrow required)
    let mut emu = run_program(&[0x86, 0x00, 0x80, 0x01]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0xFF);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mame_suba_no_borrow_clears_carry() {
    // SUBA #$01 from A=#$02 → $01, C=0 (no borrow)
    let mut emu = run_program(&[0x86, 0x02, 0x80, 0x01]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x01);
    assert!(!emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mame_cmpa_less_sets_carry() {
    // CMPA #$43 with A=#$42 → C=1 (unsigned less than)
    let mut emu = run_program(&[0x86, 0x42, 0x81, 0x43]);
    emu.step(); emu.step();
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::N));
}

#[test]
fn mame_sbca_borrow_chain() {
    // SBCA #$01 with A=#$00, C=0 → $FF, C=1
    let mut emu = run_program(&[0x1C, 0xFE, 0x86, 0x00, 0x82, 0x01]);
    emu.step(); // ANDCC #$FE (clear C)
    emu.step(); // LDA #$00
    emu.step(); // SBCA #$01
    assert_eq!(emu.cpu.a, 0xFF);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mame_sex_sign_extend() {
    // SEX sign-extends B into A
    let mut emu = run_program(&[0xC6, 0x80, 0x1D]);
    emu.step(); emu.step(); // LDB #$80, SEX → A=$FF
    assert_eq!(emu.cpu.a, 0xFF);
    assert_eq!(emu.cpu.b, 0x80);
}

#[test]
fn mame_mul_carry_from_b_bit7() {
    //  *  =  — C from bit 7 of B (), Z=0 (D= non-zero)
    let mut emu = run_program(&[0x86, 0x80, 0xC6, 0x02, 0x3D]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x01);
    assert_eq!(emu.cpu.b, 0x00);
    assert!(!emu.cpu.cc.contains(Flags::C));
    assert!(!emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn mame_dec_zero_and_overflow() {
    // DECA $80 → $7F, V=1; DECA $01 → $00, Z=1
    let mut emu = run_program(&[0x86, 0x80, 0x4A]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x7F);
    assert!(emu.cpu.cc.contains(Flags::V));

    let mut emu = run_program(&[0x86, 0x01, 0x4A]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x00);
    assert!(emu.cpu.cc.contains(Flags::Z));
}
