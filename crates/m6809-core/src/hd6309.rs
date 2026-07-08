use crate::addressing::{format_index_operand, indexed_addr};
use crate::alu::{
    adc16, add16, add8, and16, asl16, asr16, bit16, cmp16, cmp8, com16, dec16, eor16, inc16,
    lsr16, neg16, or16, rol16, ror16, sbc16, sub16, sub8, tst16,
};
use crate::cpu::{Cpu, Reg16, StepCtx, TfmPending};
use crate::flags::Flags;
use crate::memory::Memory;
use crate::types::{CpuVariant, StepResult, Trap};

/// Max bytes transferred per `step()` during TFM (keeps the UI responsive).
const TFM_CHUNK_SIZE: u16 = 512;

fn tfm_mnemonic(opcode: u8) -> &'static str {
    match opcode {
        0x38 => "TFM+",
        0x39 => "TFM-",
        0x3A => "TFM+R",
        0x3B => "TFM+W",
        _ => "TFM",
    }
}

impl Cpu {
    pub fn is_hd6309(&self) -> bool {
        self.variant == CpuVariant::Hd6309
    }

    pub(crate) fn try_hd6309_page2(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_hd6309() {
            return false;
        }

        match opcode {
            0x01 | 0x02 | 0x05 | 0x0B => {
                self.op_logic_imm_dir(mem, ctx, opcode);
                true
            }
            0x61 | 0x62 | 0x65 | 0x6B => {
                self.op_logic_imm_idx(mem, ctx, opcode);
                true
            }
            0x71 | 0x72 | 0x75 | 0x7B => {
                self.op_logic_imm_ext(mem, ctx, opcode);
                true
            }
            0x30..=0x37 => {
                self.op_inter_reg(mem, ctx, opcode);
                true
            }
            0x38 => {
                self.op_pshsw(mem, ctx);
                true
            }
            0x39 => {
                self.op_pulsw(mem, ctx);
                true
            }
            0x3A => {
                self.op_pshuw(mem, ctx);
                true
            }
            0x3B => {
                self.op_puluw(mem, ctx);
                true
            }
            0x3E => {
                self.op_muld_inh(ctx);
                true
            }
            // D-register inherent unary (MAME page2 $40–$4F)
            0x40 => {
                self.op_d_unary_inh(ctx, "NEGD", |cpu, v| neg16(v, &mut cpu.cc));
                true
            }
            0x43 => {
                self.op_d_unary_inh(ctx, "COMD", |cpu, v| com16(v, &mut cpu.cc));
                true
            }
            0x44 => {
                self.op_d_unary_inh(ctx, "LSRD", |cpu, v| lsr16(v, &mut cpu.cc));
                true
            }
            0x46 => {
                self.op_d_unary_inh(ctx, "RORD", |cpu, v| ror16(v, &mut cpu.cc));
                true
            }
            0x47 => {
                self.op_d_unary_inh(ctx, "ASRD", |cpu, v| asr16(v, &mut cpu.cc));
                true
            }
            0x48 => {
                self.op_d_unary_inh(ctx, "ASLD", |cpu, v| asl16(v, &mut cpu.cc));
                true
            }
            0x49 => {
                self.op_d_unary_inh(ctx, "ROLD", |cpu, v| rol16(v, &mut cpu.cc));
                true
            }
            0x4A => {
                self.op_d_unary_inh(ctx, "DECD", |cpu, v| dec16(v, &mut cpu.cc));
                true
            }
            0x4C => {
                self.op_d_unary_inh(ctx, "INCD", |cpu, v| inc16(v, &mut cpu.cc));
                true
            }
            0x4D => {
                tst16(self.get_reg16(Reg16::D), &mut self.cc);
                ctx.cycles = 3;
                ctx.mnemonic = "TSTD".into();
                ctx.operands.clear();
                true
            }
            0x4F => {
                self.set_reg16(Reg16::D, 0);
                self.cc.remove(Flags::V | Flags::C);
                self.cc.insert(Flags::Z);
                self.cc.remove(Flags::N);
                ctx.cycles = 3;
                ctx.mnemonic = "CLRD".into();
                ctx.operands.clear();
                true
            }
            // W-register inherent unary (MAME page2 $50–$5F)
            0x50 => {
                self.op_w_unary_inh(ctx, "NEGW", |cpu, v| neg16(v, &mut cpu.cc));
                true
            }
            0x53 => {
                self.op_w_unary_inh(ctx, "COMW", |cpu, v| com16(v, &mut cpu.cc));
                true
            }
            0x54 => {
                self.op_w_unary_inh(ctx, "LSRW", |cpu, v| lsr16(v, &mut cpu.cc));
                true
            }
            0x56 => {
                self.op_w_unary_inh(ctx, "RORW", |cpu, v| ror16(v, &mut cpu.cc));
                true
            }
            0x57 => {
                self.op_w_unary_inh(ctx, "ASRW", |cpu, v| asr16(v, &mut cpu.cc));
                true
            }
            0x58 => {
                self.op_w_unary_inh(ctx, "LSLW", |cpu, v| asl16(v, &mut cpu.cc));
                true
            }
            0x59 => {
                self.op_w_unary_inh(ctx, "ROLW", |cpu, v| rol16(v, &mut cpu.cc));
                true
            }
            0x5A => {
                self.op_w_unary_inh(ctx, "DECW", |cpu, v| dec16(v, &mut cpu.cc));
                true
            }
            0x5C => {
                self.op_w_unary_inh(ctx, "INCW", |cpu, v| inc16(v, &mut cpu.cc));
                true
            }
            0x5D => {
                tst16(self.w, &mut self.cc);
                ctx.cycles = 3;
                ctx.mnemonic = "TSTW".into();
                ctx.operands.clear();
                true
            }
            0x5F => {
                self.w = 0;
                self.cc.remove(Flags::V | Flags::C);
                self.cc.insert(Flags::Z);
                self.cc.remove(Flags::N);
                ctx.cycles = 3;
                ctx.mnemonic = "CLRW".into();
                ctx.operands.clear();
                true
            }
            0x80 => {
                self.op_alu16_imm(mem, ctx, "SUBW", |cpu, a, b| sub16(a, b, &mut cpu.cc));
                true
            }
            0x81 => {
                self.op_alu16_imm(mem, ctx, "CMPW", |cpu, a, b| {
                    cmp16(a, b, &mut cpu.cc);
                    a
                });
                true
            }
            0x82 => {
                self.op_d_alu16_imm(mem, ctx, "SBCD", |cpu, a, b| sbc16(a, b, &mut cpu.cc));
                true
            }
            0x84 => {
                self.op_d_alu16_imm(mem, ctx, "ANDD", |cpu, a, b| and16(a, b, &mut cpu.cc));
                true
            }
            0x85 => {
                self.op_d_bit16_imm(mem, ctx, "BITD");
                true
            }
            0x88 => {
                self.op_d_alu16_imm(mem, ctx, "EORD", |cpu, a, b| eor16(a, b, &mut cpu.cc));
                true
            }
            0x89 => {
                self.op_d_alu16_imm(mem, ctx, "ADCD", |cpu, a, b| adc16(a, b, &mut cpu.cc));
                true
            }
            0x8A => {
                self.op_d_alu16_imm(mem, ctx, "ORD", |cpu, a, b| or16(a, b, &mut cpu.cc));
                true
            }
            0x8B => {
                self.op_alu16_imm(mem, ctx, "ADDW", |cpu, a, b| add16(a, b, &mut cpu.cc));
                true
            }
            0x90 => {
                self.op_alu16_dir(mem, ctx, "SUBW", |cpu, a, b| sub16(a, b, &mut cpu.cc));
                true
            }
            0x91 => {
                self.op_alu16_dir(mem, ctx, "CMPW", |cpu, a, b| {
                    cmp16(a, b, &mut cpu.cc);
                    a
                });
                true
            }
            0x92 => {
                self.op_d_alu16_dir(mem, ctx, "SBCD", |cpu, a, b| sbc16(a, b, &mut cpu.cc));
                true
            }
            0x94 => {
                self.op_d_alu16_dir(mem, ctx, "ANDD", |cpu, a, b| and16(a, b, &mut cpu.cc));
                true
            }
            0x95 => {
                self.op_d_bit16_dir(mem, ctx, "BITD");
                true
            }
            0x98 => {
                self.op_d_alu16_dir(mem, ctx, "EORD", |cpu, a, b| eor16(a, b, &mut cpu.cc));
                true
            }
            0x99 => {
                self.op_d_alu16_dir(mem, ctx, "ADCD", |cpu, a, b| adc16(a, b, &mut cpu.cc));
                true
            }
            0x9A => {
                self.op_d_alu16_dir(mem, ctx, "ORD", |cpu, a, b| or16(a, b, &mut cpu.cc));
                true
            }
            0x9B => {
                self.op_alu16_dir(mem, ctx, "ADDW", |cpu, a, b| add16(a, b, &mut cpu.cc));
                true
            }
            0xA0 => {
                self.op_alu16_idx(mem, ctx, "SUBW", |cpu, a, b| sub16(a, b, &mut cpu.cc));
                true
            }
            0xA1 => {
                self.op_alu16_idx(mem, ctx, "CMPW", |cpu, a, b| {
                    cmp16(a, b, &mut cpu.cc);
                    a
                });
                true
            }
            0xA2 => {
                self.op_d_alu16_idx(mem, ctx, "SBCD", |cpu, a, b| sbc16(a, b, &mut cpu.cc));
                true
            }
            0xA4 => {
                self.op_d_alu16_idx(mem, ctx, "ANDD", |cpu, a, b| and16(a, b, &mut cpu.cc));
                true
            }
            0xA5 => {
                self.op_d_bit16_idx(mem, ctx, "BITD");
                true
            }
            0xA8 => {
                self.op_d_alu16_idx(mem, ctx, "EORD", |cpu, a, b| eor16(a, b, &mut cpu.cc));
                true
            }
            0xA9 => {
                self.op_d_alu16_idx(mem, ctx, "ADCD", |cpu, a, b| adc16(a, b, &mut cpu.cc));
                true
            }
            0xAA => {
                self.op_d_alu16_idx(mem, ctx, "ORD", |cpu, a, b| or16(a, b, &mut cpu.cc));
                true
            }
            0xAB => {
                self.op_alu16_idx(mem, ctx, "ADDW", |cpu, a, b| add16(a, b, &mut cpu.cc));
                true
            }
            0xB0 => {
                self.op_alu16_ext(mem, ctx, "SUBW", |cpu, a, b| sub16(a, b, &mut cpu.cc));
                true
            }
            0xB1 => {
                self.op_alu16_ext(mem, ctx, "CMPW", |cpu, a, b| {
                    cmp16(a, b, &mut cpu.cc);
                    a
                });
                true
            }
            0xB2 => {
                self.op_d_alu16_ext(mem, ctx, "SBCD", |cpu, a, b| sbc16(a, b, &mut cpu.cc));
                true
            }
            0xB4 => {
                self.op_d_alu16_ext(mem, ctx, "ANDD", |cpu, a, b| and16(a, b, &mut cpu.cc));
                true
            }
            0xB5 => {
                self.op_d_bit16_ext(mem, ctx, "BITD");
                true
            }
            0xB8 => {
                self.op_d_alu16_ext(mem, ctx, "EORD", |cpu, a, b| eor16(a, b, &mut cpu.cc));
                true
            }
            0xB9 => {
                self.op_d_alu16_ext(mem, ctx, "ADCD", |cpu, a, b| adc16(a, b, &mut cpu.cc));
                true
            }
            0xBA => {
                self.op_d_alu16_ext(mem, ctx, "ORD", |cpu, a, b| or16(a, b, &mut cpu.cc));
                true
            }
            0xBB => {
                self.op_alu16_ext(mem, ctx, "ADDW", |cpu, a, b| add16(a, b, &mut cpu.cc));
                true
            }
            0x86 => {
                self.op_ldw_imm(mem, ctx);
                true
            }
            0x96 => {
                self.op_ldw_dir(mem, ctx);
                true
            }
            0xA6 => {
                self.op_ldw_idx(mem, ctx);
                true
            }
            0xB6 => {
                self.op_ldw_ext(mem, ctx);
                true
            }
            0x97 => {
                self.op_stw_dir(mem, ctx);
                true
            }
            0xA7 => {
                self.op_stw_idx(mem, ctx);
                true
            }
            0xB7 => {
                self.op_stw_ext(mem, ctx);
                true
            }
            0xDC => {
                self.op_ldq_dir(mem, ctx);
                true
            }
            0xDD => {
                self.op_stq_dir(mem, ctx);
                true
            }
            0xEC => {
                self.op_ldq_idx(mem, ctx);
                true
            }
            0xED => {
                self.op_stq_idx(mem, ctx);
                true
            }
            0xFC => {
                self.op_ldq_ext(mem, ctx);
                true
            }
            0xFD => {
                self.op_stq_ext(mem, ctx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn try_hd6309_page3(
        &mut self,
        opcode: u8,
        mem: &mut Memory,
        ctx: &mut StepCtx,
    ) -> bool {
        if !self.is_hd6309() {
            return false;
        }

        match opcode {
            0x30..=0x37 => {
                self.op_bit_transfer(mem, ctx, opcode);
                true
            }
            0x38..=0x3B => {
                self.op_tfm(mem, ctx, opcode);
                true
            }
            0x3C => {
                let imm = self.fetch_imm8(mem, ctx);
                self.mode_reg &= imm;
                self.cc.set_nz8(self.mode_reg);
                self.mode_reg &= 0x3F;
                ctx.cycles = 4;
                ctx.mnemonic = "BITMD".into();
                ctx.operands = format!("#${imm:02X}");
                true
            }
            0x3D => {
                let imm = self.fetch_imm8(mem, ctx);
                self.mode_reg = imm;
                ctx.cycles = 4;
                ctx.mnemonic = "LDMD".into();
                ctx.operands = format!("#${imm:02X}");
                true
            }
            // E-register inherent unary (page3 $43–$4F)
            0x43 => {
                self.op_e_unary_inh(ctx, "COME", |cpu, v| cpu.op_com8(v));
                true
            }
            0x4A => {
                self.op_e_unary_inh(ctx, "DECE", |cpu, v| cpu.op_dec8(v));
                true
            }
            0x4C => {
                self.op_e_unary_inh(ctx, "INCE", |cpu, v| cpu.op_inc8(v));
                true
            }
            0x4D => {
                self.op_e_tst(ctx);
                true
            }
            0x4F => {
                self.op_e_clr(ctx);
                true
            }
            // F-register inherent unary (page3 $53–$5F)
            0x53 => {
                self.op_f_unary_inh(ctx, "COMF", |cpu, v| cpu.op_com8(v));
                true
            }
            0x5A => {
                self.op_f_unary_inh(ctx, "DECF", |cpu, v| cpu.op_dec8(v));
                true
            }
            0x5C => {
                self.op_f_unary_inh(ctx, "INCF", |cpu, v| cpu.op_inc8(v));
                true
            }
            0x5D => {
                self.op_f_tst(ctx);
                true
            }
            0x5F => {
                self.op_f_clr(ctx);
                true
            }
            // E-register ALU (page3 $80–$B7)
            0x80 => {
                self.op_e_alu8_imm(mem, ctx, "SUBE", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0x81 => {
                self.op_e_alu8_imm(mem, ctx, "CMPE", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0x86 => {
                self.op_e_ld8_imm(mem, ctx);
                true
            }
            0x8B => {
                self.op_e_alu8_imm(mem, ctx, "ADDE", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0x90 => {
                self.op_e_alu8_dir(mem, ctx, "SUBE", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0x91 => {
                self.op_e_alu8_dir(mem, ctx, "CMPE", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0x96 => {
                self.op_e_ld8_dir(mem, ctx);
                true
            }
            0x97 => {
                self.op_e_st8_dir(mem, ctx);
                true
            }
            0x9B => {
                self.op_e_alu8_dir(mem, ctx, "ADDE", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0xA0 => {
                self.op_e_alu8_idx(mem, ctx, "SUBE", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xA1 => {
                self.op_e_alu8_idx(mem, ctx, "CMPE", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xA6 => {
                self.op_e_ld8_idx(mem, ctx);
                true
            }
            0xA7 => {
                self.op_e_st8_idx(mem, ctx);
                true
            }
            0xAB => {
                self.op_e_alu8_idx(mem, ctx, "ADDE", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0xB0 => {
                self.op_e_alu8_ext(mem, ctx, "SUBE", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xB1 => {
                self.op_e_alu8_ext(mem, ctx, "CMPE", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xB6 => {
                self.op_e_ld8_ext(mem, ctx);
                true
            }
            0xB7 => {
                self.op_e_st8_ext(mem, ctx);
                true
            }
            0xBB => {
                self.op_e_alu8_ext(mem, ctx, "ADDE", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            // F-register ALU (page3 $C0–$F7)
            0xC0 => {
                self.op_f_alu8_imm(mem, ctx, "SUBF", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xC1 => {
                self.op_f_alu8_imm(mem, ctx, "CMPF", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xC6 => {
                self.op_f_ld8_imm(mem, ctx);
                true
            }
            0xCB => {
                self.op_f_alu8_imm(mem, ctx, "ADDF", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0xD0 => {
                self.op_f_alu8_dir(mem, ctx, "SUBF", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xD1 => {
                self.op_f_alu8_dir(mem, ctx, "CMPF", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xD6 => {
                self.op_f_ld8_dir(mem, ctx);
                true
            }
            0xD7 => {
                self.op_f_st8_dir(mem, ctx);
                true
            }
            0xDB => {
                self.op_f_alu8_dir(mem, ctx, "ADDF", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0xE0 => {
                self.op_f_alu8_idx(mem, ctx, "SUBF", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xE1 => {
                self.op_f_alu8_idx(mem, ctx, "CMPF", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xE6 => {
                self.op_f_ld8_idx(mem, ctx);
                true
            }
            0xE7 => {
                self.op_f_st8_idx(mem, ctx);
                true
            }
            0xEB => {
                self.op_f_alu8_idx(mem, ctx, "ADDF", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0xF0 => {
                self.op_f_alu8_ext(mem, ctx, "SUBF", |a, b, f| {
                    sub8(a, b, false, f);
                    a.wrapping_sub(b)
                });
                true
            }
            0xF1 => {
                self.op_f_alu8_ext(mem, ctx, "CMPF", |a, b, f| {
                    cmp8(a, b, f);
                    a
                });
                true
            }
            0xF6 => {
                self.op_f_ld8_ext(mem, ctx);
                true
            }
            0xF7 => {
                self.op_f_st8_ext(mem, ctx);
                true
            }
            0xFB => {
                self.op_f_alu8_ext(mem, ctx, "ADDF", |a, b, f| {
                    add8(a, b, false, f);
                    a.wrapping_add(b)
                });
                true
            }
            0x8F => {
                self.op_muld_imm(mem, ctx);
                true
            }
            0x9F => {
                self.op_muld_dir(mem, ctx);
                true
            }
            0xAF => {
                self.op_muld_idx(mem, ctx);
                true
            }
            0xBF => {
                self.op_muld_ext(mem, ctx);
                true
            }
            0x8D => {
                self.op_divd_imm(mem, ctx);
                true
            }
            0x9D => {
                self.op_divd_dir(mem, ctx);
                true
            }
            0xAD => {
                self.op_divd_idx(mem, ctx);
                true
            }
            0xBD => {
                self.op_divd_ext(mem, ctx);
                true
            }
            0x8E => {
                self.op_divq_imm(mem, ctx);
                true
            }
            0x9E => {
                self.op_divq_dir(mem, ctx);
                true
            }
            0xAE => {
                self.op_divq_idx(mem, ctx);
                true
            }
            0xBE => {
                self.op_divq_ext(mem, ctx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn get_hd6309_reg16(&self, code: u8) -> u16 {
        match code {
            0x0 => self.get_reg16(Reg16::D),
            0x1 => self.x,
            0x2 => self.y,
            0x3 => self.u,
            0x4 => self.s,
            0x5 => self.pc,
            0x6 => self.w,
            0x7 => self.v,
            _ => 0,
        }
    }

    pub(crate) fn set_hd6309_reg16(&mut self, code: u8, value: u16) {
        match code {
            0x0 => self.set_reg16(Reg16::D, value),
            0x1 => self.x = value,
            0x2 => self.y = value,
            0x3 => self.u = value,
            0x4 => self.s = value,
            0x5 => self.pc = value,
            0x6 => self.w = value,
            0x7 => self.v = value,
            _ => {}
        }
    }

    pub(crate) fn get_hd6309_reg8(&self, code: u8) -> u8 {
        match code {
            0x8 => self.a,
            0x9 => self.b,
            0xA => self.cc.bits(),
            0xB => self.dp,
            0xE => (self.w >> 8) as u8,
            0xF => self.w as u8,
            _ => 0,
        }
    }

    pub(crate) fn set_hd6309_reg8(&mut self, code: u8, value: u8) {
        match code {
            0x8 => self.a = value,
            0x9 => self.b = value,
            0xA => self.cc = Flags::from_byte(value),
            0xB => self.dp = value,
            0xE => self.w = (self.w & 0x00FF) | ((value as u16) << 8),
            0xF => self.w = (self.w & 0xFF00) | value as u16,
            _ => {}
        }
    }

    fn logic_name(opcode_low: u8) -> &'static str {
        match opcode_low {
            0x1 => "OIM",
            0x2 => "AIM",
            0x5 => "EIM",
            0xB => "TIM",
            _ => "???",
        }
    }

    fn apply_logic(opcode_low: u8, mem_val: u8, imm: u8, flags: &mut Flags) -> u8 {
        let result = match opcode_low {
            0x1 => mem_val | imm,
            0x2 => mem_val & imm,
            0x5 => mem_val ^ imm,
            0xB => {
                let r = mem_val & imm;
                flags.remove(Flags::V | Flags::C);
                flags.set_nz8(r);
                return r;
            }
            _ => mem_val,
        };
        flags.set_nz8(result);
        flags.remove(Flags::V);
        result
    }

    fn op_logic_imm_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let (addr, op) = self.addr_direct(mem, ctx);
        let imm = self.fetch_imm8(mem, ctx);
        let val = mem.read8(addr);
        let low = opcode & 0x0F;
        let result = Self::apply_logic(low, val, imm, &mut self.cc);
        if low != 0xB {
            mem.write8(addr, result);
        }
        ctx.cycles = 6;
        ctx.mnemonic = Self::logic_name(low).into();
        ctx.operands = format!("{op},#${imm:02X}");
    }

    fn op_logic_imm_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let postbyte_idx = ctx.bytes.len() - 1;
        let ea = indexed_addr(self, mem, postbyte);
        let imm = self.fetch_imm8(mem, ctx);
        let val = mem.read8(ea.addr);
        let low = opcode & 0x0F;
        let result = Self::apply_logic(low, val, imm, &mut self.cc);
        if low != 0xB {
            mem.write8(ea.addr, result);
        }
        ctx.cycles = 7 + ea.extra_cycles as u32;
        let extra = &ctx.bytes[postbyte_idx + 1..ctx.bytes.len() - 1];
        ctx.mnemonic = Self::logic_name(low).into();
        ctx.operands = format!("{},#${imm:02X}", format_index_operand(postbyte, extra));
    }

    fn op_logic_imm_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let (addr, op) = self.addr_extended(mem, ctx);
        let imm = self.fetch_imm8(mem, ctx);
        let val = mem.read8(addr);
        let low = opcode & 0x0F;
        let result = Self::apply_logic(low, val, imm, &mut self.cc);
        if low != 0xB {
            mem.write8(addr, result);
        }
        ctx.cycles = 7;
        ctx.mnemonic = Self::logic_name(low).into();
        ctx.operands = format!("{op},#${imm:02X}");
    }

    fn op_inter_reg(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src = (postbyte >> 4) & 0x0F;
        let dst = postbyte & 0x0F;
        let is_16 = postbyte & 0x08 != 0;

        let name = match opcode {
            0x30 => "ADDR",
            0x31 => "ADCR",
            0x32 => "SUBR",
            0x33 => "SBCR",
            0x34 => "ANDR",
            0x35 => "ORR",
            0x36 => "EORR",
            0x37 => "CMPR",
            _ => "???",
        };

        if is_16 {
            let a = self.get_hd6309_reg16(src & 0x07);
            let b = self.get_hd6309_reg16(dst & 0x07);
            let result = match opcode {
                0x30 => add16(a, b, &mut self.cc),
                0x31 => {
                    let c = self.cc.contains(Flags::C);
                    let sum = a as u32 + b as u32 + u32::from(c);
                    let r = sum as u16;
                    self.cc.set(Flags::C, sum > 0xFFFF);
                    self.cc.set(Flags::V, (!(a ^ b) & (a ^ r) & 0x8000) != 0);
                    self.cc.set_nz16(r);
                    r
                }
                0x32 => sub16(a, b, &mut self.cc),
                0x33 => {
                    let c = self.cc.contains(Flags::C) as u32;
                    let diff = a as u32 + 0x10000 - b as u32 - c;
                    let r = diff as u16;
                    self.cc.set(Flags::C, (a as u32) < (b as u32) + c);
                    self.cc.set(Flags::V, ((a ^ b) & (a ^ r) & 0x8000) != 0);
                    self.cc.set_nz16(r);
                    r
                }
                0x34 => {
                    let r = a & b;
                    self.cc.set_nz16(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x35 => {
                    let r = a | b;
                    self.cc.set_nz16(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x36 => {
                    let r = a ^ b;
                    self.cc.set_nz16(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x37 => {
                    cmp16(a, b, &mut self.cc);
                    a
                }
                _ => a,
            };
            if opcode != 0x37 {
                self.set_hd6309_reg16(dst & 0x07, result);
            }
        } else {
            let a = self.get_hd6309_reg8(src & 0x0F);
            let b = self.get_hd6309_reg8(dst & 0x0F);
            let result = match opcode {
                0x30 => {
                    let r = a.wrapping_add(b);
                    self.cc.set(Flags::C, r < a);
                    self.cc.set(Flags::V, ((a ^ r) & (b ^ r) & 0x80) != 0);
                    self.cc.set_nz8(r);
                    r
                }
                0x31 => {
                    let c = self.cc.contains(Flags::C);
                    let sum = a as u16 + b as u16 + u16::from(c);
                    let r = sum as u8;
                    self.cc.set(Flags::C, sum > 0xFF);
                    self.cc.set(Flags::V, ((a ^ r) & (b ^ r) & 0x80) != 0);
                    self.cc.set(Flags::H, (a ^ b ^ r) & 0x10 != 0);
                    self.cc.set_nz8(r);
                    r
                }
                0x32 => {
                    crate::alu::sub8(a, b, false, &mut self.cc);
                    a.wrapping_sub(b)
                }
                0x33 => {
                    let c = self.cc.contains(Flags::C);
                    crate::alu::sub8(a, b, c, &mut self.cc);
                    a.wrapping_sub(b).wrapping_sub(u8::from(c))
                }
                0x34 => {
                    let r = a & b;
                    self.cc.set_nz8(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x35 => {
                    let r = a | b;
                    self.cc.set_nz8(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x36 => {
                    let r = a ^ b;
                    self.cc.set_nz8(r);
                    self.cc.remove(Flags::V);
                    r
                }
                0x37 => {
                    crate::alu::cmp8(a, b, &mut self.cc);
                    a
                }
                _ => a,
            };
            if opcode != 0x37 {
                self.set_hd6309_reg8(dst & 0x0F, result);
            }
        }

        ctx.cycles = 4;
        ctx.mnemonic = name.into();
        ctx.operands = format!("${postbyte:02X}");
        let _ = mem;
    }

    fn op_tfm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let src_code = (postbyte >> 4) & 0x0F;
        let dst_code = postbyte & 0x0F;
        let src = self.get_hd6309_reg16(src_code & 0x07);
        let dst = self.get_hd6309_reg16(dst_code & 0x07);
        let count = self.w;
        let pc_before = self.pc.wrapping_sub(ctx.bytes.len() as u16);

        self.tfm_pending = Some(TfmPending {
            opcode,
            src,
            dst,
            src_code,
            dst_code,
            postbyte,
            remaining: count,
            pc_before,
            bytes: ctx.bytes.clone(),
            first_chunk: true,
        });
        self.run_tfm_chunk(mem, ctx);
    }

    pub(crate) fn run_tfm_chunk_step(&mut self, mem: &mut Memory) -> StepResult {
        // Safely extract info; if no pending (inconsistent state), return a safe no-op result
        let (pc_before, bytes, opcode) = match &self.tfm_pending {
            Some(t) => (t.pc_before, t.bytes.clone(), t.bytes.first().copied().unwrap_or(0x11)),
            None => {
                self.tfm_pending = None;
                return StepResult {
                    cycles: 0,
                    pc_before: self.pc,
                    pc_after: self.pc,
                    opcode: 0,
                    bytes: vec![],
                    mnemonic: String::new(),
                    operands: String::new(),
                    trap: None,
                };
            }
        };

        mem.clear_watchpoint_trigger();
        let mut ctx = StepCtx {
            cycles: 0,
            bytes,
            mnemonic: String::new(),
            operands: String::new(),
            trap: None,
        };
        self.run_tfm_chunk(mem, &mut ctx);
        self.total_cycles += ctx.cycles as u64;

        let trap = if mem.take_watchpoint_trigger().is_some() {
            Some(Trap::Watchpoint)
        } else {
            None
        };

        StepResult {
            cycles: ctx.cycles,
            pc_before,
            pc_after: self.pc,
            opcode,
            bytes: ctx.bytes,
            mnemonic: ctx.mnemonic,
            operands: ctx.operands,
            trap,
        }
    }

    fn run_tfm_chunk(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        // Take pending out to mutate locally without repeated expects.
        // This makes the code panic-free even if called in unexpected state.
        let mut pending = match self.tfm_pending.take() {
            Some(p) => p,
            None => {
                ctx.cycles = 0;
                return;
            }
        };

        let chunk = pending.remaining.min(TFM_CHUNK_SIZE);

        for _ in 0..chunk {
            let byte = mem.read8(pending.src);
            mem.write8(pending.dst, byte);
            match pending.opcode {
                0x38 => {
                    pending.src = pending.src.wrapping_add(1);
                    pending.dst = pending.dst.wrapping_add(1);
                }
                0x39 => {
                    pending.src = pending.src.wrapping_sub(1);
                    pending.dst = pending.dst.wrapping_sub(1);
                }
                0x3A => {
                    pending.src = pending.src.wrapping_add(1);
                }
                0x3B => {
                    pending.dst = pending.dst.wrapping_add(1);
                }
                _ => {}
            }
            pending.remaining -= 1;
        }

        self.w = pending.remaining;
        let first_chunk = pending.first_chunk;
        pending.first_chunk = false;

        ctx.cycles = if first_chunk {
            6 + chunk as u32 * 3
        } else {
            chunk as u32 * 3
        };
        ctx.mnemonic = tfm_mnemonic(pending.opcode).into();
        ctx.operands = format!("${:02X}", pending.postbyte);

        if pending.remaining == 0 {
            // done: apply final registers, do not put back
            self.set_hd6309_reg16(pending.src_code & 0x07, pending.src);
            self.set_hd6309_reg16(pending.dst_code & 0x07, pending.dst);
            self.w = 0;
            // tfm_pending stays None
        } else {
            self.tfm_pending = Some(pending);
        }
    }

    fn op_pshsw(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        self.push16(mem, self.w);
        ctx.cycles = 6;
        ctx.mnemonic = "PSHSW".into();
        ctx.operands.clear();
    }

    fn op_pulsw(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        self.w = self.pull16(mem);
        ctx.cycles = 6;
        ctx.mnemonic = "PULSW".into();
        ctx.operands.clear();
    }

    fn op_pshuw(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        self.push16_u(mem, self.w);
        ctx.cycles = 6;
        ctx.mnemonic = "PSHUW".into();
        ctx.operands.clear();
    }

    fn op_puluw(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        self.w = self.pull16_u(mem);
        ctx.cycles = 6;
        ctx.mnemonic = "PULUW".into();
        ctx.operands.clear();
    }

    fn exec_muld_product(&mut self, multiplicand: u16) {
        let d = self.get_reg16(Reg16::D) as i32;
        let product = d.wrapping_mul(multiplicand as i32) as u32;
        self.set_q(product);
        self.cc.remove(Flags::C | Flags::V);
        self.cc.set_nz32(product);
    }

    pub(crate) fn op_sexw(&mut self, ctx: &mut StepCtx) {
        let d = if self.w & 0x8000 != 0 {
            0xFFFF
        } else {
            0x0000
        };
        self.set_reg16(Reg16::D, d);
        self.cc.set_nz16(self.get_reg16(Reg16::D));
        ctx.cycles = 3;
        ctx.mnemonic = "SEXW".into();
        ctx.operands.clear();
    }

    fn get_q(&self) -> u32 {
        ((self.get_reg16(Reg16::D) as u32) << 16) | u32::from(self.w)
    }

    fn set_q(&mut self, value: u32) {
        self.set_reg16(Reg16::D, (value >> 16) as u16);
        self.w = value as u16;
    }

    fn load_q_from_addr(&mut self, mem: &Memory, addr: u16) {
        let d = mem.read16(addr);
        let w = mem.read16(addr.wrapping_add(2));
        self.set_reg16(Reg16::D, d);
        self.w = w;
        self.cc.set_nz32(self.get_q());
    }

    fn store_q_to_addr(&mut self, mem: &mut Memory, addr: u16) {
        mem.write16(addr, self.get_reg16(Reg16::D));
        mem.write16(addr.wrapping_add(2), self.w);
        let q = self.get_q();
        self.cc.remove(Flags::V);
        self.cc.set_nz32(q);
    }

    pub(crate) fn op_ldq_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let d = self.fetch_imm16(mem, ctx);
        let w = self.fetch_imm16(mem, ctx);
        self.set_reg16(Reg16::D, d);
        self.w = w;
        self.cc.set_nz32(self.get_q());
        ctx.cycles = 7;
        ctx.mnemonic = "LDQ".into();
        ctx.operands = format!("#${:08X}", self.get_q());
    }

    fn op_ldq_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        self.load_q_from_addr(mem, addr);
        ctx.cycles = 9;
        ctx.mnemonic = "LDQ".into();
        ctx.operands = op_str;
    }

    fn op_ldq_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        self.load_q_from_addr(mem, addr);
        ctx.cycles = 9 + extra as u32;
        ctx.mnemonic = "LDQ".into();
        ctx.operands = op_str;
    }

    fn op_ldq_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        self.load_q_from_addr(mem, addr);
        ctx.cycles = 10;
        ctx.mnemonic = "LDQ".into();
        ctx.operands = op_str;
    }

    fn op_stq_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        self.store_q_to_addr(mem, addr);
        ctx.cycles = 9;
        ctx.mnemonic = "STQ".into();
        ctx.operands = op_str;
    }

    fn op_stq_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        self.store_q_to_addr(mem, addr);
        ctx.cycles = 9 + extra as u32;
        ctx.mnemonic = "STQ".into();
        ctx.operands = op_str;
    }

    fn op_stq_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        self.store_q_to_addr(mem, addr);
        ctx.cycles = 10;
        ctx.mnemonic = "STQ".into();
        ctx.operands = op_str;
    }

    fn op_muld_inh(&mut self, ctx: &mut StepCtx) {
        let d = self.get_reg16(Reg16::D);
        self.exec_muld_product(d);
        ctx.cycles = 28;
        ctx.mnemonic = "MULD".into();
        ctx.operands.clear();
    }

    fn op_muld_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm16(mem, ctx);
        self.exec_muld_product(val);
        ctx.cycles = 25;
        ctx.mnemonic = "MULD".into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_muld_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        self.exec_muld_product(mem.read16(addr));
        ctx.cycles = 27;
        ctx.mnemonic = "MULD".into();
        ctx.operands = op_str;
    }

    fn op_muld_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        self.exec_muld_product(mem.read16(addr));
        ctx.cycles = 27 + extra as u32;
        ctx.mnemonic = "MULD".into();
        ctx.operands = op_str;
    }

    fn op_muld_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        self.exec_muld_product(mem.read16(addr));
        ctx.cycles = 28;
        ctx.mnemonic = "MULD".into();
        ctx.operands = op_str;
    }

    fn get_e(&self) -> u8 {
        (self.w >> 8) as u8
    }

    fn get_f(&self) -> u8 {
        self.w as u8
    }

    fn set_e(&mut self, value: u8) {
        self.w = (self.w & 0x00FF) | ((value as u16) << 8);
    }

    fn set_f(&mut self, value: u8) {
        self.w = (self.w & 0xFF00) | u16::from(value);
    }

    fn op_d_unary_inh<F>(&mut self, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16) -> u16,
    {
        let d = self.get_reg16(Reg16::D);
        let result = op(self, d);
        self.set_reg16(Reg16::D, result);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands.clear();
    }

    fn op_w_unary_inh<F>(&mut self, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16) -> u16,
    {
        self.w = op(self, self.w);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands.clear();
    }

    fn op_e_unary_inh<F>(&mut self, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u8) -> u8,
    {
        let e = self.get_e();
        let result = op(self, e);
        self.set_e(result);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands.clear();
    }

    fn op_f_unary_inh<F>(&mut self, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u8) -> u8,
    {
        let f = self.get_f();
        let result = op(self, f);
        self.set_f(result);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands.clear();
    }

    fn op_e_tst(&mut self, ctx: &mut StepCtx) {
        self.cc.remove(Flags::V | Flags::C);
        self.cc.set_nz8(self.get_e());
        ctx.cycles = 3;
        ctx.mnemonic = "TSTE".into();
        ctx.operands.clear();
    }

    fn op_f_tst(&mut self, ctx: &mut StepCtx) {
        self.cc.remove(Flags::V | Flags::C);
        self.cc.set_nz8(self.get_f());
        ctx.cycles = 3;
        ctx.mnemonic = "TSTF".into();
        ctx.operands.clear();
    }

    fn op_e_clr(&mut self, ctx: &mut StepCtx) {
        self.set_e(0);
        self.cc.remove(Flags::V | Flags::C);
        self.cc.insert(Flags::Z);
        self.cc.remove(Flags::N);
        ctx.cycles = 3;
        ctx.mnemonic = "CLRE".into();
        ctx.operands.clear();
    }

    fn op_f_clr(&mut self, ctx: &mut StepCtx) {
        self.set_f(0);
        self.cc.remove(Flags::V | Flags::C);
        self.cc.insert(Flags::Z);
        self.cc.remove(Flags::N);
        ctx.cycles = 3;
        ctx.mnemonic = "CLRF".into();
        ctx.operands.clear();
    }

    fn op_d_alu16_imm<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let val = self.fetch_imm16(mem, ctx);
        let d = self.get_reg16(Reg16::D);
        let result = op(self, d, val);
        self.set_reg16(Reg16::D, result);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_d_alu16_dir<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read16(addr);
        let d = self.get_reg16(Reg16::D);
        let result = op(self, d, val);
        self.set_reg16(Reg16::D, result);
        ctx.cycles = 7;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_d_alu16_idx<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read16(addr);
        let d = self.get_reg16(Reg16::D);
        let result = op(self, d, val);
        self.set_reg16(Reg16::D, result);
        ctx.cycles = 7 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_d_alu16_ext<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read16(addr);
        let d = self.get_reg16(Reg16::D);
        let result = op(self, d, val);
        self.set_reg16(Reg16::D, result);
        ctx.cycles = 8;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_d_bit16_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str) {
        let val = self.fetch_imm16(mem, ctx);
        bit16(self.get_reg16(Reg16::D), val, &mut self.cc);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_d_bit16_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        bit16(self.get_reg16(Reg16::D), mem.read16(addr), &mut self.cc);
        ctx.cycles = 7;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_d_bit16_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        bit16(self.get_reg16(Reg16::D), mem.read16(addr), &mut self.cc);
        ctx.cycles = 7 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_d_bit16_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        bit16(self.get_reg16(Reg16::D), mem.read16(addr), &mut self.cc);
        ctx.cycles = 8;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_e_alu8_imm<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let val = self.fetch_imm8(mem, ctx);
        let result = op(self.get_e(), val, &mut self.cc);
        self.set_e(result);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${val:02X}");
    }

    fn op_e_alu8_dir<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_e(), val, &mut self.cc);
        self.set_e(result);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_e_alu8_idx<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_e(), val, &mut self.cc);
        self.set_e(result);
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_e_alu8_ext<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_e(), val, &mut self.cc);
        self.set_e(result);
        ctx.cycles = 6;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_e_ld8_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm8(mem, ctx);
        self.set_e(val);
        self.cc.set_nz8(val);
        ctx.cycles = 3;
        ctx.mnemonic = "LDE".into();
        ctx.operands = format!("#${val:02X}");
    }

    fn op_e_ld8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read8(addr);
        self.set_e(val);
        self.cc.set_nz8(val);
        ctx.cycles = 5;
        ctx.mnemonic = "LDE".into();
        ctx.operands = op_str;
    }

    fn op_e_ld8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read8(addr);
        self.set_e(val);
        self.cc.set_nz8(val);
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = "LDE".into();
        ctx.operands = op_str;
    }

    fn op_e_ld8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read8(addr);
        self.set_e(val);
        self.cc.set_nz8(val);
        ctx.cycles = 6;
        ctx.mnemonic = "LDE".into();
        ctx.operands = op_str;
    }

    fn op_e_st8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        mem.write8(addr, self.get_e());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_e());
        ctx.cycles = 5;
        ctx.mnemonic = "STE".into();
        ctx.operands = op_str;
    }

    fn op_e_st8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        mem.write8(addr, self.get_e());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_e());
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = "STE".into();
        ctx.operands = op_str;
    }

    fn op_e_st8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        mem.write8(addr, self.get_e());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_e());
        ctx.cycles = 6;
        ctx.mnemonic = "STE".into();
        ctx.operands = op_str;
    }

    fn op_f_alu8_imm<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let val = self.fetch_imm8(mem, ctx);
        let result = op(self.get_f(), val, &mut self.cc);
        self.set_f(result);
        ctx.cycles = 3;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${val:02X}");
    }

    fn op_f_alu8_dir<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_f(), val, &mut self.cc);
        self.set_f(result);
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_f_alu8_idx<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_f(), val, &mut self.cc);
        self.set_f(result);
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_f_alu8_ext<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: Fn(u8, u8, &mut Flags) -> u8,
    {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read8(addr);
        let result = op(self.get_f(), val, &mut self.cc);
        self.set_f(result);
        ctx.cycles = 6;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_f_ld8_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm8(mem, ctx);
        self.set_f(val);
        self.cc.set_nz8(val);
        ctx.cycles = 3;
        ctx.mnemonic = "LDF".into();
        ctx.operands = format!("#${val:02X}");
    }

    fn op_f_ld8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read8(addr);
        self.set_f(val);
        self.cc.set_nz8(val);
        ctx.cycles = 5;
        ctx.mnemonic = "LDF".into();
        ctx.operands = op_str;
    }

    fn op_f_ld8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read8(addr);
        self.set_f(val);
        self.cc.set_nz8(val);
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = "LDF".into();
        ctx.operands = op_str;
    }

    fn op_f_ld8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read8(addr);
        self.set_f(val);
        self.cc.set_nz8(val);
        ctx.cycles = 6;
        ctx.mnemonic = "LDF".into();
        ctx.operands = op_str;
    }

    fn op_f_st8_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        mem.write8(addr, self.get_f());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_f());
        ctx.cycles = 5;
        ctx.mnemonic = "STF".into();
        ctx.operands = op_str;
    }

    fn op_f_st8_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        mem.write8(addr, self.get_f());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_f());
        ctx.cycles = 5 + extra as u32;
        ctx.mnemonic = "STF".into();
        ctx.operands = op_str;
    }

    fn op_f_st8_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        mem.write8(addr, self.get_f());
        self.cc.remove(Flags::V);
        self.cc.set_nz8(self.get_f());
        ctx.cycles = 6;
        ctx.mnemonic = "STF".into();
        ctx.operands = op_str;
    }

    fn bit_reg_value(&self, code: u8) -> u8 {
        match code {
            0 => self.a,
            1 => self.b,
            2 => self.cc.bits(),
            _ => 0,
        }
    }

    fn op_bit_transfer(&mut self, mem: &mut Memory, ctx: &mut StepCtx, opcode: u8) {
        let postbyte = self.fetch_imm8(mem, ctx);
        let addr_byte = self.fetch_imm8(mem, ctx);
        let addr = (self.dp as u16) << 8 | addr_byte as u16;

        let reg_code = (postbyte >> 6) & 0x03;
        let reg_bit = (postbyte >> 3) & 0x07;
        let mem_bit = postbyte & 0x07;

        let reg_val = self.bit_reg_value(reg_code);
        let r_bit = (reg_val >> reg_bit) & 1;
        let mut mem_val = mem.read8(addr);
        let m_bit = (mem_val >> mem_bit) & 1;

        let result_bit = match opcode {
            0x30 => r_bit & m_bit,
            0x31 => r_bit & !m_bit,
            0x32 => r_bit | m_bit,
            0x33 => r_bit | !m_bit,
            0x34 => r_bit ^ m_bit,
            0x35 => r_bit ^ !m_bit,
            0x36 => m_bit,
            0x37 => {
                mem_val = (mem_val & !(1 << mem_bit)) | (r_bit << mem_bit);
                mem.write8(addr, mem_val);
                ctx.cycles = 6;
                ctx.mnemonic = "STBT".into();
                ctx.operands = format!(
                    "{},{},{},<${addr_byte:02X}",
                    ["A", "B", "CC"][reg_code as usize],
                    reg_bit,
                    mem_bit
                );
                return;
            }
            _ => m_bit,
        };

        if opcode == 0x36 {
            let mask = 1u8 << reg_bit;
            let new_reg = (reg_val & !mask) | (m_bit << reg_bit);
            match reg_code {
                0 => self.a = new_reg,
                1 => self.b = new_reg,
                2 => self.cc = Flags::from_byte(new_reg),
                _ => {}
            }
            ctx.cycles = 6;
            ctx.mnemonic = "LDBT".into();
            ctx.operands = format!(
                "{},{},{},<${addr_byte:02X}",
                ["A", "B", "CC"][reg_code as usize],
                reg_bit,
                mem_bit
            );
            return;
        }

        mem_val = (mem_val & !(1 << mem_bit)) | (result_bit << mem_bit);
        mem.write8(addr, mem_val);

        let name = match opcode {
            0x30 => "BAND",
            0x31 => "BIAND",
            0x32 => "BOR",
            0x33 => "BIOR",
            0x34 => "BEOR",
            0x35 => "BIEOR",
            _ => "BIT",
        };

        ctx.cycles = 6;
        ctx.mnemonic = name.into();
        ctx.operands = format!(
            "{},{},{},<${addr_byte:02X}",
            ["A", "B", "CC"][reg_code as usize],
            reg_bit,
            mem_bit
        );
    }

    fn op_alu16_imm<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let val = self.fetch_imm16(mem, ctx);
        let result = op(self, self.w, val);
        if name != "CMPW" {
            self.w = result;
        }
        ctx.cycles = 5;
        ctx.mnemonic = name.into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_alu16_dir<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read16(addr);
        let result = op(self, self.w, val);
        if name != "CMPW" {
            self.w = result;
        }
        ctx.cycles = 7;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_alu16_idx<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let postbyte = self.fetch_imm8(mem, ctx);
        let ea = indexed_addr(self, mem, postbyte);
        let val = mem.read16(ea.addr);
        let result = op(self, self.w, val);
        if name != "CMPW" {
            self.w = result;
        }
        ctx.cycles = 7 + ea.extra_cycles as u32;
        ctx.mnemonic = name.into();
        ctx.operands = format!("${postbyte:02X}");
    }

    fn op_alu16_ext<F>(&mut self, mem: &mut Memory, ctx: &mut StepCtx, name: &str, op: F)
    where
        F: FnOnce(&mut Cpu, u16, u16) -> u16,
    {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read16(addr);
        let result = op(self, self.w, val);
        if name != "CMPW" {
            self.w = result;
        }
        ctx.cycles = 8;
        ctx.mnemonic = name.into();
        ctx.operands = op_str;
    }

    fn op_ldw_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm16(mem, ctx);
        self.w = val;
        self.cc.set_nz16(val);
        ctx.cycles = 5;
        ctx.mnemonic = "LDW".into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_ldw_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        let val = mem.read16(addr);
        self.w = val;
        self.cc.set_nz16(val);
        ctx.cycles = 7;
        ctx.mnemonic = "LDW".into();
        ctx.operands = op_str;
    }

    fn op_ldw_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        let val = mem.read16(addr);
        self.w = val;
        self.cc.set_nz16(val);
        ctx.cycles = 7 + extra as u32;
        ctx.mnemonic = "LDW".into();
        ctx.operands = op_str;
    }

    fn op_ldw_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        let val = mem.read16(addr);
        self.w = val;
        self.cc.set_nz16(val);
        ctx.cycles = 8;
        ctx.mnemonic = "LDW".into();
        ctx.operands = op_str;
    }

    fn op_stw_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        mem.write16(addr, self.w);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(self.w);
        ctx.cycles = 7;
        ctx.mnemonic = "STW".into();
        ctx.operands = op_str;
    }

    fn op_stw_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        mem.write16(addr, self.w);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(self.w);
        ctx.cycles = 7 + extra as u32;
        ctx.mnemonic = "STW".into();
        ctx.operands = op_str;
    }

    fn op_stw_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        mem.write16(addr, self.w);
        self.cc.remove(Flags::V);
        self.cc.set_nz16(self.w);
        ctx.cycles = 8;
        ctx.mnemonic = "STW".into();
        ctx.operands = op_str;
    }

    fn op_divd_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm16(mem, ctx);
        self.exec_divd(mem, val);
        ctx.cycles = 25;
        ctx.mnemonic = "DIVD".into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_divd_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        self.exec_divd(mem, mem.read16(addr));
        ctx.cycles = 27;
        ctx.mnemonic = "DIVD".into();
        ctx.operands = op_str;
    }

    fn op_divd_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        self.exec_divd(mem, mem.read16(addr));
        ctx.cycles = 27 + extra as u32;
        ctx.mnemonic = "DIVD".into();
        ctx.operands = op_str;
    }

    fn op_divd_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        self.exec_divd(mem, mem.read16(addr));
        ctx.cycles = 28;
        ctx.mnemonic = "DIVD".into();
        ctx.operands = op_str;
    }

    fn op_divq_imm(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let val = self.fetch_imm16(mem, ctx);
        self.exec_divq(mem, val);
        ctx.cycles = 34;
        ctx.mnemonic = "DIVQ".into();
        ctx.operands = format!("#${val:04X}");
    }

    fn op_divq_dir(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_direct(mem, ctx);
        self.exec_divq(mem, mem.read16(addr));
        ctx.cycles = 36;
        ctx.mnemonic = "DIVQ".into();
        ctx.operands = op_str;
    }

    fn op_divq_idx(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, extra, op_str) = self.addr_indexed(mem, ctx);
        self.exec_divq(mem, mem.read16(addr));
        ctx.cycles = 36 + extra as u32;
        ctx.mnemonic = "DIVQ".into();
        ctx.operands = op_str;
    }

    fn op_divq_ext(&mut self, mem: &mut Memory, ctx: &mut StepCtx) {
        let (addr, op_str) = self.addr_extended(mem, ctx);
        self.exec_divq(mem, mem.read16(addr));
        ctx.cycles = 37;
        ctx.mnemonic = "DIVQ".into();
        ctx.operands = op_str;
    }

    fn exec_divd(&mut self, mem: &mut Memory, divisor: u16) {
        if divisor == 0 {
            self.enter_hw_trap(mem, 0x80);
            return;
        }
        let dividend = self.get_reg16(Reg16::D) as i32;
        let div = divisor as i32;
        let quotient = dividend / div;
        let remainder = dividend % div;
        if !(-32768..=32767).contains(&quotient) {
            self.cc.insert(Flags::C);
            return;
        }
        self.w = quotient as u16;
        self.set_reg16(Reg16::D, remainder as u16);
        self.cc.remove(Flags::C);
        self.cc.set_nz16(self.w);
    }

    fn exec_divq(&mut self, mem: &mut Memory, divisor: u16) {
        if divisor == 0 {
            self.enter_hw_trap(mem, 0x80);
            return;
        }
        let dividend: i32 =
            (((self.get_reg16(Reg16::D) as u32) << 16) | u32::from(self.w)) as i32;
        let div = divisor as i16 as i32;
        if div == -1 && dividend == i32::MIN {
            self.cc.insert(Flags::C);
            return;
        }
        let quotient = dividend / div;
        let remainder = dividend % div;
        if !(-32768..=32767).contains(&quotient) {
            self.cc.insert(Flags::C);
            return;
        }
        self.w = quotient as u16;
        self.set_reg16(Reg16::D, remainder as u16);
        self.cc.remove(Flags::C);
        self.cc.set_nz16(quotient as u16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::Reg16;
    use crate::memory::Memory;

    fn mem_with_program(pc: u16, bytes: &[u8]) -> Memory {
        let mut mem = Memory::new();
        let _ = mem.load_binary(pc, bytes);
        mem
    }

    #[test]
    fn negd_operates_on_d_not_w() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0x00;
        cpu.b = 0x05;
        cpu.w = 0x1234;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x40]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NEGD");
        assert_eq!(cpu.get_reg16(Reg16::D), 0xFFFB);
        assert_eq!(cpu.w, 0x1234);
    }

    #[test]
    fn negw_inherent_on_page2() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0x0005;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x50]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "NEGW");
        assert_eq!(cpu.w, 0xFFFB);
    }

    #[test]
    fn andd_imm_masks_d_register() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0xFF;
        cpu.b = 0x0F;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x84, 0x00, 0xF0]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "ANDD");
        assert_eq!(cpu.get_reg16(Reg16::D), 0x0000);
    }

    #[test]
    fn lde_imm_loads_high_byte_of_w() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0x0000;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x86, 0xAB]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LDE");
        assert_eq!(cpu.w, 0xAB00);
    }

    #[test]
    fn ldf_imm_loads_low_byte_of_w() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0x0000;
        let mut mem = mem_with_program(0x0100, &[0x11, 0xC6, 0xCD]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LDF");
        assert_eq!(cpu.w, 0x00CD);
    }

    #[test]
    fn come_complements_e_register() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0x5500;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x43]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "COME");
        assert_eq!(cpu.w, 0xAA00);
    }

    #[test]
    fn leax_works_on_6809() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.x = 0x0200;
        let mut mem = mem_with_program(0x0100, &[0x30, 0x05]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LEAX");
        assert_eq!(cpu.x, 0x0205);
        assert_eq!(step.trap, None);
    }

    #[test]
    fn leax_works_on_6309() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.x = 0x0200;
        let mut mem = mem_with_program(0x0100, &[0x30, 0x05]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LEAX");
        assert_eq!(cpu.x, 0x0205);
    }

    #[test]
    fn muld_on_6309() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0;
        cpu.b = 10;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x3E]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "MULD");
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
        assert_eq!(cpu.w, 100);
    }

    #[test]
    fn divd_stores_quotient_in_w() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0;
        cpu.b = 20;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x8D, 0x00, 0x04]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVD");
        assert_eq!(cpu.w, 5);
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
    }

    #[test]
    fn aim_on_6309() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.dp = 0;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x02, 0x20, 0xF0]);
        mem.write8(0x0020, 0xFF);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "AIM");
        assert_eq!(mem.read8(0x0020), 0xF0);
    }

    #[test]
    fn bor_bit_transfer() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0x02;
        cpu.dp = 0;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x32, 0x0F, 0x20]);
        mem.write8(0x0020, 0x00);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "BOR");
        assert_eq!(mem.read8(0x0020), 0x80);
    }

    #[test]
    fn bitmd_clears_trap_bits() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.mode_reg = 0xC0;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x3C, 0xFF]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "BITMD");
        assert_eq!(cpu.mode_reg & 0xC0, 0);
    }

    #[test]
    fn pshsw_pulsw_roundtrip() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0xBEEF;
        cpu.s = 0x0200;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x38, 0x10, 0x39]);
        let push = cpu.step(&mut mem);
        assert_eq!(push.mnemonic, "PSHSW");
        assert_eq!(mem.read8(0x01FF), 0xEF);
        assert_eq!(mem.read8(0x01FE), 0xBE);
        cpu.w = 0;
        let pull = cpu.step(&mut mem);
        assert_eq!(pull.mnemonic, "PULSW");
        assert_eq!(cpu.w, 0xBEEF);
    }

    #[test]
    fn muld_with_immediate_operand() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0;
        cpu.b = 10;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x8F, 0x00, 0x0A]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "MULD");
        assert_eq!(cpu.w, 100);
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
    }

    #[test]
    fn tfm_on_page3() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.x = 0x600;
        cpu.y = 0x700;
        cpu.w = 2;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x38, 0x12]);
        mem.write8(0x600, 0x41);
        mem.write8(0x601, 0x42);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "TFM+");
        assert_eq!(mem.read8(0x700), 0x41);
        assert_eq!(mem.read8(0x701), 0x42);
        assert_eq!(cpu.w, 0);
        assert!(cpu.tfm_pending.is_none());
    }

    #[test]
    fn tfm_large_transfer_completes_across_chunks() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.x = 0x1000;
        cpu.y = 0x2000;
        cpu.w = 600;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x38, 0x12]);
        for i in 0..600u16 {
            mem.write8(0x1000 + i, (i & 0xFF) as u8);
        }

        let step1 = cpu.step(&mut mem);
        assert_eq!(step1.mnemonic, "TFM+");
        assert!(cpu.tfm_pending.is_some());
        assert_eq!(cpu.w, 600 - TFM_CHUNK_SIZE);

        let step2 = cpu.step(&mut mem);
        assert_eq!(step2.mnemonic, "TFM+");
        assert!(cpu.tfm_pending.is_none());
        assert_eq!(cpu.w, 0);
        assert_eq!(cpu.x, 0x1000 + 600);
        assert_eq!(cpu.y, 0x2000 + 600);
        assert_eq!(mem.read8(0x2000), 0);
        assert_eq!(mem.read8(0x2000 + 599), (599 & 0xFF) as u8);
    }

    #[test]
    fn sexw_sign_extends_w_into_d() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0x8001;
        let mut mem = mem_with_program(0x0100, &[0x14]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "SEXW");
        assert_eq!(cpu.get_reg16(Reg16::D), 0xFFFF);
        assert_eq!(cpu.w, 0x8001);
    }

    #[test]
    fn ldq_imm_loads_q_register() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        let mut mem = mem_with_program(0x0100, &[0xCD, 0x00, 0x01, 0x00, 0x02]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "LDQ");
        assert_eq!(cpu.get_reg16(Reg16::D), 0x0001);
        assert_eq!(cpu.w, 0x0002);
    }

    #[test]
    fn stq_stores_q_to_memory() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0x12;
        cpu.b = 0x34;
        cpu.w = 0x5678;
        cpu.dp = 0;
        let mut mem = mem_with_program(0x0100, &[0x10, 0xDD, 0x20]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "STQ");
        assert_eq!(mem.read16(0x0020), 0x1234);
        assert_eq!(mem.read16(0x0022), 0x5678);
    }

    #[test]
    fn pshuw_puluw_roundtrip() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.w = 0xCAFE;
        cpu.u = 0x0300;
        let mut mem = mem_with_program(0x0100, &[0x10, 0x3A, 0x10, 0x3B]);
        let push = cpu.step(&mut mem);
        assert_eq!(push.mnemonic, "PSHUW");
        assert_eq!(mem.read8(0x02FF), 0xFE);
        assert_eq!(mem.read8(0x02FE), 0xCA);
        cpu.w = 0;
        let pull = cpu.step(&mut mem);
        assert_eq!(pull.mnemonic, "PULUW");
        assert_eq!(cpu.w, 0xCAFE);
    }

    #[test]
    fn divd_zero_traps_to_fff0() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x8D, 0x00, 0x00]);
        mem.write16(0xFFF0, 0x0600);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVD");
        assert_eq!(cpu.pc, 0x0600);
        assert_ne!(cpu.mode_reg & 0x80, 0);
    }

    #[test]
    fn divq_divides_q_register() {
        let mut cpu = Cpu::new();
        cpu.pc = 0x0100;
        cpu.variant = CpuVariant::Hd6309;
        cpu.a = 0;
        cpu.b = 0;
        cpu.w = 0x0020;
        let mut mem = mem_with_program(0x0100, &[0x11, 0x8E, 0x00, 0x04]);
        let step = cpu.step(&mut mem);
        assert_eq!(step.mnemonic, "DIVQ");
        assert_eq!(cpu.w, 8);
        assert_eq!(cpu.get_reg16(Reg16::D), 0);
    }
}