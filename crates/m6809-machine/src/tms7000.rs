//! Texas Instruments TMS7000-family CPU core (clean-room Rust), configured as
//! the TMS7041 inside the GI CTS256A-AL2 code-to-speech chip.
//!
//! Reimplemented from the publicly documented TMS7000 instruction set. Opcode
//! map, addressing modes, flag behaviour, cycle counts and the interrupt
//! model follow MAME's `tms7000` core (license: BSD-3-Clause, copyright hap /
//! Tim Lindner) and the *TMS7000 Family Data Manual* (TI SPND001B, 1986). No
//! GPL sources were used.
//!
//! Part modelled: TMS7041 / PIC7041 (TMS70x1 class), i.e. what the
//! CTS256A-AL2 mask ROM runs on:
//!
//! | Address          | On-chip resource                                        |
//! |------------------|---------------------------------------------------------|
//! | `$0000-$007F`    | register file R0-R127 (128 bytes of RAM)                |
//! | `$0080-$00FF`    | unimplemented register file (reads 0, writes ignored)   |
//! | `$0100-$010B`    | peripheral file P0-P11 (IOCNT0, timer 1, ports A-D)     |
//! | `$0110-$0117`    | peripheral file P16-P23 (IOCNT1, timer 2, serial port)  |
//! | `$F000-$FFFF`    | 4 KiB mask ROM                                          |
//! | everything else  | external bus ([`Tms7000Bus`], full-expansion mode)      |
//!
//! External interrupts INT1/INT3 are falling-edge **and** level sensitive on
//! the TMS70x1 (data manual, appendix C.3.5): the IOCNT0 flag is the OR of an
//! edge-latched "pulse" flip-flop and the current pin level. The pulse is
//! cleared by the interrupt acknowledge or by writing 1 to the flag bit, so a
//! short strobe is never lost while a held level keeps the flag set.
//!
//! Not modelled because the CTS256A-AL2 ROM never uses them in parallel-input
//! mode (INT2/INT5 vectors are `$FFFF`, P2/P3/P18/P19 are never accessed):
//! timers 1-3 (their registers are plain storage, INT2/INT5 never fire) and
//! the UART used by the serial-input straps (P17/P20-P23 are plain storage,
//! INT4 never fires). Ports B-D do not switch function with the memory mode;
//! the core always behaves like full-expansion mode.

use std::borrow::Cow;

/// External memory bus seen by the CPU (everything outside the on-chip
/// register file, peripheral file and mask ROM).
pub trait Tms7000Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, data: u8);
}

/// TMS7000 status-register flags.
const SR_C: u8 = 0x80;
const SR_N: u8 = 0x40;
const SR_Z: u8 = 0x20;
const SR_I: u8 = 0x10;

/// External interrupt line indices for [`Tms7000::set_int_line`].
const EXT_INT1: usize = 0;
const EXT_INT3: usize = 1;

/// Interrupt levels in priority order (vector = `$FFFC - 2 * level`).
const IRQ_INT1: u16 = 0;
const IRQ_INT2: u16 = 1;
const IRQ_INT3: u16 = 2;
const IRQ_INT4: u16 = 3;
const IRQ_INT5: u16 = 4;

/// On-chip register file size of a TMS70x1 part.
const RF_SIZE: usize = 128;

#[derive(Clone, Copy)]
enum Alu {
    Mov,
    And,
    Or,
    Xor,
    Btjo,
    Btjz,
    Add,
    Adc,
    Sub,
    Sbb,
    Mpy,
    Cmp,
    Dac,
    Dsb,
    Clr,
    Dec,
    Inc,
    Inv,
    Rl,
    Rlc,
    Rr,
    Rrc,
    Swap,
    Xchb,
    Djnz,
}

const WB_NO: i32 = -1;

#[derive(Clone)]
pub struct Tms7000<B> {
    rom: Cow<'static, [u8]>,
    rom_base: u32,
    rf: [u8; RF_SIZE],
    pc: u16,
    sp: u8,
    sr: u8,
    /// IOCNT0 enables (d0/d2/d4), INT2 flag (d3) and memory mode (d6-d7).
    /// The INT1/INT3 flags (d1/d5) are derived from `ext_pulse`/`ext_level`.
    iocnt0: u8,
    /// IOCNT1: INT4/INT5 enables (d0/d2) and flags (d1/d3).
    iocnt1: u8,
    /// Pin level of INT1/INT3 (true = asserted, i.e. the active-low pin is low).
    ext_level: [bool; 2],
    /// Edge-detect pulse flip-flops of INT1/INT3.
    ext_pulse: [bool; 2],
    /// Timer 1/2 data + control registers (storage only, see module docs).
    timer_data: [u8; 2],
    timer_ctl: [u8; 2],
    /// Serial-port registers P17, P20, P21 (storage only).
    serial_regs: [u8; 3],
    port_latch: [u8; 4],
    port_ddr: [u8; 4],
    port_in: [u8; 4],
    idle_state: bool,
    icount: i64,
    bus: B,
}

impl<B: Tms7000Bus> Tms7000<B> {
    /// Build with a mask ROM mapped at the top of the address space (a 4 KiB
    /// image lands at `$F000`) and reset the CPU.
    pub fn new(rom: impl Into<Cow<'static, [u8]>>, bus: B) -> Self {
        let rom = rom.into();
        let rom_base = 0x1_0000u32.saturating_sub(rom.len() as u32);
        let mut cpu = Self {
            rom,
            rom_base,
            rf: [0; RF_SIZE],
            pc: 0,
            sp: 0,
            sr: 0,
            iocnt0: 0,
            iocnt1: 0,
            ext_level: [false; 2],
            ext_pulse: [false; 2],
            timer_data: [0; 2],
            timer_ctl: [0; 2],
            serial_regs: [0; 3],
            port_latch: [0xFF; 4],
            port_ddr: [0, 0xFF, 0, 0],
            port_in: [0xFF; 4],
            idle_state: false,
            icount: 0,
            bus,
        };
        cpu.reset();
        cpu
    }

    /// Pulse the RESET pin: ports, I/O control and status are cleared and
    /// the CPU vectors through `$FFFE` (the microcode's TRAP 0).
    pub fn reset(&mut self) {
        if self.idle_state {
            self.pc = self.pc.wrapping_add(1);
            self.idle_state = false;
        }
        // While RESET is asserted: ports A/B high, C/D inputs.
        self.port_latch = [0xFF; 4];
        self.port_ddr = [0, 0xFF, 0, 0];
        self.sr = 0;
        self.iocnt0 = 0;
        self.iocnt1 = 0;
        self.ext_pulse = [false; 2];
        self.timer_data = [0; 2];
        self.timer_ctl = [0; 2];
        self.serial_regs = [0; 3];
        self.sp = 0xFF;
        self.execute_one(0xFF);
        self.icount -= 3; // 17 cycles in total
    }

    // ---- external pins / inspection ----

    /// Set the value presented on an input port (0=A, 1=B, 2=C, 3=D).
    pub fn set_port_in(&mut self, port: usize, value: u8) {
        if let Some(p) = self.port_in.get_mut(port) {
            *p = value;
        }
    }

    /// Output pins of a port as driven by the CPU (`latch & ddr`).
    pub fn port_out(&self, port: usize) -> u8 {
        match (self.port_latch.get(port), self.port_ddr.get(port)) {
            (Some(l), Some(d)) => l & d,
            _ => 0xFF,
        }
    }

    /// Drive the INT1 pin (`true` = asserted / low).
    pub fn set_int1(&mut self, asserted: bool) {
        self.set_int_line(EXT_INT1, asserted);
    }

    /// Drive the INT3 pin (`true` = asserted / low).
    pub fn set_int3(&mut self, asserted: bool) {
        self.set_int_line(EXT_INT3, asserted);
    }

    fn set_int_line(&mut self, line: usize, asserted: bool) {
        if asserted && !self.ext_level[line] {
            // Falling edge on the pin: latch it in the pulse flip-flop.
            self.ext_pulse[line] = true;
        }
        self.ext_level[line] = asserted;
    }

    pub fn pc(&self) -> u16 {
        self.pc
    }

    #[cfg(test)]
    pub fn sp(&self) -> u8 {
        self.sp
    }

    #[cfg(test)]
    pub fn status(&self) -> u8 {
        self.sr
    }

    /// Register-file byte (R0-R127; the unimplemented half reads 0).
    pub fn reg(&self, n: u8) -> u8 {
        self.read_r8(n)
    }

    /// 16-bit register pair `R(n-1):R(n)` as used by indirect addressing.
    pub fn reg16(&self, n: u8) -> u16 {
        self.read_r16(n)
    }

    /// IOCNT0 as the CPU would read it (side-effect free).
    pub fn iocnt0(&self) -> u8 {
        self.read_iocnt0()
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.idle_state
    }

    pub fn bus(&self) -> &B {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut B {
        &mut self.bus
    }

    // ---- memory / register / peripheral access ----

    fn read_byte(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x00FF => self.read_r8(addr as u8),
            0x0100..=0x01FF => self.read_p((addr - 0x0100) as u8),
            _ if u32::from(addr) >= self.rom_base => {
                self.rom[(u32::from(addr) - self.rom_base) as usize]
            }
            _ => self.bus.read(addr),
        }
    }

    fn write_byte(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x00FF => self.write_r8(addr as u8, data),
            0x0100..=0x01FF => self.write_p((addr - 0x0100) as u8, data),
            // Writes to the internal mask ROM are ignored.
            _ if u32::from(addr) >= self.rom_base => {}
            _ => self.bus.write(addr, data),
        }
    }

    fn read_r8(&self, a: u8) -> u8 {
        self.rf.get(a as usize).copied().unwrap_or(0)
    }
    fn write_r8(&mut self, a: u8, d: u8) {
        if let Some(r) = self.rf.get_mut(a as usize) {
            *r = d;
        }
    }
    fn read_r16(&self, a: u8) -> u16 {
        (u16::from(self.read_r8(a.wrapping_sub(1))) << 8) | u16::from(self.read_r8(a))
    }
    fn write_r16(&mut self, a: u8, d: u16) {
        self.write_r8(a.wrapping_sub(1), (d >> 8) as u8);
        self.write_r8(a, d as u8);
    }
    fn read_mem8(&mut self, a: u16) -> u8 {
        self.read_byte(a)
    }
    fn write_mem8(&mut self, a: u16, d: u8) {
        self.write_byte(a, d);
    }
    fn read_mem16(&mut self, a: u16) -> u16 {
        let hi = u16::from(self.read_byte(a));
        let lo = u16::from(self.read_byte(a.wrapping_add(1)));
        (hi << 8) | lo
    }

    fn imm8(&mut self) -> u8 {
        let v = self.read_byte(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }
    fn imm16(&mut self) -> u16 {
        let hi = u16::from(self.imm8());
        let lo = u16::from(self.imm8());
        (hi << 8) | lo
    }

    fn push8(&mut self, data: u8) {
        self.sp = self.sp.wrapping_add(1);
        self.write_r8(self.sp, data);
    }
    fn pull8(&mut self) -> u8 {
        let v = self.read_r8(self.sp);
        self.sp = self.sp.wrapping_sub(1);
        v
    }
    fn push16(&mut self, data: u16) {
        self.push8((data >> 8) as u8);
        self.push8(data as u8);
    }
    fn pull16(&mut self) -> u16 {
        let lo = u16::from(self.pull8());
        let hi = u16::from(self.pull8());
        lo | (hi << 8)
    }

    // ---- peripheral file ----

    fn ext_flag(&self, line: usize) -> bool {
        self.ext_pulse[line] || self.ext_level[line]
    }

    fn read_iocnt0(&self) -> u8 {
        (self.iocnt0 & !0x22)
            | (u8::from(self.ext_flag(EXT_INT1)) << 1)
            | (u8::from(self.ext_flag(EXT_INT3)) << 5)
    }

    fn read_p(&mut self, offset: u8) -> u8 {
        match offset {
            0x00 => self.read_iocnt0(),
            0x10 => self.iocnt1,
            0x02 | 0x12 => self.timer_data[usize::from(offset >> 4)],
            0x03 | 0x13 => self.timer_ctl[usize::from(offset >> 4)],
            0x04 | 0x06 | 0x08 | 0x0A => {
                // Port B is output-only: it reads back its latch (ddr = $FF).
                let port = usize::from(offset / 2 - 2);
                (self.port_in[port] & !self.port_ddr[port])
                    | (self.port_latch[port] & self.port_ddr[port])
            }
            0x05 | 0x09 | 0x0B => self.port_ddr[usize::from(offset / 2 - 2)],
            0x11 => 0, // SSTAT: no UART modelled (RXRDY/TXRDY clear)
            0x14 => self.serial_regs[1],
            0x15 => self.serial_regs[2],
            0x16 => 0, // RXBUF
            // Remaining on-chip peripheral-file holes.
            0x01 | 0x07 | 0x0C..=0x0F | 0x17 => 0,
            // P24-P255: peripheral expansion on the external bus.
            _ => self.bus.read(0x0100 | u16::from(offset)),
        }
    }

    fn write_p(&mut self, offset: u8, data: u8) {
        match offset {
            0x00 => {
                // d0/d2/d4 enables and d6-d7 memory mode are written directly;
                // writing 1 to a flag (d1/d3/d5) clears it. The INT1/INT3
                // flags only lose their latched edge: a held level stays.
                self.iocnt0 = (self.iocnt0 & 0x08 & !data) | (data & 0xD5);
                if data & 0x02 != 0 {
                    self.ext_pulse[EXT_INT1] = false;
                }
                if data & 0x20 != 0 {
                    self.ext_pulse[EXT_INT3] = false;
                }
            }
            0x10 => {
                self.iocnt1 = (self.iocnt1 & (!data & 0x0A)) | (data & 0x05);
            }
            0x02 | 0x12 => self.timer_data[usize::from(offset >> 4)] = data,
            0x03 | 0x13 => self.timer_ctl[usize::from(offset >> 4)] = data,
            0x04 | 0x06 | 0x08 | 0x0A => {
                self.port_latch[usize::from(offset / 2 - 2)] = data;
            }
            0x05 | 0x09 | 0x0B => {
                // Changing the direction does not refresh the output pins.
                self.port_ddr[usize::from(offset / 2 - 2)] = data;
            }
            0x11 => self.serial_regs[0] = data,
            0x14 => self.serial_regs[1] = data,
            0x15 => self.serial_regs[2] = data,
            0x16 | 0x17 => {} // RXBUF (read-only) / TXBUF: no UART modelled
            0x01 | 0x07 | 0x0C..=0x0F => {}
            _ => self.bus.write(0x0100 | u16::from(offset), data),
        }
    }

    // ---- interrupts ----

    /// Highest-priority interrupt that is flagged and enabled (ignores I).
    fn pending_interrupt(&self) -> Option<u16> {
        if self.iocnt0 & 0x01 != 0 && self.ext_flag(EXT_INT1) {
            return Some(IRQ_INT1);
        }
        if self.iocnt0 & 0x0C == 0x0C {
            return Some(IRQ_INT2);
        }
        if self.iocnt0 & 0x10 != 0 && self.ext_flag(EXT_INT3) {
            return Some(IRQ_INT3);
        }
        if self.iocnt1 & 0x03 == 0x03 {
            return Some(IRQ_INT4);
        }
        if self.iocnt1 & 0x0C == 0x0C {
            return Some(IRQ_INT5);
        }
        None
    }

    fn do_interrupt(&mut self, level: u16) {
        // Acknowledge: clear the pulse flip-flop / internal flag.
        match level {
            IRQ_INT1 => self.ext_pulse[EXT_INT1] = false,
            IRQ_INT2 => self.iocnt0 &= !0x08,
            IRQ_INT3 => self.ext_pulse[EXT_INT3] = false,
            IRQ_INT4 => self.iocnt1 &= !0x02,
            _ => self.iocnt1 &= !0x08,
        }
        if self.idle_state {
            self.icount -= 17;
            self.pc = self.pc.wrapping_add(1);
            self.idle_state = false;
        } else {
            self.icount -= 19;
        }
        let sr = self.sr;
        self.push8(sr);
        let pc = self.pc;
        self.push16(pc);
        self.sr = 0;
        self.pc = self.read_mem16(0xFFFC - level * 2);
    }

    // ---- ALU core ----

    fn get_c(&self) -> u8 {
        (self.sr >> 7) & 1
    }
    fn set_c(&mut self, x: u32) {
        self.sr = (self.sr & 0x7f) | (((x >> 1) & 0x80) as u8);
    }
    fn set_nz(&mut self, x: u32) {
        self.sr = (self.sr & 0x9f)
            | (((x >> 1) & 0x40) as u8)
            | (if x & 0xff == 0 { 0x20 } else { 0 });
    }
    fn set_cnz(&mut self, x: u32) {
        self.sr = (self.sr & 0x1f)
            | (((x >> 1) & 0xc0) as u8)
            | (if x & 0xff == 0 { 0x20 } else { 0 });
    }

    /// Perform an ALU op; returns writeback value (>=0) or [`WB_NO`].
    fn alu(&mut self, op: Alu, p1: u8, p2: u8) -> i32 {
        let a = u32::from(p1);
        let b = u32::from(p2);
        match op {
            Alu::Mov => {
                self.set_cnz(b);
                b as i32
            }
            Alu::And => {
                let t = a & b;
                self.set_cnz(t);
                t as i32
            }
            Alu::Or => {
                let t = a | b;
                self.set_cnz(t);
                t as i32
            }
            Alu::Xor => {
                let t = a ^ b;
                self.set_cnz(t);
                t as i32
            }
            Alu::Add => {
                let t = a + b;
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Adc => {
                let t = a + b + u32::from(self.get_c());
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Sub => {
                let t = a.wrapping_sub(b);
                self.set_nz(t);
                self.set_c(!(t & 0xffff));
                (t & 0xff) as i32
            }
            Alu::Sbb => {
                let t = a
                    .wrapping_sub(b)
                    .wrapping_sub(u32::from(self.get_c() == 0));
                self.set_nz(t);
                self.set_c(!(t & 0xffff));
                (t & 0xff) as i32
            }
            Alu::Cmp => {
                let t = a.wrapping_sub(b);
                self.set_nz(t);
                self.set_c(!(t & 0xffff));
                WB_NO
            }
            Alu::Mpy => {
                self.icount -= 39;
                let t = a * b;
                self.set_cnz(t >> 8);
                // The product always lands in A:B.
                self.write_r16(1, t as u16);
                WB_NO
            }
            Alu::Dac => {
                self.icount -= 2;
                let c = u32::from(self.get_c());
                let (h1, l1) = (a >> 4 & 0xf, a & 0xf);
                let (h2, l2) = (b >> 4 & 0xf, b & 0xf);
                let mut d = if l1 + l2 + c < 10 { 0 } else { 1 };
                if h1 + h2 == 9 {
                    d |= 2;
                } else if h1 + h2 > 9 {
                    d |= 4;
                }
                const LUT: [u32; 6] = [0x00, 0x06, 0x00, 0x66, 0x60, 0x66];
                // 8-bit result: the carry comes only from the BCD decision.
                let t = (a + b + c + LUT[d as usize]) & 0xff;
                self.set_cnz(t);
                if d > 2 {
                    self.sr |= SR_C;
                }
                t as i32
            }
            Alu::Dsb => {
                self.icount -= 2;
                let c = i32::from(self.get_c() == 0);
                let (h1, l1) = (p1 >> 4, p1 & 0xf);
                let (h2, l2) = (p2 >> 4, p2 & 0xf);
                // Signed nibble compare: a borrow into an empty low nibble
                // (`0 - 1`) must request the decimal adjust.
                let mut d = if i32::from(l1) - c >= i32::from(l2) { 0 } else { 1 };
                if h1 == h2 {
                    d |= 2;
                } else if h1 < h2 {
                    d |= 4;
                }
                const LUT: [u8; 6] = [0x00, 0x06, 0x00, 0x66, 0x60, 0x66];
                let t = p1
                    .wrapping_sub(p2)
                    .wrapping_sub(c as u8)
                    .wrapping_sub(LUT[d as usize]);
                // 8-bit result: C is set only when no decimal borrow occurred.
                self.set_cnz(u32::from(t));
                if d <= 2 {
                    self.sr |= SR_C;
                }
                i32::from(t)
            }
            Alu::Clr => {
                self.set_cnz(0);
                0
            }
            Alu::Dec => {
                let t = a.wrapping_sub(1);
                self.set_nz(t);
                self.set_c(!(t & 0xffff));
                (t & 0xff) as i32
            }
            Alu::Inc => {
                let t = a + 1;
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Inv => {
                // 8-bit result: INV never produces a carry.
                let t = !a & 0xff;
                self.set_cnz(t);
                t as i32
            }
            Alu::Rl => {
                let t = (a << 1) | (a >> 7);
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Rlc => {
                let t = (a << 1) | u32::from(self.get_c());
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Rr => {
                let t = (a >> 1) | (a << 8) | ((a << 7) & 0x80);
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Rrc => {
                let t = (a >> 1) | (a << 8) | (u32::from(self.get_c()) << 7);
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Swap => {
                self.icount -= 3;
                let t = (a >> 4) | (a << 4);
                self.set_cnz(t);
                (t & 0xff) as i32
            }
            Alu::Xchb => {
                self.icount -= 1;
                let t = u32::from(self.read_r8(1));
                self.set_cnz(t);
                self.write_r8(1, p1);
                t as i32
            }
            Alu::Btjo => {
                let t = a & b;
                self.set_cnz(t);
                self.shortbranch(t != 0);
                WB_NO
            }
            Alu::Btjz => {
                let t = !a & b & 0xff;
                self.set_cnz(t);
                self.shortbranch(t != 0);
                WB_NO
            }
            Alu::Djnz => {
                let t = a.wrapping_sub(1);
                self.shortbranch(t & 0xff != 0);
                (t & 0xff) as i32
            }
        }
    }

    // ---- addressing-mode wrappers ----

    fn wb_r(&mut self, reg: u8, r: i32) {
        if r > WB_NO {
            self.write_r8(reg, r as u8);
        }
    }

    fn wb_p(&mut self, reg: u8, r: i32) {
        if r > WB_NO {
            self.write_p(reg, r as u8);
        }
    }

    fn am_a(&mut self, op: Alu) {
        self.icount -= 5;
        let p1 = self.read_r8(0);
        let r = self.alu(op, p1, 0);
        self.wb_r(0, r);
    }
    fn am_b(&mut self, op: Alu) {
        self.icount -= 5;
        let p1 = self.read_r8(1);
        let r = self.alu(op, p1, 0);
        self.wb_r(1, r);
    }
    fn am_r(&mut self, op: Alu) {
        self.icount -= 7;
        let reg = self.imm8();
        let p1 = self.read_r8(reg);
        let r = self.alu(op, p1, 0);
        self.wb_r(reg, r);
    }
    fn am_a2a(&mut self, op: Alu) {
        self.icount -= 6;
        let a = self.read_r8(0);
        let r = self.alu(op, a, a);
        self.wb_r(0, r);
    }
    fn am_a2b(&mut self, op: Alu) {
        self.icount -= 6;
        let (p1, p2) = (self.read_r8(1), self.read_r8(0));
        let r = self.alu(op, p1, p2);
        self.wb_r(1, r);
    }
    fn am_a2r(&mut self, op: Alu) {
        self.icount -= 8;
        let reg = self.imm8();
        let (p1, p2) = (self.read_r8(reg), self.read_r8(0));
        let r = self.alu(op, p1, p2);
        self.wb_r(reg, r);
    }
    fn am_a2p(&mut self, op: Alu) {
        self.icount -= 10;
        let reg = self.imm8();
        let p1 = self.read_p(reg);
        let p2 = self.read_r8(0);
        let r = self.alu(op, p1, p2);
        self.wb_p(reg, r);
    }
    fn am_b2a(&mut self, op: Alu) {
        self.icount -= 5;
        let (p1, p2) = (self.read_r8(0), self.read_r8(1));
        let r = self.alu(op, p1, p2);
        self.wb_r(0, r);
    }
    fn am_b2b(&mut self, op: Alu) {
        self.icount -= 6;
        let b = self.read_r8(1);
        let r = self.alu(op, b, b);
        self.wb_r(1, r);
    }
    fn am_b2r(&mut self, op: Alu) {
        self.icount -= 7;
        let reg = self.imm8();
        let (p1, p2) = (self.read_r8(reg), self.read_r8(1));
        let r = self.alu(op, p1, p2);
        self.wb_r(reg, r);
    }
    fn am_b2p(&mut self, op: Alu) {
        self.icount -= 9;
        let reg = self.imm8();
        let p1 = self.read_p(reg);
        let p2 = self.read_r8(1);
        let r = self.alu(op, p1, p2);
        self.wb_p(reg, r);
    }
    fn am_r2a(&mut self, op: Alu) {
        self.icount -= 8;
        let src = self.imm8();
        let (p1, p2) = (self.read_r8(0), self.read_r8(src));
        let r = self.alu(op, p1, p2);
        self.wb_r(0, r);
    }
    fn am_r2b(&mut self, op: Alu) {
        self.icount -= 8;
        let src = self.imm8();
        let (p1, p2) = (self.read_r8(1), self.read_r8(src));
        let r = self.alu(op, p1, p2);
        self.wb_r(1, r);
    }
    fn am_r2r(&mut self, op: Alu) {
        self.icount -= 10;
        let src = self.imm8();
        let p2 = self.read_r8(src);
        let reg = self.imm8();
        let p1 = self.read_r8(reg);
        let r = self.alu(op, p1, p2);
        self.wb_r(reg, r);
    }
    fn am_i2a(&mut self, op: Alu) {
        self.icount -= 7;
        let imm = self.imm8();
        let p1 = self.read_r8(0);
        let r = self.alu(op, p1, imm);
        self.wb_r(0, r);
    }
    fn am_i2b(&mut self, op: Alu) {
        self.icount -= 7;
        let imm = self.imm8();
        let p1 = self.read_r8(1);
        let r = self.alu(op, p1, imm);
        self.wb_r(1, r);
    }
    fn am_i2r(&mut self, op: Alu) {
        self.icount -= 9;
        let imm = self.imm8();
        let reg = self.imm8();
        let p1 = self.read_r8(reg);
        let r = self.alu(op, p1, imm);
        self.wb_r(reg, r);
    }
    fn am_i2p(&mut self, op: Alu) {
        self.icount -= 11;
        let imm = self.imm8();
        let reg = self.imm8();
        let p1 = self.read_p(reg);
        let r = self.alu(op, p1, imm);
        self.wb_p(reg, r);
    }
    fn am_p2a(&mut self, op: Alu) {
        self.icount -= 9;
        let p = self.imm8();
        let p2 = self.read_p(p);
        let p1 = self.read_r8(0);
        let r = self.alu(op, p1, p2);
        self.wb_r(0, r);
    }
    fn am_p2b(&mut self, op: Alu) {
        self.icount -= 8;
        let p = self.imm8();
        let p2 = self.read_p(p);
        let p1 = self.read_r8(1);
        let r = self.alu(op, p1, p2);
        self.wb_r(1, r);
    }

    // ---- branches / jumps ----

    fn shortbranch(&mut self, check: bool) {
        self.icount -= 2;
        let d = self.imm8() as i8;
        if check {
            self.pc = self.pc.wrapping_add(d as u16);
            self.icount -= 2;
        }
    }
    fn jmp(&mut self, check: bool) {
        self.icount -= 3;
        self.shortbranch(check);
    }

    fn decd_reg(&mut self, addr: u8, cycles: i64) {
        self.icount -= cycles;
        let t = u32::from(self.read_r16(addr)).wrapping_sub(1);
        self.write_r16(addr, t as u16);
        self.set_nz(t >> 8);
        self.set_c(!((t >> 8) & 0xffff));
    }

    fn execute_one(&mut self, op: u8) {
        match op {
            0x00 => self.icount -= 5, // nop
            0x01 => {
                // idle: re-executes until an interrupt wakes the CPU
                self.icount -= 6;
                self.pc = self.pc.wrapping_sub(1);
                self.idle_state = true;
            }
            0x05 => {
                // eint
                self.icount -= 5;
                self.sr |= SR_N | SR_Z | SR_C | SR_I;
            }
            0x06 => {
                // dint
                self.icount -= 5;
                self.sr &= !(SR_N | SR_Z | SR_C | SR_I);
            }
            0x07 => {
                // setc
                self.icount -= 5;
                self.sr = (self.sr & !SR_N) | SR_C | SR_Z;
            }
            0x08 => {
                // pop st
                self.icount -= 6;
                self.sr = self.pull8() & 0xf0;
            }
            0x09 => {
                // stsp
                self.icount -= 6;
                let sp = self.sp;
                self.write_r8(1, sp);
            }
            0x0a => {
                // rets
                self.icount -= 7;
                self.pc = self.pull16();
            }
            0x0b => {
                // reti
                self.icount -= 9;
                self.pc = self.pull16();
                self.sr = self.pull8() & 0xf0;
            }
            0x0d => {
                // ldsp
                self.icount -= 5;
                self.sp = self.read_r8(1);
            }
            0x0e => {
                // push st
                self.icount -= 6;
                let sr = self.sr;
                self.push8(sr);
            }

            0x12 => self.am_r2a(Alu::Mov),
            0x13 => self.am_r2a(Alu::And),
            0x14 => self.am_r2a(Alu::Or),
            0x15 => self.am_r2a(Alu::Xor),
            0x16 => self.am_r2a(Alu::Btjo),
            0x17 => self.am_r2a(Alu::Btjz),
            0x18 => self.am_r2a(Alu::Add),
            0x19 => self.am_r2a(Alu::Adc),
            0x1a => self.am_r2a(Alu::Sub),
            0x1b => self.am_r2a(Alu::Sbb),
            0x1c => self.am_r2a(Alu::Mpy),
            0x1d => self.am_r2a(Alu::Cmp),
            0x1e => self.am_r2a(Alu::Dac),
            0x1f => self.am_r2a(Alu::Dsb),

            0x22 => self.am_i2a(Alu::Mov),
            0x23 => self.am_i2a(Alu::And),
            0x24 => self.am_i2a(Alu::Or),
            0x25 => self.am_i2a(Alu::Xor),
            0x26 => self.am_i2a(Alu::Btjo),
            0x27 => self.am_i2a(Alu::Btjz),
            0x28 => self.am_i2a(Alu::Add),
            0x29 => self.am_i2a(Alu::Adc),
            0x2a => self.am_i2a(Alu::Sub),
            0x2b => self.am_i2a(Alu::Sbb),
            0x2c => self.am_i2a(Alu::Mpy),
            0x2d => self.am_i2a(Alu::Cmp),
            0x2e => self.am_i2a(Alu::Dac),
            0x2f => self.am_i2a(Alu::Dsb),

            0x32 => self.am_r2b(Alu::Mov),
            0x33 => self.am_r2b(Alu::And),
            0x34 => self.am_r2b(Alu::Or),
            0x35 => self.am_r2b(Alu::Xor),
            0x36 => self.am_r2b(Alu::Btjo),
            0x37 => self.am_r2b(Alu::Btjz),
            0x38 => self.am_r2b(Alu::Add),
            0x39 => self.am_r2b(Alu::Adc),
            0x3a => self.am_r2b(Alu::Sub),
            0x3b => self.am_r2b(Alu::Sbb),
            0x3c => self.am_r2b(Alu::Mpy),
            0x3d => self.am_r2b(Alu::Cmp),
            0x3e => self.am_r2b(Alu::Dac),
            0x3f => self.am_r2b(Alu::Dsb),

            0x42 => self.am_r2r(Alu::Mov),
            0x43 => self.am_r2r(Alu::And),
            0x44 => self.am_r2r(Alu::Or),
            0x45 => self.am_r2r(Alu::Xor),
            0x46 => self.am_r2r(Alu::Btjo),
            0x47 => self.am_r2r(Alu::Btjz),
            0x48 => self.am_r2r(Alu::Add),
            0x49 => self.am_r2r(Alu::Adc),
            0x4a => self.am_r2r(Alu::Sub),
            0x4b => self.am_r2r(Alu::Sbb),
            0x4c => self.am_r2r(Alu::Mpy),
            0x4d => self.am_r2r(Alu::Cmp),
            0x4e => self.am_r2r(Alu::Dac),
            0x4f => self.am_r2r(Alu::Dsb),

            0x52 => self.am_i2b(Alu::Mov),
            0x53 => self.am_i2b(Alu::And),
            0x54 => self.am_i2b(Alu::Or),
            0x55 => self.am_i2b(Alu::Xor),
            0x56 => self.am_i2b(Alu::Btjo),
            0x57 => self.am_i2b(Alu::Btjz),
            0x58 => self.am_i2b(Alu::Add),
            0x59 => self.am_i2b(Alu::Adc),
            0x5a => self.am_i2b(Alu::Sub),
            0x5b => self.am_i2b(Alu::Sbb),
            0x5c => self.am_i2b(Alu::Mpy),
            0x5d => self.am_i2b(Alu::Cmp),
            0x5e => self.am_i2b(Alu::Dac),
            0x5f => self.am_i2b(Alu::Dsb),

            0x62 => self.am_b2a(Alu::Mov),
            0x63 => self.am_b2a(Alu::And),
            0x64 => self.am_b2a(Alu::Or),
            0x65 => self.am_b2a(Alu::Xor),
            0x66 => self.am_b2a(Alu::Btjo),
            0x67 => self.am_b2a(Alu::Btjz),
            0x68 => self.am_b2a(Alu::Add),
            0x69 => self.am_b2a(Alu::Adc),
            0x6a => self.am_b2a(Alu::Sub),
            0x6b => self.am_b2a(Alu::Sbb),
            0x6c => self.am_b2a(Alu::Mpy),
            0x6d => self.am_b2a(Alu::Cmp),
            0x6e => self.am_b2a(Alu::Dac),
            0x6f => self.am_b2a(Alu::Dsb),

            0x72 => self.am_i2r(Alu::Mov),
            0x73 => self.am_i2r(Alu::And),
            0x74 => self.am_i2r(Alu::Or),
            0x75 => self.am_i2r(Alu::Xor),
            0x76 => self.am_i2r(Alu::Btjo),
            0x77 => self.am_i2r(Alu::Btjz),
            0x78 => self.am_i2r(Alu::Add),
            0x79 => self.am_i2r(Alu::Adc),
            0x7a => self.am_i2r(Alu::Sub),
            0x7b => self.am_i2r(Alu::Sbb),
            0x7c => self.am_i2r(Alu::Mpy),
            0x7d => self.am_i2r(Alu::Cmp),
            0x7e => self.am_i2r(Alu::Dac),
            0x7f => self.am_i2r(Alu::Dsb),

            0x80 => self.am_p2a(Alu::Mov),
            0x82 => self.am_a2p(Alu::Mov),
            0x83 => self.am_a2p(Alu::And),
            0x84 => self.am_a2p(Alu::Or),
            0x85 => self.am_a2p(Alu::Xor),
            0x86 => self.am_a2p(Alu::Btjo),
            0x87 => self.am_a2p(Alu::Btjz),
            0x88 => self.movd_dir(),
            0x8a => self.lda_dir(),
            0x8b => self.sta_dir(),
            0x8c => self.br_dir(),
            0x8d => self.cmpa_dir(),
            0x8e => self.call_dir(),

            0x91 => self.am_p2b(Alu::Mov),
            0x92 => self.am_b2p(Alu::Mov),
            0x93 => self.am_b2p(Alu::And),
            0x94 => self.am_b2p(Alu::Or),
            0x95 => self.am_b2p(Alu::Xor),
            0x96 => self.am_b2p(Alu::Btjo),
            0x97 => self.am_b2p(Alu::Btjz),
            0x98 => self.movd_ind(),
            0x9a => self.lda_ind(),
            0x9b => self.sta_ind(),
            0x9c => self.br_ind(),
            0x9d => self.cmpa_ind(),
            0x9e => self.call_ind(),

            0xa2 => self.am_i2p(Alu::Mov),
            0xa3 => self.am_i2p(Alu::And),
            0xa4 => self.am_i2p(Alu::Or),
            0xa5 => self.am_i2p(Alu::Xor),
            0xa6 => self.am_i2p(Alu::Btjo),
            0xa7 => self.am_i2p(Alu::Btjz),
            0xa8 => self.movd_inx(),
            0xaa => self.lda_inx(),
            0xab => self.sta_inx(),
            0xac => self.br_inx(),
            0xad => self.cmpa_inx(),
            0xae => self.call_inx(),

            0xb0 => self.am_a2a(Alu::Mov), // aka clrc/tsta
            0xb1 => self.am_b2a(Alu::Mov), // undocumented
            0xb2 => self.am_a(Alu::Dec),
            0xb3 => self.am_a(Alu::Inc),
            0xb4 => self.am_a(Alu::Inv),
            0xb5 => self.am_a(Alu::Clr),
            0xb6 => self.am_a(Alu::Xchb),
            0xb7 => self.am_a(Alu::Swap),
            0xb8 => self.push_reg(0),
            0xb9 => self.pop_reg(0),
            0xba => self.am_a(Alu::Djnz),
            0xbb => self.decd_reg(0, 9),
            0xbc => self.am_a(Alu::Rr),
            0xbd => self.am_a(Alu::Rrc),
            0xbe => self.am_a(Alu::Rl),
            0xbf => self.am_a(Alu::Rlc),

            0xc0 => self.am_a2b(Alu::Mov),
            0xc1 => self.am_b2b(Alu::Mov), // aka tstb
            0xc2 => self.am_b(Alu::Dec),
            0xc3 => self.am_b(Alu::Inc),
            0xc4 => self.am_b(Alu::Inv),
            0xc5 => self.am_b(Alu::Clr),
            0xc6 => self.am_b(Alu::Xchb),
            0xc7 => self.am_b(Alu::Swap),
            0xc8 => self.push_reg(1),
            0xc9 => self.pop_reg(1),
            0xca => self.am_b(Alu::Djnz),
            0xcb => self.decd_reg(1, 9),
            0xcc => self.am_b(Alu::Rr),
            0xcd => self.am_b(Alu::Rrc),
            0xce => self.am_b(Alu::Rl),
            0xcf => self.am_b(Alu::Rlc),

            0xd0 => self.am_a2r(Alu::Mov),
            0xd1 => self.am_b2r(Alu::Mov),
            0xd2 => self.am_r(Alu::Dec),
            0xd3 => self.am_r(Alu::Inc),
            0xd4 => self.am_r(Alu::Inv),
            0xd5 => self.am_r(Alu::Clr),
            0xd6 => self.am_r(Alu::Xchb),
            0xd7 => self.am_r(Alu::Swap),
            0xd8 => self.push_r_imm(),
            0xd9 => self.pop_r_imm(),
            0xda => self.am_r(Alu::Djnz),
            0xdb => {
                let reg = self.imm8();
                self.decd_reg(reg, 11);
            }
            0xdc => self.am_r(Alu::Rr),
            0xdd => self.am_r(Alu::Rrc),
            0xde => self.am_r(Alu::Rl),
            0xdf => self.am_r(Alu::Rlc),

            0xe0 => self.jmp(true),
            0xe1 => self.jmp(self.sr & SR_N != 0),
            0xe2 => self.jmp(self.sr & SR_Z != 0),
            0xe3 => self.jmp(self.sr & SR_C != 0),
            0xe4 => self.jmp(self.sr & (SR_Z | SR_N) == 0),
            0xe5 => self.jmp(self.sr & SR_N == 0),
            0xe6 => self.jmp(self.sr & SR_Z == 0),
            0xe7 => self.jmp(self.sr & SR_C == 0),

            0xe8..=0xff => self.trap(op.wrapping_shl(1)),

            _ => {
                // illegal
                self.icount -= 5;
            }
        }
    }

    fn push_reg(&mut self, r: u8) {
        self.icount -= 6;
        let t = self.read_r8(r);
        self.push8(t);
        self.set_cnz(u32::from(t));
    }
    fn pop_reg(&mut self, r: u8) {
        self.icount -= 6;
        let t = self.pull8();
        self.write_r8(r, t);
        self.set_cnz(u32::from(t));
    }
    fn push_r_imm(&mut self) {
        self.icount -= 8;
        let reg = self.imm8();
        let t = self.read_r8(reg);
        self.push8(t);
        self.set_cnz(u32::from(t));
    }
    fn pop_r_imm(&mut self) {
        self.icount -= 8;
        let t = self.pull8();
        let reg = self.imm8();
        self.write_r8(reg, t);
        self.set_cnz(u32::from(t));
    }

    fn cmpa(&mut self, addr: u16) {
        let a = u32::from(self.read_r8(0));
        let t = a.wrapping_sub(u32::from(self.read_mem8(addr)));
        self.set_nz(t);
        self.set_c(!(t & 0xffff));
    }
    fn cmpa_dir(&mut self) {
        self.icount -= 12;
        let addr = self.imm16();
        self.cmpa(addr);
    }
    fn cmpa_inx(&mut self) {
        self.icount -= 14;
        let addr = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
        self.cmpa(addr);
    }
    fn cmpa_ind(&mut self) {
        self.icount -= 11;
        let reg = self.imm8();
        let addr = self.read_r16(reg);
        self.cmpa(addr);
    }

    fn lda(&mut self, addr: u16) {
        let t = self.read_mem8(addr);
        self.write_r8(0, t);
        self.set_cnz(u32::from(t));
    }
    fn lda_dir(&mut self) {
        self.icount -= 11;
        let addr = self.imm16();
        self.lda(addr);
    }
    fn lda_inx(&mut self) {
        self.icount -= 13;
        let addr = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
        self.lda(addr);
    }
    fn lda_ind(&mut self) {
        self.icount -= 10;
        let reg = self.imm8();
        let addr = self.read_r16(reg);
        self.lda(addr);
    }

    fn sta(&mut self, addr: u16) {
        let t = self.read_r8(0);
        self.write_mem8(addr, t);
        self.set_cnz(u32::from(t));
    }
    fn sta_dir(&mut self) {
        self.icount -= 11;
        let addr = self.imm16();
        self.sta(addr);
    }
    fn sta_inx(&mut self) {
        self.icount -= 13;
        let addr = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
        self.sta(addr);
    }
    fn sta_ind(&mut self) {
        self.icount -= 10;
        let reg = self.imm8();
        let addr = self.read_r16(reg);
        self.sta(addr);
    }

    fn movd_dir(&mut self) {
        self.icount -= 15;
        let t = self.imm16();
        let reg = self.imm8();
        self.write_r16(reg, t);
        self.set_cnz(u32::from(t >> 8));
    }
    fn movd_inx(&mut self) {
        self.icount -= 17;
        let t = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
        let reg = self.imm8();
        self.write_r16(reg, t);
        self.set_cnz(u32::from(t >> 8));
    }
    fn movd_ind(&mut self) {
        self.icount -= 14;
        let src = self.imm8();
        let t = self.read_r16(src);
        let reg = self.imm8();
        self.write_r16(reg, t);
        self.set_cnz(u32::from(t >> 8));
    }

    fn br_dir(&mut self) {
        self.icount -= 10;
        self.pc = self.imm16();
    }
    fn br_inx(&mut self) {
        self.icount -= 12;
        self.pc = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
    }
    fn br_ind(&mut self) {
        self.icount -= 9;
        let reg = self.imm8();
        self.pc = self.read_r16(reg);
    }

    fn call(&mut self, target: u16) {
        let pc = self.pc;
        self.push16(pc);
        self.pc = target;
    }
    fn call_dir(&mut self) {
        self.icount -= 14;
        let t = self.imm16();
        self.call(t);
    }
    fn call_inx(&mut self) {
        self.icount -= 16;
        let t = self.imm16().wrapping_add(u16::from(self.read_r8(1)));
        self.call(t);
    }
    fn call_ind(&mut self) {
        self.icount -= 13;
        let reg = self.imm8();
        let t = self.read_r16(reg);
        self.call(t);
    }

    fn trap(&mut self, address: u8) {
        self.icount -= 14;
        let pc = self.pc;
        self.push16(pc);
        self.pc = self.read_mem16(0xff00 | u16::from(address));
    }

    /// Execute one instruction (or take one pending interrupt) and return the
    /// number of internal state-clock cycles it used.
    pub fn step(&mut self) -> u32 {
        let start = self.icount;
        if self.sr & SR_I != 0 {
            if let Some(level) = self.pending_interrupt() {
                self.do_interrupt(level);
                return (start - self.icount) as u32;
            }
        }
        let op = self.imm8();
        self.execute_one(op);
        (start - self.icount) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech_rom::CTS256A;

    /// 64 KiB of plain external RAM.
    #[derive(Clone)]
    struct FlatBus(Vec<u8>);

    impl Tms7000Bus for FlatBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.0[addr as usize]
        }
        fn write(&mut self, addr: u16, data: u8) {
            self.0[addr as usize] = data;
        }
    }

    /// Build a tiny program in a fake 4 KiB ROM at $F000 with the reset
    /// vector pointing at $F000 and INT1/INT3 vectors at $F100/$F180.
    fn rom_with(bytes: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x1000];
        rom[..bytes.len()].copy_from_slice(bytes);
        rom[0x0FFE] = 0xF0; // reset -> $F000
        rom[0x0FFF] = 0x00;
        rom[0x0FFC] = 0xF1; // INT1 -> $F100
        rom[0x0FFD] = 0x00;
        rom[0x0FF8] = 0xF1; // INT3 -> $F180
        rom[0x0FF9] = 0x80;
        rom
    }

    fn cpu_with(bytes: &[u8]) -> Tms7000<FlatBus> {
        Tms7000::new(rom_with(bytes), FlatBus(vec![0; 0x1_0000]))
    }

    fn run(cpu: &mut Tms7000<FlatBus>, n: usize) {
        for _ in 0..n {
            cpu.step();
        }
    }

    #[test]
    fn reset_reads_vector_and_pushes_like_trap0() {
        let cpu = cpu_with(&[0x00]);
        assert_eq!(cpu.pc(), 0xF000);
        // SP starts at $FF and the reset microcode pushes the PC (TRAP 0).
        assert_eq!(cpu.sp(), 0x01);
        assert_eq!(cpu.status(), 0);
    }

    #[test]
    fn mov_imm_to_a_sets_zero_flag() {
        // MOV %>00,A ; MOV %>05,A
        let mut cpu = cpu_with(&[0x22, 0x00, 0x22, 0x05]);
        cpu.step();
        assert_eq!(cpu.reg(0), 0x00);
        assert_ne!(cpu.status() & SR_Z, 0, "Z should be set for 0");
        cpu.step();
        assert_eq!(cpu.reg(0), 0x05);
        assert_eq!(cpu.status() & SR_Z, 0, "Z clear for non-zero");
    }

    #[test]
    fn add_imm_sets_carry() {
        // MOV %>F0,A ; ADD %>20,A -> 0x110 -> A=0x10, C set
        let mut cpu = cpu_with(&[0x22, 0xF0, 0x28, 0x20]);
        run(&mut cpu, 2);
        assert_eq!(cpu.reg(0), 0x10);
        assert_ne!(cpu.status() & SR_C, 0, "carry out of 0xF0+0x20");
    }

    #[test]
    fn djnz_loops() {
        // MOV %>03,B ; L: DJNZ B,L
        let mut cpu = cpu_with(&[0x52, 0x03, 0xCA, 0xFE]);
        cpu.step();
        let mut guard = 0;
        while cpu.reg(1) != 0 && guard < 100 {
            cpu.step();
            guard += 1;
        }
        assert_eq!(cpu.reg(1), 0, "B should count down to 0");
        assert_eq!(guard, 3, "DJNZ should execute 3 times");
    }

    #[test]
    fn inv_clears_carry_like_an_8bit_op() {
        // MOV %>55,A ; SETC ; INV A   (MAME op_inv: u8 result, C = 0)
        let mut cpu = cpu_with(&[0x22, 0x55, 0x07, 0xB4]);
        run(&mut cpu, 3);
        assert_eq!(cpu.reg(0), 0xAA);
        assert_eq!(cpu.status() & SR_C, 0, "INV must not set C");
        assert_ne!(cpu.status() & SR_N, 0, "bit 7 of the result -> N");
        // INV of $FF -> $00: Z set, C still clear
        let mut cpu = cpu_with(&[0x22, 0xFF, 0x07, 0xB4]);
        run(&mut cpu, 3);
        assert_eq!(cpu.reg(0), 0x00);
        assert_eq!(cpu.status() & (SR_C | SR_Z), SR_Z);
    }

    /// Run `DSB B,A` (A = A - B - borrow, BCD) with the given carry-in.
    fn dsb(a: u8, b: u8, carry_in: bool) -> (u8, u8) {
        // MOV %a,A ; MOV %b,B ; SETC or CLRC (TSTA clears C) ; DSB B,A
        let c = if carry_in { 0x07 } else { 0xB0 };
        let mut cpu = cpu_with(&[0x22, a, 0x52, b, c, 0x6F]);
        run(&mut cpu, 4);
        (cpu.reg(0), cpu.status())
    }

    #[test]
    fn dsb_follows_mame_decimal_subtract() {
        // $10 - $00 with a borrow pending (C=0): signed nibble compare
        // (0 - 1 < 0) requests the adjust -> $09, no further borrow (C=1).
        let (r, st) = dsb(0x10, 0x00, false);
        assert_eq!(r, 0x09);
        assert_ne!(st & SR_C, 0);
        // $45 - $12 = $33, no borrow
        let (r, st) = dsb(0x45, 0x12, true);
        assert_eq!(r, 0x33);
        assert_ne!(st & SR_C, 0);
        // $12 - $45 = $67 with borrow: the u8 result must not set C
        let (r, st) = dsb(0x12, 0x45, true);
        assert_eq!(r, 0x67);
        assert_eq!(st & SR_C, 0, "negative result -> borrow (C=0)");
        // $00 - $01 = $99 with borrow, N from bit 7
        let (r, st) = dsb(0x00, 0x01, true);
        assert_eq!(r, 0x99);
        assert_eq!(st & (SR_C | SR_N | SR_Z), SR_N);
        // $00 - $00 = $00: Z set, no borrow
        let (r, st) = dsb(0x00, 0x00, true);
        assert_eq!(r, 0x00);
        assert_eq!(st & (SR_C | SR_Z), SR_C | SR_Z);
    }

    #[test]
    fn dac_adds_bcd() {
        // MOV %>19,A ; MOV %>03,B ; CLRC ; DAC B,A -> $22
        let mut cpu = cpu_with(&[0x22, 0x19, 0x52, 0x03, 0xB0, 0x6E]);
        run(&mut cpu, 4);
        assert_eq!(cpu.reg(0), 0x22);
        assert_eq!(cpu.status() & SR_C, 0);
        // $99 + $01 = $00 with decimal carry
        let mut cpu = cpu_with(&[0x22, 0x99, 0x52, 0x01, 0xB0, 0x6E]);
        run(&mut cpu, 4);
        assert_eq!(cpu.reg(0), 0x00);
        assert_eq!(cpu.status() & (SR_C | SR_Z), SR_C | SR_Z);
    }

    #[test]
    fn port_write_is_observable() {
        // MOV %>FF,A ; MOVP A,P9 (ddr C) ; MOV %>AA,A ; MOVP A,P8 (port C)
        let mut cpu = cpu_with(&[0x22, 0xFF, 0x82, 0x09, 0x22, 0xAA, 0x82, 0x08]);
        run(&mut cpu, 4);
        assert_eq!(cpu.port_out(2), 0xAA, "port C should latch 0xAA");
        // Port B is output-only and resets high.
        assert_eq!(cpu.port_out(1), 0xFF);
    }

    #[test]
    fn external_accesses_go_to_the_bus() {
        // MOV %>5A,A ; STA @>3000 ; CLR A ; LDA @>3000 ; MOVP A,P24 (ext $0118)
        let mut cpu = cpu_with(&[
            0x22, 0x5A, 0x8B, 0x30, 0x00, 0xB5, 0x8A, 0x30, 0x00, 0x82, 0x18,
        ]);
        run(&mut cpu, 5);
        assert_eq!(cpu.bus().0[0x3000], 0x5A);
        assert_eq!(cpu.reg(0), 0x5A);
        assert_eq!(cpu.bus().0[0x0118], 0x5A, "P24+ is peripheral expansion");
        // The on-chip register file is not on the external bus.
        assert_eq!(cpu.bus().0[0x0000], 0);
    }

    #[test]
    fn register_file_is_128_bytes() {
        // MOV %>77,R127 ; MOV %>66,R128 (unimplemented on a TMS70x1)
        let mut cpu = cpu_with(&[0x72, 0x77, 0x7F, 0x72, 0x66, 0x80]);
        run(&mut cpu, 2);
        assert_eq!(cpu.reg(127), 0x77);
        assert_eq!(cpu.reg(128), 0x00);
    }

    #[test]
    fn int1_is_taken_when_enabled() {
        // MOVP %>01,P0 (enable INT1) ; EINT ; JMP self
        let mut rom = rom_with(&[0xA2, 0x01, 0x00, 0x05, 0xE0, 0xFE]);
        rom[0x0100] = 0x0B; // RETI at $F100
        let mut cpu = Tms7000::new(rom, FlatBus(vec![0; 0x1_0000]));
        run(&mut cpu, 2);
        cpu.set_int1(true);
        let cycles = cpu.step();
        assert_eq!(cpu.pc(), 0xF100, "INT1 should vector to its handler");
        assert_eq!(cycles, 19);
        assert_eq!(cpu.status() & SR_I, 0, "entry clears the status register");
    }

    #[test]
    fn int3_edge_is_latched_even_after_the_pin_is_released() {
        // MOVP %>10,P0 (enable INT3) ; NOP ; NOP ; EINT ; JMP self
        let mut rom = rom_with(&[0xA2, 0x10, 0x00, 0x00, 0x00, 0x05, 0xE0, 0xFE]);
        rom[0x0180] = 0x0B; // RETI
        let mut cpu = Tms7000::new(rom, FlatBus(vec![0; 0x1_0000]));
        cpu.step();
        // A short strobe while interrupts are still globally disabled.
        cpu.set_int3(true);
        cpu.set_int3(false);
        assert_ne!(cpu.iocnt0() & 0x20, 0, "pulse flip-flop holds the flag");
        run(&mut cpu, 3); // NOP, NOP, EINT
        cpu.step();
        assert_eq!(cpu.pc(), 0xF180, "latched edge is serviced after EINT");
        assert_eq!(cpu.iocnt0() & 0x20, 0, "acknowledge clears the pulse");
    }

    #[test]
    fn held_level_keeps_the_flag_and_clearing_only_drops_the_edge() {
        // MOVP %>22,P0 (clear INT1/INT3 flags) ; JMP self
        let mut cpu = cpu_with(&[0xA2, 0x22, 0x00, 0xE0, 0xFE]);
        cpu.set_int3(true);
        assert_ne!(cpu.iocnt0() & 0x20, 0);
        cpu.step();
        assert_ne!(cpu.iocnt0() & 0x20, 0, "level still asserted -> flag set");
        cpu.set_int3(false);
        assert_eq!(cpu.iocnt0() & 0x20, 0, "edge was cleared by the write");
        // A pulse without a held level is cleared by writing 1.
        cpu.set_int1(true);
        cpu.set_int1(false);
        assert_ne!(cpu.iocnt0() & 0x02, 0);
        let mut cpu2 = cpu_with(&[0xA2, 0x02, 0x00]);
        cpu2.set_int1(true);
        cpu2.set_int1(false);
        cpu2.step();
        assert_eq!(cpu2.iocnt0() & 0x02, 0);
    }

    #[test]
    fn level_sensitive_int_reenters_after_reti_while_held() {
        // MOVP %>01,P0 ; EINT ; JMP self ; handler: RETI
        let mut rom = rom_with(&[0xA2, 0x01, 0x00, 0x05, 0xE0, 0xFE]);
        rom[0x0100] = 0x0B;
        let mut cpu = Tms7000::new(rom, FlatBus(vec![0; 0x1_0000]));
        run(&mut cpu, 2);
        cpu.set_int1(true);
        cpu.step(); // enter
        assert_eq!(cpu.pc(), 0xF100);
        cpu.step(); // RETI
        cpu.step(); // re-enter: the level is still asserted
        assert_eq!(cpu.pc(), 0xF100);
        cpu.set_int1(false);
        cpu.step(); // RETI
        cpu.step(); // back in the main loop
        assert!(cpu.pc() >= 0xF004 && cpu.pc() < 0xF008);
    }

    #[test]
    fn idle_waits_for_an_interrupt() {
        // MOVP %>01,P0 ; EINT ; IDLE ; NOP
        let mut rom = rom_with(&[0xA2, 0x01, 0x00, 0x05, 0x01, 0x00]);
        rom[0x0100] = 0x0B;
        let mut cpu = Tms7000::new(rom, FlatBus(vec![0; 0x1_0000]));
        run(&mut cpu, 10);
        assert!(cpu.is_idle());
        assert_eq!(cpu.pc(), 0xF004);
        cpu.set_int1(true);
        assert_eq!(cpu.step(), 17, "leaving IDLE takes 17 cycles");
        cpu.set_int1(false);
        cpu.step(); // RETI returns past the IDLE opcode
        assert_eq!(cpu.pc(), 0xF005);
        assert!(!cpu.is_idle());
    }

    #[test]
    fn boots_real_cts256_rom_into_its_main_loop() {
        // Parallel mode, internal RAM buffers: straps PA = $00.
        let mut cpu = Tms7000::new(CTS256A, FlatBus(vec![0xFF; 0x1_0000]));
        cpu.set_port_in(0, 0x00);
        assert_eq!(cpu.pc(), 0xF000, "reset vector points at the ROM start");
        let mut reached = false;
        for _ in 0..20_000 {
            cpu.step();
            if cpu.pc() == 0xF110 {
                reached = true;
                break;
            }
        }
        assert!(reached, "ROM should reach its main loop");
        assert_eq!(cpu.sp() & 0xC0, 0, "stack stays in the register file");
        // Parallel mode enables INT3 (the data strobe) and interrupts.
        assert_ne!(cpu.iocnt0() & 0x10, 0);
        assert_ne!(cpu.status() & SR_I, 0);
    }
}
