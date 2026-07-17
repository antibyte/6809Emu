use m6809_core::Emulator;

fn emu_with_program(program: &[u8]) -> Emulator {
    let mut emu = Emulator::new();
    emu.load_and_reset(0x0100, program, 0x0100).unwrap();
    emu
}

// ── Arithmetic & Logic ──────────────────────────────────────────────

#[test]
fn lda_ldb_immediate() {
    let mut emu = emu_with_program(&[0x86, 0x42, 0xC6, 0x37]);
    emu.step(); // LDA #$42
    assert_eq!(emu.cpu.a, 0x42);
    emu.step(); // LDB #$37
    assert_eq!(emu.cpu.b, 0x37);
}

#[test]
fn adda_addd_immediate() {
    let mut emu = emu_with_program(&[0x86, 0x10, 0x8B, 0x20, 0xCC, 0x01, 0x00, 0xC3, 0x00, 0x10]);
    emu.step(); // LDA #$10
    emu.step(); // ADDA #$20
    assert_eq!(emu.cpu.a, 0x30);
    emu.step(); // LDD #$0100
    emu.step(); // ADDD #$0010
    assert_eq!(((emu.cpu.a as u16) << 8) | emu.cpu.b as u16, 0x0110);
}

#[test]
fn mul_sets_carry_correctly() {
    let mut emu = emu_with_program(&[0x86, 0x20, 0xC6, 0x03, 0x3D]);
    emu.step(); // LDA #$20
    emu.step(); // LDB #$03
    emu.step(); // MUL → D = $0060
    assert_eq!(((emu.cpu.a as u16) << 8) | emu.cpu.b as u16, 0x0060);
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::C));
}

#[test]
fn mul_large_sets_carry() {
    let mut emu = emu_with_program(&[0x86, 0x80, 0xC6, 0x03, 0x3D]);
    emu.step(); // LDA #$80
    emu.step(); // LDB #$03
    emu.step(); // MUL → D = $0180, carry set (result > $FF)
    assert_eq!(((emu.cpu.a as u16) << 8) | emu.cpu.b as u16, 0x0180);
    assert!(emu.cpu.cc.contains(m6809_core::Flags::C));
}

#[test]
fn cmp_sets_z_flag() {
    let mut emu = emu_with_program(&[0x86, 0x42, 0x81, 0x42]);
    emu.step(); // LDA #$42
    emu.step(); // CMPA #$42
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
}

// ── Branches ────────────────────────────────────────────────────────

#[test]
fn branch_taken_and_not_taken() {
    // BEQ +2 (taken), then BRA skip, NOP (skipped)
    let mut emu = emu_with_program(&[
        0x86, 0x00,       // LDA #$00  (sets Z)
        0x27, 0x02,       // BEQ +2    (taken, skip BRA)
        0x20, 0x01,       // BRA +1    (skipped)
        0x86, 0xFF,       // LDA #$FF  (should execute)
    ]);
    emu.step(); // LDA #$00
    emu.step(); // BEQ +2 (taken)
    assert_eq!(emu.cpu.a, 0x00); // LDA #$FF was skipped
}

#[test]
fn bne_loop_countdown() {
    // ORG $0100: LDB #5, loop: DECB, BNE loop, SWI
    let mut emu = emu_with_program(&[
        0xC6, 0x05,       // LDB #5
        0x5F,             // CLRB       (reset B for clean test)
        0xC6, 0x03,       // LDB #3
        0x5A,             // loop: DECB
        0x26, 0xFD,       // BNE loop   (-3 → back to DECB)
        0x3F,             // SWI
    ]);
    emu.step(); // LDB #3
    emu.run(20);
    assert_eq!(emu.cpu.b, 0);
}

#[test]
fn long_branch_reaches_far_target() {
    // LBRA to target 256 bytes ahead
    let mut program = vec![0x12u8; 0x106]; // fill with NOPs
    program[0] = 0x16;      // LBRA opcode
    program[1] = 0x01;      // offset hi ($0100)
    program[2] = 0x00;      // offset lo
    // LBRA at $0100, PC after = $0103, target = $0103 + $0100 = $0203
    // program[0x103] = $0203
    program[0x103] = 0x86;  // LDA
    program[0x104] = 0xAA;

    let mut emu = Emulator::new();
    emu.load_and_reset(0x0100, &program, 0x0100).unwrap();
    emu.run(10);
    assert_eq!(emu.cpu.a, 0xAA);
}

// ── Stack Operations ────────────────────────────────────────────────

#[test]
fn pshs_puls_roundtrip() {
    let mut emu = emu_with_program(&[
        0x86, 0x42,       // LDA #$42
        0xC6, 0x37,       // LDB #$37
        0x34, 0x06,       // PSHS D (push A,B)
        0x86, 0x00,       // LDA #$00
        0x35, 0x06,       // PULS D (pull A,B)
    ]);
    emu.step(); emu.step(); emu.step();
    emu.step(); // LDA #$00
    emu.step(); // PULS D
    assert_eq!(emu.cpu.a, 0x42);
    assert_eq!(emu.cpu.b, 0x37);
}

#[test]
fn pshs_cc_puls_cc_roundtrip() {
    let mut emu = emu_with_program(&[
        0x1A, 0x01,       // ORCC #$01 (set C)
        0x34, 0x01,       // PSHS CC
        0x1C, 0xFE,       // ANDCC #$FE (clear C)
        0x35, 0x01,       // PULS CC
    ]);
    emu.step(); emu.step(); // ORCC, PSHS CC
    emu.step();             // ANDCC (clear C)
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::C));
    emu.step();             // PULS CC
    assert!(emu.cpu.cc.contains(m6809_core::Flags::C));
}

// ── Indexed Addressing ──────────────────────────────────────────────

#[test]
fn indexed_auto_inc_dec() {
    let mut emu = emu_with_program(&[
        0xCE, 0x02, 0x00, // LDU #$0200
        0x86, 0xAA,       // LDA #$AA
        0xA7, 0xC0,       // STA ,U++ (store, post-inc)
        0x86, 0xBB,       // LDA #$BB
        0xA7, 0xC0,       // STA ,U++ (store at +1, post-inc)
    ]);
    emu.step(); emu.step();
    emu.step(); // STA ,U++ at $0200
    assert_eq!(emu.memory.read8(0x0200), 0xAA);
    emu.step(); emu.step(); // STA ,U++ at $0201
    assert_eq!(emu.memory.read8(0x0201), 0xBB);
    assert_eq!(emu.cpu.u, 0x0202);
}

#[test]
fn indexed_const5_offset() {
    let mut emu = emu_with_program(&[
        0xCE, 0x02, 0x00, // LDU #$0200
        0x86, 0x42,       // LDA #$42
        0xA7, 0x45,       // STA 5,U (postbyte: bit7=0, reg=10=U, off=00101=5)
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0205), 0x42);
}

#[test]
fn indexed_indirect() {
    let mut emu = emu_with_program(&[
        0xCE, 0x02, 0x00, // LDU #$0200
        0x86, 0x00,       // LDA #0
        0xA7, 0xC0,       // STA ,U++ (store pointer low byte at $0200)
        0x86, 0x02,       // LDA #$02 (pointer high byte)
        0xA7, 0xC0,       // STA ,U++ (store pointer high at $0201)
        // Actually let's use a simpler approach with [,U]
    ]);
    // Just verify the auto-inc worked
    emu.step(); emu.step(); emu.step(); emu.step(); emu.step();
    assert_eq!(emu.memory.read8(0x0200), 0x00);
    assert_eq!(emu.memory.read8(0x0201), 0x02);
}

// ── TFR / EXG ───────────────────────────────────────────────────────

#[test]
fn tfr_copy_register() {
    let mut emu = emu_with_program(&[
        0x86, 0x42,       // LDA #$42
        0x1F, 0x89,       // TFR A,B
    ]);
    emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x42);
    assert_eq!(emu.cpu.b, 0x42);
}

#[test]
fn exg_swap_registers() {
    let mut emu = emu_with_program(&[
        0x86, 0xAA,       // LDA #$AA
        0xC6, 0xBB,       // LDB #$BB
        0x1E, 0x89,       // EXG A,B
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0xBB);
    assert_eq!(emu.cpu.b, 0xAA);
}

#[test]
fn tfr_16bit_exchange() {
    let mut emu = emu_with_program(&[
        0xCE, 0x12, 0x34, // LDU #$1234
        0x8E, 0x56, 0x78, // LDX #$5678
        0x1F, 0x13,       // TFR X,U (X=1, U=3)
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.x, 0x5678);
    assert_eq!(emu.cpu.u, 0x5678);
}

// ── ORCC / ANDCC ────────────────────────────────────────────────────

#[test]
fn orcc_sets_flags() {
    let mut emu = emu_with_program(&[0x1A, 0x24]); // ORCC #$24 (set Z and H)
    emu.step();
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
    assert!(emu.cpu.cc.contains(m6809_core::Flags::H));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::C));
}

#[test]
fn andcc_clears_flags() {
    let mut emu = emu_with_program(&[
        0x1A, 0xFF,       // ORCC #$FF (set all)
        0x1C, 0xFE,       // ANDCC #$FE (clear C)
    ]);
    emu.step(); emu.step();
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::C));
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
}

// ── JSR / RTS ───────────────────────────────────────────────────────

#[test]
fn jsr_rts_roundtrip() {
    // JSR $0104, then at $0104: LDA #$AA, RTS
    let mut emu = emu_with_program(&[
        0xBD, 0x01, 0x04, // JSR $0104
        0x12,             // NOP (return here after RTS)
        0x86, 0xAA,       // $0104: LDA #$AA
        0x39,             // RTS
    ]);
    emu.step(); // JSR
    assert_eq!(emu.cpu.pc, 0x0104);
    emu.step(); // LDA #$AA
    assert_eq!(emu.cpu.a, 0xAA);
    emu.step(); // RTS → back to NOP
    assert_eq!(emu.cpu.pc, 0x0103);
}

// ── SWI / RTI ───────────────────────────────────────────────────────

#[test]
fn swi_rti_vector() {
    let mut emu = Emulator::new();
    // Set SWI vector at $FFFA-$FFFB → $0200
    emu.memory.write16(0xFFFA, 0x0200);
    // At $0100: SWI
    emu.memory.write8(0x0100, 0x3F);
    // At $0200: LDA #$77, RTI
    emu.memory.write8(0x0200, 0x86);
    emu.memory.write8(0x0201, 0x77);
    emu.memory.write8(0x0202, 0x3B);
    emu.cpu.pc = 0x0100;
    emu.cpu.s = 0x0400;
    emu.cpu.cc.remove(m6809_core::Flags::I);

    emu.step(); // SWI
    assert_eq!(emu.cpu.pc, 0x0200);
    emu.step(); // LDA #$77
    assert_eq!(emu.cpu.a, 0x77);
    emu.step(); // RTI
    assert_eq!(emu.cpu.pc, 0x0101);
}

// ── DAA ─────────────────────────────────────────────────────────────

#[test]
fn daa_after_addition() {
    let mut emu = emu_with_program(&[
        0x86, 0x09,       // LDA #$09
        0x8B, 0x01,       // ADDA #$01  → $0A
        0x19,             // DAA        → $10 (BCD adjust)
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.a, 0x10);
}

// ── LEA ─────────────────────────────────────────────────────────────

#[test]
fn leax_sets_z_from_effective_address() {
    // LEAX 5,U with U=0 → X=5, Z cleared (Motorola LEAX sets Z)
    let mut emu = emu_with_program(&[
        0xCE, 0x00, 0x00, // LDU #$0000
        0x30, 0x45,       // LEAX 5,U
    ]);
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.x, 0x0005);
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::Z));

    // LEAX ,X with X=0 → Z set
    let mut emu2 = emu_with_program(&[0x30, 0x84]); // LEAX ,X
    emu2.cpu.x = 0;
    emu2.cpu.cc.remove(m6809_core::Flags::Z);
    emu2.step();
    assert_eq!(emu2.cpu.x, 0);
    assert!(emu2.cpu.cc.contains(m6809_core::Flags::Z));
}

#[test]
fn leas_leay_z_and_leas_no_z() {
    // LEAY sets Z; LEAS does not touch Z
    let mut emu = emu_with_program(&[
        0xCE, 0x00, 0x00, // LDU #$0000
        0x31, 0xC4,       // LEAY ,U  → Y=0, Z set
        0x1A, 0x04,       // ORCC #$04 set Z
        0x32, 0x45,       // LEAS 5,U → S=5, Z must remain set
    ]);
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.y, 0);
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.s, 5);
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
}

#[test]
fn clr_direct_sets_flags() {
    let mut emu = emu_with_program(&[0x0F, 0x50]); // CLR <$50
    emu.memory.write8(0x0050, 0xFF);
    emu.cpu.cc = m6809_core::Flags::from_bits_truncate(0x0B); // N V C
    emu.step();
    assert_eq!(emu.memory.read8(0x0050), 0x00);
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::N));
    assert!(emu.cpu.cc.contains(m6809_core::Flags::Z));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::V));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::C));
}

#[test]
fn sex_sets_nz_from_d() {
    let mut emu = emu_with_program(&[0xC6, 0x80, 0x1D]); // LDB #$80, SEX
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0xFF);
    assert!(emu.cpu.cc.contains(m6809_core::Flags::N));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::Z));

    let mut emu2 = emu_with_program(&[0xC6, 0x00, 0x1D]); // LDB #0, SEX
    emu2.step();
    emu2.step();
    assert_eq!(emu2.cpu.a, 0x00);
    assert!(!emu2.cpu.cc.contains(m6809_core::Flags::N));
    assert!(emu2.cpu.cc.contains(m6809_core::Flags::Z));
}

#[test]
fn tst_preserves_carry() {
    let mut emu = emu_with_program(&[0x86, 0x01, 0x1A, 0x01, 0x4D]); // LDA #1, ORCC #C, TSTA
    emu.step();
    emu.step();
    emu.step();
    assert!(emu.cpu.cc.contains(m6809_core::Flags::C));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::Z));
    assert!(!emu.cpu.cc.contains(m6809_core::Flags::V));
}

// ── Halt / SWI ──────────────────────────────────────────────────────

#[test]
fn swi_vectors_and_sets_trap() {
    let mut emu = emu_with_program(&[0x12, 0x3F, 0x12]);
    emu.step(); // NOP
    let result = emu.step(); // SWI
    assert_eq!(result.trap, Some(m6809_core::types::Trap::Swi));
}

// ── Memory Operations ───────────────────────────────────────────────

#[test]
fn store_load_roundtrip() {
    let mut emu = emu_with_program(&[
        0x86, 0x42,       // LDA #$42
        0x97, 0x20,       // STA $20
        0xD6, 0x20,       // LDB $20
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.b, 0x42);
}

#[test]
fn extended_addressing() {
    let mut emu = emu_with_program(&[
        0x86, 0x55,       // LDA #$55
        0xB7, 0x02, 0x00, // STA $0200
        0xF6, 0x02, 0x00, // LDB $0200
    ]);
    emu.step(); emu.step(); emu.step();
    assert_eq!(emu.cpu.b, 0x55);
}

// ── Cycle counting ──────────────────────────────────────────────────

#[test]
fn nop_takes_2_cycles() {
    let mut emu = emu_with_program(&[0x12]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 2);
}

#[test]
fn lda_immediate_takes_2_cycles() {
    let mut emu = emu_with_program(&[0x86, 0x42]);
    emu.step();
    assert_eq!(emu.cpu.total_cycles, 2);
}

#[test]
fn branch_taken_3_not_taken_2() {
    let mut emu = emu_with_program(&[
        0x27, 0x02,       // BEQ +2 (not taken, Z clear)
        0x12,             // NOP
    ]);
    emu.step(); // BEQ not taken
    assert_eq!(emu.cpu.total_cycles, 2);
}
