use crate::cpu::Cpu;
use crate::memory::Memory;
use crate::types::CpuVariant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrMode {
    Inherent,
    Immediate,
    Direct,
    Extended,
    Relative8,
    Relative16,
    Indexed,
}

#[derive(Debug, Clone)]
pub struct EffectiveAddress {
    pub mode: AddrMode,
    pub addr: u16,
    pub extra_cycles: u8,
    pub postbyte: Option<u8>,
    pub index_reg: Option<char>,
}

pub fn direct_addr(cpu: &Cpu, mem: &Memory) -> u16 {
    let offset = mem.read8(cpu.pc) as u16;
    (cpu.dp as u16) << 8 | offset
}

pub fn extended_addr(mem: &Memory, pc: u16) -> u16 {
    mem.read16(pc)
}

pub fn relative8(mem: &Memory, pc: u16) -> u16 {
    let offset = mem.read8(pc) as i8 as i16;
    pc.wrapping_add(1).wrapping_add(offset as u16)
}

pub fn relative16(mem: &Memory, pc: u16) -> u16 {
    let offset = mem.read16(pc) as i16 as i32;
    pc.wrapping_add(2).wrapping_add(offset as u16)
}

/// Index register selector from postbyte bits 6-5.
fn index_reg_char(reg_bits: u8) -> char {
    match reg_bits {
        0 => 'X',
        1 => 'Y',
        2 => 'U',
        3 => 'S',
        _ => 'X',
    }
}

/// Decode the indexed addressing mode from a postbyte.
///
/// Based on the M6809 Programming Manual postbyte layout:
/// - Bits 6-5: Register (00=X, 01=Y, 10=U, 11=S)
/// - Bit 7: Extended mode flag (0 = 5-bit constant, 1 = special mode)
/// - Bit 4: Indirect flag (when bit 7=1)
/// - Bits 3-0: Mode sub-type (when bit 7=1)
/// - PCR is encoded in mode bits (0x8c/0x8d), not register field
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    /// 5-bit signed constant offset (-16..+15), no extra bytes
    Const5(i8),
    /// ,R (zero offset)
    ZeroOffset,
    /// ,R+ (auto-increment 1, direct only)
    AutoInc1,
    /// ,R++ (auto-increment 2)
    AutoInc2,
    /// ,-R (auto-decrement 1, direct only)
    AutoDec1,
    /// ,--R (auto-decrement 2)
    AutoDec2,
    /// B,R (accumulator B offset)
    AccB,
    /// A,R (accumulator A offset)
    AccA,
    /// D,R (accumulator D offset)
    AccD,
    /// n8,R (signed 8-bit offset, 1 extra byte)
    Off8Signed,
    /// n16,R (signed 16-bit offset, 2 extra bytes)
    Off16Signed,
    /// n8,PCR (8-bit PCR offset, 1 extra byte)
    Pcr8,
    /// n16,PCR (16-bit PCR offset, 2 extra bytes)
    Pcr16,
    /// [address] (indirect extended, 2 extra bytes)
    IndirectExtended,
    /// E,R (6309 only)
    AccE,
    /// F,R (6309 only)
    AccF,
    /// W,R (6309 only)
    AccW,
}

/// Whether this mode is indirect.
fn is_indirect(postbyte: u8) -> bool {
    (postbyte & 0x90) == 0x90
}

/// Decode the mode from a postbyte, returning (IndexMode, is_indirect, extra_bytes, reg_bits).
pub fn decode_index_mode(postbyte: u8) -> (IndexMode, bool, u8, u8) {
    let reg_bits = (postbyte >> 5) & 3;
    let indirect = is_indirect(postbyte);
    let pbm = postbyte & 0x8f;

    if pbm < 0x80 {
        // 5-bit signed constant: bits 4-0
        let off = (postbyte & 0x1f) as i8;
        let off = if off > 15 { off - 32 } else { off };
        return (IndexMode::Const5(off), false, 0, reg_bits);
    }

    let mode = match pbm {
        0x80 => IndexMode::AutoInc1,
        0x81 => IndexMode::AutoInc2,
        0x82 => IndexMode::AutoDec1,
        0x83 => IndexMode::AutoDec2,
        0x84 => IndexMode::ZeroOffset,
        0x85 => IndexMode::AccB,
        0x86 => IndexMode::AccA,
        0x87 => IndexMode::AccE,
        0x88 => IndexMode::Off8Signed,
        0x89 => IndexMode::Off16Signed,
        0x8a => IndexMode::AccF,
        0x8b => IndexMode::AccD,
        0x8c => IndexMode::Pcr8,
        0x8d => IndexMode::Pcr16,
        0x8e => IndexMode::AccW,
        0x8f => IndexMode::IndirectExtended,
        _ => IndexMode::ZeroOffset,
    };

    let extra_bytes = match mode {
        IndexMode::Off8Signed | IndexMode::Pcr8 => 1,
        IndexMode::Off16Signed | IndexMode::Pcr16 | IndexMode::IndirectExtended => 2,
        _ => 0,
    };

    (mode, indirect, extra_bytes, reg_bits)
}

/// Compute extra cycles for an indexed mode (M6809 cycle counts).
fn index_extra_cycles(mode: IndexMode, indirect: bool) -> u8 {
    let base = match mode {
        IndexMode::Const5(_) | IndexMode::ZeroOffset => 0,
        IndexMode::AutoInc1 | IndexMode::AutoDec1 => 2,
        IndexMode::AutoInc2 | IndexMode::AutoDec2 => 3,
        IndexMode::AccA | IndexMode::AccB | IndexMode::AccE | IndexMode::AccF
        | IndexMode::AccW | IndexMode::AccD => 1,
        IndexMode::Off8Signed | IndexMode::Pcr8 => 1,
        IndexMode::Off16Signed | IndexMode::Pcr16 => 2,
        IndexMode::IndirectExtended => 2,
    };
    if indirect {
        base + 3
    } else {
        base
    }
}

/// Format an indexed operand for disassembly / trace output.
pub fn format_index_operand(postbyte: u8, extra: &[u8]) -> String {
    let (mode, indirect, _, reg_bits) = decode_index_mode(postbyte);

    // For PCR modes, the "register" is PCR
    let reg_str: &str = match mode {
        IndexMode::Pcr8 | IndexMode::Pcr16 => "PCR",
        _ => match reg_bits {
            0 => "X",
            1 => "Y",
            2 => "U",
            3 => "S",
            _ => "X",
        },
    };

    let inner = match mode {
        IndexMode::Const5(off) => format!("{off},{reg_str}"),
        IndexMode::ZeroOffset => format!(",{reg_str}"),
        IndexMode::AutoInc1 => format!(",{reg_str}+"),
        IndexMode::AutoInc2 => format!(",{reg_str}++"),
        IndexMode::AutoDec1 => format!(",-{reg_str}"),
        IndexMode::AutoDec2 => format!(",--{reg_str}"),
        IndexMode::AccB => format!("B,{reg_str}"),
        IndexMode::AccA => format!("A,{reg_str}"),
        IndexMode::AccD => format!("D,{reg_str}"),
        IndexMode::AccE => format!("E,{reg_str}"),
        IndexMode::AccF => format!("F,{reg_str}"),
        IndexMode::AccW => format!("W,{reg_str}"),
        IndexMode::Off8Signed if !extra.is_empty() => {
            let off = extra[0] as i8 as i16;
            format!("{off},{reg_str}")
        }
        IndexMode::Off16Signed if extra.len() >= 2 => {
            let off = i16::from_be_bytes([extra[0], extra[1]]);
            format!("{off},{reg_str}")
        }
        IndexMode::Pcr8 if !extra.is_empty() => {
            let off = extra[0] as i8 as i16;
            format!("{off},PCR")
        }
        IndexMode::Pcr16 if extra.len() >= 2 => {
            let off = i16::from_be_bytes([extra[0], extra[1]]);
            format!("{off},PCR")
        }
        IndexMode::IndirectExtended if extra.len() >= 2 => {
            let addr = u16::from_be_bytes([extra[0], extra[1]]);
            format!("${addr:04X}")
        }
        _ => format!(",{reg_str}"),
    };

    if indirect && !matches!(mode, IndexMode::AutoInc1 | IndexMode::AutoDec1) {
        format!("[{inner}]")
    } else {
        inner
    }
}

pub fn indexed_addr(cpu: &mut Cpu, mem: &Memory, postbyte: u8) -> EffectiveAddress {
    let (mode, indirect, extra_bytes, reg_bits) = decode_index_mode(postbyte);

    // Determine base register (PCR modes compute from PC)
    let (base, index_reg) = match mode {
        IndexMode::Pcr8 | IndexMode::Pcr16 => {
            let pc_after = cpu.pc.wrapping_add(extra_bytes as u16);
            (pc_after, None)
        }
        _ => {
            let reg = index_reg_char(reg_bits);
            let val = match reg {
                'X' => cpu.x,
                'Y' => cpu.y,
                'U' => cpu.u,
                'S' => cpu.s,
                _ => cpu.x,
            };
            (val, Some(reg))
        }
    };

    let (final_addr, cycles) = compute_indexed_addr(cpu, mem, mode, indirect, extra_bytes, base);

    update_auto_inc_dec(cpu, mode, reg_bits);

    EffectiveAddress {
        mode: AddrMode::Indexed,
        addr: final_addr,
        extra_cycles: cycles,
        postbyte: Some(postbyte),
        index_reg,
    }
}

fn compute_indexed_addr(
    cpu: &mut Cpu,
    mem: &Memory,
    mode: IndexMode,
    indirect: bool,
    _extra_bytes: u8,
    base: u16,
) -> (u16, u8) {
    let extra_cycles = index_extra_cycles(mode, indirect);

    let direct_addr = match mode {
        IndexMode::Const5(off) => base.wrapping_add(off as i16 as u16),
        IndexMode::ZeroOffset => base,
        IndexMode::AutoInc1 => base, // post-increment: EA = R, then R += 1
        IndexMode::AutoInc2 => base, // post-increment: EA = R, then R += 2
        IndexMode::AutoDec1 => base.wrapping_sub(1), // pre-decrement: R -= 1, EA = R
        IndexMode::AutoDec2 => base.wrapping_sub(2), // pre-decrement: R -= 2, EA = R
        IndexMode::AccA => base.wrapping_add(cpu.a as u16),
        IndexMode::AccB => base.wrapping_add(cpu.b as u16),
        IndexMode::AccD => base.wrapping_add(((cpu.a as u16) << 8) | cpu.b as u16),
        IndexMode::AccE if cpu.variant == CpuVariant::Hd6309 => {
            base.wrapping_add(cpu.w >> 8)
        }
        IndexMode::AccF if cpu.variant == CpuVariant::Hd6309 => {
            base.wrapping_add(cpu.w & 0xFF)
        }
        IndexMode::AccW if cpu.variant == CpuVariant::Hd6309 => base.wrapping_add(cpu.w),
        IndexMode::Off8Signed => {
            let off = mem.read8(cpu.pc) as i8 as i16;
            cpu.pc = cpu.pc.wrapping_add(1);
            base.wrapping_add(off as u16)
        }
        IndexMode::Off16Signed => {
            let off = mem.read16(cpu.pc) as i16 as i32;
            cpu.pc = cpu.pc.wrapping_add(2);
            base.wrapping_add(off as u16)
        }
        IndexMode::Pcr8 => {
            let off = mem.read8(cpu.pc) as i8 as i16;
            cpu.pc = cpu.pc.wrapping_add(1);
            // base = PC after postbyte + extra bytes (already computed)
            base.wrapping_add(off as u16)
        }
        IndexMode::Pcr16 => {
            let off = mem.read16(cpu.pc) as i16 as i32;
            cpu.pc = cpu.pc.wrapping_add(2);
            base.wrapping_add(off as u16)
        }
        IndexMode::IndirectExtended => {
            let addr = mem.read16(cpu.pc);
            cpu.pc = cpu.pc.wrapping_add(2);
            addr
        }
        _ => base,
    };

    if indirect {
        // For indirect modes, read the effective address from memory
        match mode {
            IndexMode::AutoInc1 | IndexMode::AutoDec1 => {
                // Illegal indirect - return direct address as fallback
                (direct_addr, extra_cycles)
            }
            IndexMode::IndirectExtended => {
                // Already indirect: the address read IS the EA
                (direct_addr, extra_cycles)
            }
            _ => {
                let ea = mem.read16(direct_addr);
                (ea, extra_cycles)
            }
        }
    } else {
        (direct_addr, extra_cycles)
    }
}

fn update_auto_inc_dec(cpu: &mut Cpu, mode: IndexMode, reg_bits: u8) {
    match mode {
        IndexMode::AutoInc1 | IndexMode::AutoInc2 => {
            let inc = if matches!(mode, IndexMode::AutoInc1) { 1u16 } else { 2u16 };
            match reg_bits {
                0 => cpu.x = cpu.x.wrapping_add(inc),
                1 => cpu.y = cpu.y.wrapping_add(inc),
                2 => cpu.u = cpu.u.wrapping_add(inc),
                3 => cpu.s = cpu.s.wrapping_add(inc),
                _ => {}
            }
        }
        IndexMode::AutoDec1 | IndexMode::AutoDec2 => {
            let dec = if matches!(mode, IndexMode::AutoDec1) { 1u16 } else { 2u16 };
            match reg_bits {
                0 => cpu.x = cpu.x.wrapping_sub(dec),
                1 => cpu.y = cpu.y.wrapping_sub(dec),
                2 => cpu.u = cpu.u.wrapping_sub(dec),
                3 => cpu.s = cpu.s.wrapping_sub(dec),
                _ => {}
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_const5_offset() {
        // 5,X = 0x05 (bit7=0, reg=00=X, off=00101=5)
        let (mode, indirect, extra, reg) = decode_index_mode(0x05);
        assert_eq!(mode, IndexMode::Const5(5));
        assert!(!indirect);
        assert_eq!(extra, 0);
        assert_eq!(reg, 0); // X
    }

    #[test]
    fn decode_const5_negative() {
        // -2,X = 0x1E (bit7=0, reg=00=X, off=11110=-2)
        let (mode, _, _, _) = decode_index_mode(0x1E);
        assert_eq!(mode, IndexMode::Const5(-2));
    }

    #[test]
    fn decode_zero_offset() {
        // ,X = 0x84 (bit7=1, reg=00=X, mode=0100=ZeroOffset)
        let (mode, indirect, extra, reg) = decode_index_mode(0x84);
        assert_eq!(mode, IndexMode::ZeroOffset);
        assert!(!indirect);
        assert_eq!(extra, 0);
        assert_eq!(reg, 0); // X
    }

    #[test]
    fn decode_indirect_zero_offset() {
        // [,X] = 0x94 (bit7=1, bit4=1=indirect, reg=00=X, mode=0100)
        let (mode, indirect, _, _) = decode_index_mode(0x94);
        assert_eq!(mode, IndexMode::ZeroOffset);
        assert!(indirect);
    }

    #[test]
    fn decode_auto_inc1() {
        // ,X+ = 0x80 (bit7=1, reg=00=X, mode=0000=AutoInc1)
        let (mode, _, _, reg) = decode_index_mode(0x80);
        assert_eq!(mode, IndexMode::AutoInc1);
        assert_eq!(reg, 0); // X
    }

    #[test]
    fn decode_auto_inc2() {
        // ,Y++ = 0xA1 (bit7=1, reg=01=Y, mode=0001=AutoInc2)
        let (mode, _, _, reg) = decode_index_mode(0xA1);
        assert_eq!(mode, IndexMode::AutoInc2);
        assert_eq!(reg, 1); // Y
    }

    #[test]
    fn decode_auto_dec1() {
        // ,-S = 0xE2 (bit7=1, reg=11=S, mode=0010=AutoDec1)
        let (mode, _, _, reg) = decode_index_mode(0xE2);
        assert_eq!(mode, IndexMode::AutoDec1);
        assert_eq!(reg, 3); // S
    }

    #[test]
    fn decode_auto_dec2() {
        // ,--U = 0xC3 (bit7=1, reg=10=U, mode=0011=AutoDec2)
        let (mode, _, _, reg) = decode_index_mode(0xC3);
        assert_eq!(mode, IndexMode::AutoDec2);
        assert_eq!(reg, 2); // U
    }

    #[test]
    fn decode_acc_a_offset() {
        // A,X = 0x86 (bit7=1, reg=00=X, mode=0110=AccA)
        let (mode, _, _, _) = decode_index_mode(0x86);
        assert_eq!(mode, IndexMode::AccA);
    }

    #[test]
    fn decode_acc_b_offset() {
        // B,Y = 0xA5 (bit7=1, reg=01=Y, mode=0101=AccB)
        let (mode, _, _, reg) = decode_index_mode(0xA5);
        assert_eq!(mode, IndexMode::AccB);
        assert_eq!(reg, 1); // Y
    }

    #[test]
    fn decode_acc_d_offset() {
        // D,X = 0x8B (bit7=1, reg=00=X, mode=1011=AccD)
        let (mode, _, _, _) = decode_index_mode(0x8B);
        assert_eq!(mode, IndexMode::AccD);
    }

    #[test]
    fn decode_off8_signed() {
        // n8,X = 0x88 (bit7=1, reg=00=X, mode=1000=Off8Signed, 1 extra byte)
        let (mode, _, extra, _) = decode_index_mode(0x88);
        assert_eq!(mode, IndexMode::Off8Signed);
        assert_eq!(extra, 1);
    }

    #[test]
    fn decode_off16_signed() {
        // n16,Y = 0xB9 (bit7=1, reg=01=Y, mode=1001=Off16Signed, 2 extra bytes)
        let (mode, _, extra, reg) = decode_index_mode(0xB9);
        assert_eq!(mode, IndexMode::Off16Signed);
        assert_eq!(extra, 2);
        assert_eq!(reg, 1); // Y
    }

    #[test]
    fn decode_pcr8() {
        // n8,PCR = 0x8C (bit7=1, reg=00, mode=1100=Pcr8, 1 extra byte)
        let (mode, _, extra, _) = decode_index_mode(0x8C);
        assert_eq!(mode, IndexMode::Pcr8);
        assert_eq!(extra, 1);
    }

    #[test]
    fn decode_pcr16() {
        // n16,PCR = 0x8D (bit7=1, reg=00, mode=1101=Pcr16, 2 extra bytes)
        let (mode, _, extra, _) = decode_index_mode(0x8D);
        assert_eq!(mode, IndexMode::Pcr16);
        assert_eq!(extra, 2);
    }

    #[test]
    fn decode_indirect_extended() {
        // [address] = 0x9F (bit7=1, bit4=1=indirect, reg=11, mode=1111=IndirectExtended)
        let (mode, indirect, extra, _) = decode_index_mode(0x9F);
        assert_eq!(mode, IndexMode::IndirectExtended);
        assert!(indirect);
        assert_eq!(extra, 2);
    }

    #[test]
    fn format_zero_offset() {
        assert_eq!(format_index_operand(0x84, &[]), ",X");
        assert_eq!(format_index_operand(0xA4, &[]), ",Y");
        assert_eq!(format_index_operand(0xC4, &[]), ",U");
        assert_eq!(format_index_operand(0xE4, &[]), ",S");
    }

    #[test]
    fn format_const5() {
        assert_eq!(format_index_operand(0x05, &[]), "5,X");
        assert_eq!(format_index_operand(0x1E, &[]), "-2,X");
        assert_eq!(format_index_operand(0x25, &[]), "5,Y");
    }

    #[test]
    fn format_auto_inc_dec() {
        assert_eq!(format_index_operand(0x80, &[]), ",X+");
        assert_eq!(format_index_operand(0x81, &[]), ",X++");
        assert_eq!(format_index_operand(0x82, &[]), ",-X");
        assert_eq!(format_index_operand(0x83, &[]), ",--X");
    }

    #[test]
    fn format_acc_offsets() {
        assert_eq!(format_index_operand(0x86, &[]), "A,X");
        assert_eq!(format_index_operand(0x85, &[]), "B,X");
        assert_eq!(format_index_operand(0x8B, &[]), "D,X");
    }

    #[test]
    fn format_pcr() {
        assert_eq!(format_index_operand(0x8C, &[0x05]), "5,PCR");
        assert_eq!(format_index_operand(0x8D, &[0x00, 0x10]), "16,PCR");
    }

    #[test]
    fn format_indirect() {
        assert_eq!(format_index_operand(0x94, &[]), "[,X]");
        assert_eq!(format_index_operand(0x98, &[0x10]), "[16,X]");
    }
}