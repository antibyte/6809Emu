use crate::flags::Flags;

pub fn add8(a: u8, b: u8, carry: bool, flags: &mut Flags) {
    let c = if carry { 1u16 } else { 0 };
    let r = a as u16 + b as u16 + c;
    let result = r as u8;
    flags.set(Flags::C, r > 0xFF);
    flags.set(Flags::H, (a ^ b ^ result) & 0x10 != 0);
    flags.set(Flags::V, (!(a ^ b) & (a ^ result) & 0x80) != 0);
    flags.set_nz8(result);
}

pub fn sub8(a: u8, b: u8, borrow: bool, flags: &mut Flags) {
    let c = if borrow { 1u16 } else { 0 };
    let r = a as u16 + 0x100 - b as u16 - c;
    let result = r as u8;
    // Motorola/MAME: C=1 when a borrow is required (unsigned a < b + borrow-in).
    flags.set(Flags::C, (a as u16) < (b as u16) + c);
    flags.set(Flags::H, (a ^ b ^ result) & 0x10 != 0);
    flags.set(Flags::V, ((a ^ b) & (a ^ result) & 0x80) != 0);
    flags.set_nz8(result);
}

pub fn add16(a: u16, b: u16, flags: &mut Flags) -> u16 {
    let r = a as u32 + b as u32;
    let result = r as u16;
    flags.set(Flags::C, r > 0xFFFF);
    flags.set(Flags::V, (!(a ^ b) & (a ^ result) & 0x8000) != 0);
    flags.set_nz16(result);
    result
}

pub fn sub16(a: u16, b: u16, flags: &mut Flags) -> u16 {
    let r = a as u32 + 0x10000 - b as u32;
    let result = r as u16;
    flags.set(Flags::C, a < b);
    flags.set(Flags::V, ((a ^ b) & (a ^ result) & 0x8000) != 0);
    flags.set_nz16(result);
    result
}

pub fn cmp8(a: u8, b: u8, flags: &mut Flags) {
    sub8(a, b, false, flags);
}

pub fn cmp16(a: u16, b: u16, flags: &mut Flags) {
    sub16(a, b, flags);
}

pub fn asl8(value: u8, flags: &mut Flags) -> u8 {
    let c = value & 0x80 != 0;
    let result = value << 1;
    flags.set(Flags::C, c);
    flags.set(Flags::V, (value ^ result) & 0x80 != 0);
    flags.set_nz8(result);
    result
}

pub fn asr8(value: u8, flags: &mut Flags) -> u8 {
    let c = value & 0x01 != 0;
    let result = ((value as i8) >> 1) as u8;
    flags.set(Flags::C, c);
    flags.set_nz8(result);
    result
}

pub fn lsr8(value: u8, flags: &mut Flags) -> u8 {
    let c = value & 0x01 != 0;
    let result = value >> 1;
    flags.set(Flags::C, c);
    flags.set_nz8(result);
    result
}

pub fn rol8(value: u8, flags: &mut Flags) -> u8 {
    let c_in = flags.contains(Flags::C);
    let c_out = value & 0x80 != 0;
    let result = (value << 1) | u8::from(c_in);
    flags.set(Flags::C, c_out);
    flags.set(Flags::V, (result & 0x80) != (value & 0x80));
    flags.set_nz8(result);
    result
}

pub fn ror8(value: u8, flags: &mut Flags) -> u8 {
    let c_in = flags.contains(Flags::C);
    let c_out = value & 0x01 != 0;
    let result = (value >> 1) | (u8::from(c_in) << 7);
    flags.set(Flags::C, c_out);
    flags.set_nz8(result);
    result
}

pub fn inc16(value: u16, flags: &mut Flags) -> u16 {
    let result = value.wrapping_add(1);
    flags.set(Flags::V, value == 0x7FFF);
    flags.set_nz16(result);
    result
}

pub fn dec16(value: u16, flags: &mut Flags) -> u16 {
    let result = value.wrapping_sub(1);
    flags.set(Flags::V, value == 0x8000);
    flags.set_nz16(result);
    result
}

pub fn neg16(value: u16, flags: &mut Flags) -> u16 {
    sub16(0, value, flags)
}

pub fn com16(value: u16, flags: &mut Flags) -> u16 {
    let result = !value;
    flags.insert(Flags::C);
    flags.set_nz16(result);
    result
}

pub fn asl16(value: u16, flags: &mut Flags) -> u16 {
    let c = value & 0x8000 != 0;
    let result = value << 1;
    flags.set(Flags::C, c);
    flags.set(Flags::V, (value ^ result) & 0x8000 != 0);
    flags.set_nz16(result);
    result
}

pub fn asr16(value: u16, flags: &mut Flags) -> u16 {
    let c = value & 0x0001 != 0;
    let result = ((value as i16) >> 1) as u16;
    flags.set(Flags::C, c);
    flags.set_nz16(result);
    result
}

pub fn lsr16(value: u16, flags: &mut Flags) -> u16 {
    let c = value & 0x0001 != 0;
    let result = value >> 1;
    flags.set(Flags::C, c);
    flags.set_nz16(result);
    result
}

pub fn rol16(value: u16, flags: &mut Flags) -> u16 {
    let c_in = flags.contains(Flags::C);
    let c_out = value & 0x8000 != 0;
    let result = (value << 1) | u16::from(c_in);
    flags.set(Flags::C, c_out);
    flags.set(Flags::V, (result & 0x8000) != (value & 0x8000));
    flags.set_nz16(result);
    result
}

pub fn ror16(value: u16, flags: &mut Flags) -> u16 {
    let c_in = flags.contains(Flags::C);
    let c_out = value & 0x0001 != 0;
    let result = (value >> 1) | (u16::from(c_in) << 15);
    flags.set(Flags::C, c_out);
    flags.set_nz16(result);
    result
}

pub fn tst16(value: u16, flags: &mut Flags) {
    flags.remove(Flags::V | Flags::C);
    flags.set_nz16(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub8_borrow_sets_carry() {
        let mut f = Flags::empty();
        sub8(0x00, 0x01, false, &mut f);
        assert!(f.contains(Flags::C));
    }

    #[test]
    fn sub8_no_borrow_clears_carry() {
        let mut f = Flags::empty();
        sub8(0x02, 0x01, false, &mut f);
        assert!(!f.contains(Flags::C));
    }

    #[test]
    fn sub8_sbc_with_carry_clear_borrows_extra() {
        let mut f = Flags::empty();
        sub8(0x10, 0x05, true, &mut f);
        assert!(!f.contains(Flags::C));
    }

    #[test]
    fn sub16_borrow_sets_carry() {
        let mut f = Flags::empty();
        sub16(0x0000, 0x0001, &mut f);
        assert!(f.contains(Flags::C));
    }

    #[test]
    fn cmp8_sets_carry_on_unsigned_less() {
        let mut f = Flags::empty();
        cmp8(0x42, 0x43, &mut f);
        assert!(f.contains(Flags::C));
    }
}