use m6809_core::{CpuVariant, Emulator, Flags};

fn run_mc6809(program: &[u8]) -> Emulator {
    let mut emu = Emulator::new();
    emu.set_variant(CpuVariant::Mc6809);
    emu.load_and_reset(0x0100, program, 0x0100).unwrap();
    emu
}

#[test]
fn page2_fallthrough_clra() {
    // MAME #6797: $10 $4F on real 6809 executes CLRA
    let mut emu = run_mc6809(&[0x10, 0x4F]);
    emu.cpu.b = 0xFF;
    let step = emu.step();
    assert_eq!(step.mnemonic, "CLRA");
    assert_eq!(emu.cpu.a, 0x00);
    assert_eq!(emu.cpu.b, 0xFF);
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn opcode_01_is_neg_direct() {
    let mut emu = run_mc6809(&[0x01, 0x42]);
    emu.memory.write8(0x0042, 0x05);
    let step = emu.step();
    assert_eq!(step.mnemonic, "NEG");
    assert_eq!(emu.memory.read8(0x0042), 0xFB);
    assert!(step.trap.is_none());
}

#[test]
fn opcode_14_enters_hcf_mode() {
    let mut emu = run_mc6809(&[0x14]);
    let step = emu.step();
    assert_eq!(step.mnemonic, "HCF");
    assert!(emu.cpu.free_run);
    let step2 = emu.step();
    assert_eq!(step2.mnemonic, "HCF");
    assert_eq!(emu.cpu.pc, 0x0101);
}

#[test]
fn opcode_1b_is_nop() {
    let mut emu = run_mc6809(&[0x1B, 0x12]);
    let step = emu.step();
    assert_eq!(step.mnemonic, "NOP");
    assert_eq!(emu.cpu.pc, 0x0101);
}

#[test]
fn xdec_sets_carry_from_lsb() {
    let mut emu = run_mc6809(&[0x86, 0x03, 0x4B]);
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0x02);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn mul_z_flag_from_d_register() {
    //  *  =  — Z from full D register ( non-zero)
    let mut emu = run_mc6809(&[0x86, 0x10, 0xC6, 0x10, 0x3D]);
    emu.step();
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0x01);
    assert_eq!(emu.cpu.b, 0x00);
    assert!(!emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn hd6309_still_traps_illegal_page1() {
    let mut emu = Emulator::new();
    emu.set_variant(CpuVariant::Hd6309);
    emu.load_and_reset(0x0100, &[0x01], 0x0100).unwrap();
    let step = emu.step();
    assert_eq!(step.trap, Some(m6809_core::types::Trap::IllegalOpcode));
}

#[test]
fn xnc_neg_when_carry_clear() {
    let mut emu = run_mc6809(&[0x86, 0x05, 0x42]);
    emu.cpu.cc.remove(Flags::C);
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0xFB);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn xnc_com_when_carry_set() {
    let mut emu = run_mc6809(&[0x86, 0x05, 0x1A, 0x01, 0x42]);
    emu.step();
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0xFA);
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn xclr_preserves_carry() {
    let mut emu = run_mc6809(&[0x86, 0x55, 0x1A, 0x01, 0x4E]);
    emu.step();
    emu.step();
    emu.step();
    assert_eq!(emu.cpu.a, 0x00);
    assert!(emu.cpu.cc.contains(Flags::C));
    assert!(emu.cpu.cc.contains(Flags::Z));
}

#[test]
fn opcode_cd_enters_hcf_mode() {
    let mut emu = run_mc6809(&[0xCD]);
    let step = emu.step();
    assert_eq!(step.mnemonic, "HCF");
    assert!(emu.cpu.free_run);
}

#[test]
fn page3_fallthrough_clrb() {
    let mut emu = run_mc6809(&[0x11, 0x5F]);
    emu.cpu.b = 0x42;
    let step = emu.step();
    assert_eq!(step.mnemonic, "CLRB");
    assert_eq!(emu.cpu.b, 0x00);
}

#[test]
fn xandcc_extra_cycle() {
    let mut emu = run_mc6809(&[0x38, 0xFE]);
    let step = emu.step();
    assert_eq!(step.mnemonic, "XANDCC");
    assert_eq!(step.cycles, 4);
    assert!(!emu.cpu.cc.contains(Flags::C));
}

#[test]
fn com_clears_overflow_flag() {
    let mut emu = run_mc6809(&[0x86, 0x55, 0x43]);
    emu.cpu.cc.insert(Flags::V);
    emu.step();
    emu.step();
    assert!(!emu.cpu.cc.contains(Flags::V));
    assert!(emu.cpu.cc.contains(Flags::C));
}

#[test]
fn x18_manipulates_cc() {
    let mut emu = run_mc6809(&[0x18, 0xFF]);
    emu.cpu.cc = Flags::from_bits_truncate(0x01);
    let step = emu.step();
    assert_eq!(step.mnemonic, "X18");
    assert_eq!(emu.cpu.cc.bits(), 0x02);
}

#[test]
fn xres_vectors_through_reset() {
    let mut emu = run_mc6809(&[0x3E]);
    emu.cpu.s = 0x0200;
    emu.cpu.pc = 0x0100;
    emu.memory.write16(0xFFFE, 0xC000);
    let step = emu.step();
    assert_eq!(step.mnemonic, "XRES");
    assert_eq!(emu.cpu.pc, 0xC000);
}

#[test]
fn xswi2_vectors_to_fff4() {
    let mut emu = run_mc6809(&[0x10, 0x3E]);
    emu.cpu.s = 0x0200;
    emu.memory.write16(0xFFF4, 0xD000);
    let step = emu.step();
    assert_eq!(step.mnemonic, "XSWI2");
    assert_eq!(emu.cpu.pc, 0xD000);
}

#[test]
fn xfirq_vectors_to_fff6() {
    let mut emu = run_mc6809(&[0x11, 0x3E]);
    emu.cpu.s = 0x0200;
    emu.memory.write16(0xFFF6, 0xE000);
    let step = emu.step();
    assert_eq!(step.mnemonic, "XFIRQ");
    assert_eq!(emu.cpu.pc, 0xE000);
}

#[test]
fn xst8_imm_sets_flags_without_store() {
    let mut emu = run_mc6809(&[0x86, 0x55, 0x87, 0x00]);
    emu.step();
    let step = emu.step();
    assert_eq!(step.mnemonic, "XST");
    assert_eq!(emu.cpu.a, 0x55);
    assert!(!emu.cpu.cc.contains(Flags::Z));
    assert!(!emu.cpu.cc.contains(Flags::V));
}

#[test]
fn xadd_imm_updates_flags_not_register() {
    let mut emu = run_mc6809(&[0xCC, 0x10, 0x00, 0x10, 0xC3, 0x00, 0x01]);
    emu.step();
    let step = emu.step();
    assert_eq!(step.mnemonic, "XADD");
    assert_eq!(emu.cpu.a, 0x10);
    assert_eq!(emu.cpu.b, 0x00);
    assert!(!emu.cpu.cc.contains(Flags::C));
}

#[test]
fn tfr_x_to_b_copies_low_byte() {
    let mut emu = run_mc6809(&[0x1F, 0x19]);
    emu.cpu.x = 0xABCD;
    emu.cpu.b = 0x00;
    emu.step();
    assert_eq!(emu.cpu.b, 0xCD);
    assert_eq!(emu.cpu.x, 0xABCD);
}

#[test]
fn tfr_b_to_y_fills_high_byte_with_ones() {
    let mut emu = run_mc6809(&[0x1F, 0x92]);
    emu.cpu.b = 0x42;
    emu.cpu.y = 0x0000;
    emu.step();
    assert_eq!(emu.cpu.y, 0xFF42);
}

#[test]
fn tfr_cc_to_y_duplicates_cc_byte() {
    let mut emu = run_mc6809(&[0x1F, 0xA2]);
    emu.cpu.cc = Flags::from_bits_truncate(0x50);
    emu.cpu.y = 0;
    emu.step();
    assert_eq!(emu.cpu.y, 0x5050);
}

#[test]
fn tfr_dp_to_y_duplicates_dp_byte() {
    let mut emu = run_mc6809(&[0x1F, 0xB2]);
    emu.cpu.dp = 0x42;
    emu.cpu.y = 0;
    emu.step();
    assert_eq!(emu.cpu.y, 0x4242);
}

#[test]
fn tfr_to_s_enables_nmi() {
    let mut emu = run_mc6809(&[0x1F, 0x14]);
    emu.memory.write16(0xFFFC, 0x0300);
    emu.step();
    assert!(emu.cpu.lds_encountered);
    emu.trigger_nmi();
    let step = emu.step();
    assert_eq!(step.mnemonic, "NMI");
    assert_eq!(emu.cpu.pc, 0x0300);
}

#[test]
fn tfr_undefined_reg7_to_y_is_ffff() {
    let mut emu = run_mc6809(&[0x1F, 0x72]);
    emu.cpu.y = 0x0000;
    emu.step();
    assert_eq!(emu.cpu.y, 0xFFFF);
}

#[test]
fn nmi_ignored_until_first_lds() {
    let mut emu = run_mc6809(&[0x12]);
    emu.memory.write16(0xFFFC, 0x0300);
    emu.trigger_nmi();
    let step = emu.step();
    assert_eq!(step.mnemonic, "NOP");
    assert_eq!(emu.cpu.pc, 0x0101);
    assert!(!emu.cpu.lds_encountered);

    let mut emu2 = run_mc6809(&[0x10, 0xCE, 0x01, 0xFF]);
    emu2.memory.write16(0xFFFC, 0x0300);
    emu2.step();
    assert!(emu2.cpu.lds_encountered);
    emu2.trigger_nmi();
    let step = emu2.step();
    assert_eq!(step.mnemonic, "NMI");
    assert_eq!(emu2.cpu.pc, 0x0300);
}