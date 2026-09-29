//! CTS256A-AL2 code-to-speech front end, driven by the real mask ROM.
//!
//! The CTS256A-AL2 is a PIC7041 (a licensed TMS7041 second source) whose
//! 4 KiB mask ROM holds General Instrument's letter-to-sound program. This
//! module runs that ROM on the crate's [`crate::tms7000`] core and wires it
//! like schematic 2 of GI application note AN-0505D (parallel input,
//! 2 KiB external RAM):
//!
//! - 10 MHz crystal with the ÷4 clock option: 2.5 MHz state clock. Both the
//!   app note's baud-rate formula (table 6) and the ROM's own UART table
//!   (e.g. 300 baud = `2.5 MHz / (64 * 130)`) assume 2.5 MHz. With the
//!   SP0256's 3.12 MHz crystal that is 250 CTS cycles per 10 kHz sample.
//! - Port A straps, read by the ROM after reset: PA0-PA2 = `000` parallel
//!   input, PA3 = 0 default UART values, PA4 = 1 external RAM buffers,
//!   PA7 = 0 carriage-return-only delimiter mode.
//! - `$0200` read: the 74LS374 parallel-input latch. The host's data strobe
//!   also drives INT3; the ROM's INT3 handler disarms INT3, reads the latch,
//!   stores the character and re-arms INT3.
//! - `$2000-$2FFF` write: the SP0256-AL2 ALD strobe. The allophone is the
//!   low six address bits (the SP0256 A1-A6 inputs sit on the latched
//!   address bus), so the ROM outputs allophone `n` with a store to
//!   `$2000 + n`.
//! - `$3000-$37FF`: 2 KiB static RAM (1792-byte input buffer + 256-byte
//!   allophone output buffer, found by the ROM's RAM-sizing loop).
//! - INT1 ← SP0256 LRQ (ready = asserted), BUSY* → port B0.
//! - No exception-word/user EPROM: open-bus reads return `$FF`, so the
//!   ROM's signature scan of `$1000-$E000` finds nothing.
//!
//! Host adapter: bytes written by the 6809 (or the UI) queue in a host FIFO
//! ([`HOST_FIFO_CAP`] bytes). The next byte is strobed into the latch only
//! when the ROM has re-armed its INT3 input interrupt, at least 450 µs after
//! the previous strobe (AN-0505 table 9), and never inside the few idle-loop
//! instructions that clear the ROM's "CR received" flag (a CR strobed there
//! would be forgotten and the phrase never spoken). The strobe is held until
//! the ROM reads the latch, so it survives the INT1 handler's read-modify-
//! write of IOCNT0. No character is lost and the allophone stream does not
//! depend on how fast the host writes. Carriage-return-only mode makes the
//! ROM wait for a CR before it converts a phrase, exactly like the real chip
//! (and, like the real chip, a phrase longer than its 1792-byte buffer
//! without a CR fills the buffer, raises BUSY* and stalls until a reset).
//!
//! After reset the ROM speaks "O.K." (hardware behaviour). The greeting can
//! be suppressed: its allophones are then swallowed while LRQ is presented
//! as always ready, so the ROM drains them in a few milliseconds of CTS time.

use std::collections::VecDeque;

use crate::sp0256::Sp0256;
use crate::tms7000::{Tms7000, Tms7000Bus};

/// CTS256A-AL2 internal state clock: 10 MHz crystal, ÷4 clock option.
pub const CTS_CLOCK_HZ: u32 = 2_500_000;

/// Port A straps: parallel input, default UART values, external RAM,
/// carriage-return-only delimiter mode.
const PORT_A_STRAPS: u8 = 0x10;

/// Parallel-input latch (74LS374 output enable) decode.
const LATCH_ADDR: u16 = 0x0200;
/// SP0256 ALD decode (`$2000-$2FFF`, allophone in the low address bits).
const ALD_BASE: u16 = 0x2000;
const ALD_END: u16 = 0x2FFF;
/// External static RAM.
const RAM_BASE: u16 = 0x3000;
const RAM_SIZE: usize = 0x0800;

/// ROM main loop: first reached once initialisation (and the queueing of the
/// "O-K" greeting) has finished.
const ROM_MAIN_LOOP: u16 = 0xF110;
/// ROM idle loop (`$F105-$F11B`): waiting for input with an empty buffer.
const ROM_IDLE_LOOP: std::ops::Range<u16> = 0xF105..0xF11C;
/// In carriage-return-only mode the idle loop waits here for the CR flag.
const ROM_CR_WAIT: u16 = 0xF10C;
/// Minimum time between two data strobes (AN-0505 table 9: 450 µs).
const STROBE_HOLDOFF_CYCLES: u64 = CTS_CLOCK_HZ as u64 * 450 / 1_000_000;

// Register-file layout of the mask ROM (buffer pointers are register pairs
// `R(n-1):R(n)`, read with `Tms7000::reg16(n)`).
/// R11 bit 0: any-delimiter mode (PA7 strap); bit 4: CR received.
const R_FLAGS: u8 = 11;
const R_IN_READ: u8 = 3;
const R_IN_WRITE: u8 = 5;
const R_OUT_READ: u8 = 7;
const R_OUT_WRITE: u8 = 9;
const R_OUT_END: u8 = 39;
const R_IN_START: u8 = 41;
const R_OUT_START: u8 = 43;

/// IOCNT0 enable bits.
const IOCNT0_INT1_ENABLE: u8 = 0x01;
const IOCNT0_INT3_ENABLE: u8 = 0x10;

/// Host-side input FIFO capacity (bytes waiting for the parallel port).
pub const HOST_FIFO_CAP: usize = 4096;
/// Number of recently output allophones kept for the UI.
const RECENT_CAP: usize = 24;

/// External bus of the CTS256A-AL2 board.
#[derive(Clone)]
struct CtsBus {
    ram: Vec<u8>,
    latch: u8,
    /// 74LS74 LATCH-BUSY flip-flop: set by the strobe, cleared by the read.
    latch_full: bool,
    /// Allophone strobed into the SP0256 by the last instruction.
    ald: Option<u8>,
}

impl CtsBus {
    fn new() -> Self {
        Self {
            ram: vec![0; RAM_SIZE],
            latch: 0,
            latch_full: false,
            ald: None,
        }
    }
}

impl Tms7000Bus for CtsBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            LATCH_ADDR => {
                self.latch_full = false;
                self.latch
            }
            RAM_BASE..=0x37FF => self.ram[usize::from(addr - RAM_BASE)],
            // Nothing else answers (no EPROM, no UART-parameter buffer).
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            ALD_BASE..=ALD_END => self.ald = Some((addr & 0x3F) as u8),
            RAM_BASE..=0x37FF => self.ram[usize::from(addr - RAM_BASE)] = data,
            _ => {}
        }
    }
}

/// The CTS256A-AL2 chip plus its host adapter.
#[derive(Clone)]
pub struct Cts256 {
    cpu: Tms7000<CtsBus>,
    /// False while the chip is held in reset (CTS disabled).
    running: bool,
    /// The ROM has finished initialising (greeting queued).
    booted: bool,
    /// Swallowing the "O.K." greeting.
    muting: bool,
    host_fifo: VecDeque<u8>,
    /// INT3 data strobe currently held low.
    strobe: bool,
    /// CTS cycles since the last strobe was released (hold-off timer).
    since_strobe: u64,
    /// Cycles executed beyond the last budget.
    cycle_debt: i64,
    last_allophone: u8,
    recent: VecDeque<u8>,
    #[cfg(test)]
    pub(crate) log: Vec<u8>,
}

impl Cts256 {
    /// Build the chip around its mask ROM; it stays in reset until
    /// [`Self::reset`] enables it.
    pub fn new(rom: &'static [u8]) -> Self {
        Self {
            cpu: Tms7000::new(rom, CtsBus::new()),
            running: false,
            booted: false,
            muting: false,
            host_fifo: VecDeque::new(),
            strobe: false,
            since_strobe: STROBE_HOLDOFF_CYCLES,
            cycle_debt: 0,
            last_allophone: 0,
            recent: VecDeque::new(),
            #[cfg(test)]
            log: Vec::new(),
        }
    }

    /// Pulse RESET. With `enabled` the ROM boots (and greets with "O.K."
    /// unless `greeting` is false); otherwise the chip stays in reset.
    pub fn reset(&mut self, enabled: bool, greeting: bool) {
        self.host_fifo.clear();
        self.strobe = false;
        self.since_strobe = STROBE_HOLDOFF_CYCLES;
        self.cycle_debt = 0;
        self.last_allophone = 0;
        self.recent.clear();
        self.booted = false;
        self.muting = enabled && !greeting;
        self.running = enabled;
        let bus = self.cpu.bus_mut();
        bus.latch_full = false;
        bus.ald = None;
        self.cpu.set_int3(false);
        self.cpu.set_int1(false);
        self.cpu.set_port_in(0, PORT_A_STRAPS);
        self.cpu.reset();
    }

    /// Queue one byte for the parallel port. Returns false when the chip is
    /// in reset or the host FIFO is full (the byte is dropped).
    pub fn push_ascii(&mut self, byte: u8) -> bool {
        if !self.running || self.host_fifo.len() >= HOST_FIFO_CAP {
            return false;
        }
        self.host_fifo.push_back(byte);
        true
    }

    /// Run the chip for `cycles` state-clock cycles, feeding allophones to
    /// `sp` through the LRQ/ALD handshake.
    pub fn run(&mut self, sp: &mut Sp0256, cycles: u32) {
        if !self.running {
            return;
        }
        let mut budget = i64::from(cycles) - self.cycle_debt;
        while budget > 0 {
            self.feed_input();
            self.cpu.set_int1(self.muting || sp.lrq_ready());
            let used = self.cpu.step();
            budget -= i64::from(used);
            self.since_strobe = self.since_strobe.saturating_add(u64::from(used));
            self.after_step(sp);
        }
        self.cycle_debt = -budget;
    }

    /// Strobe the next host byte into the latch once the ROM is ready for
    /// it: INT3 re-armed, latch empty, 450 µs since the previous strobe, and
    /// not while the idle loop is about to clear its CR flag (a CR strobed
    /// there would be forgotten and the phrase never spoken).
    fn feed_input(&mut self) {
        if !self.booted
            || self.muting
            || self.strobe
            || self.since_strobe < STROBE_HOLDOFF_CYCLES
            || self.cpu.bus().latch_full
            || self.cpu.iocnt0() & IOCNT0_INT3_ENABLE == 0
            || self.in_cr_flag_window()
        {
            return;
        }
        if let Some(byte) = self.host_fifo.pop_front() {
            let bus = self.cpu.bus_mut();
            bus.latch = byte;
            bus.latch_full = true;
            self.cpu.set_int3(true);
            self.strobe = true;
        }
    }

    fn after_step(&mut self, sp: &mut Sp0256) {
        if let Some(code) = self.cpu.bus_mut().ald.take() {
            // While muted the greeting is swallowed; otherwise a strobe
            // while LRQ is busy is dropped by the SP0256's single latch.
            if !self.muting && sp.lrq_ready() {
                sp.ald_w(code);
                self.last_allophone = code;
                if self.recent.len() >= RECENT_CAP {
                    self.recent.pop_front();
                }
                self.recent.push_back(code);
                #[cfg(test)]
                self.log.push(code);
            }
        }
        if self.strobe && !self.cpu.bus().latch_full {
            // The ROM read the latch: release the data strobe.
            self.cpu.set_int3(false);
            self.strobe = false;
            self.since_strobe = 0;
        }
        if !self.booted && self.cpu.pc() == ROM_MAIN_LOOP {
            self.booted = true;
        }
        if self.muting && self.booted && self.rom_idle() {
            self.muting = false;
        }
    }

    /// Carriage-return-only mode, empty input buffer, and the idle loop has
    /// not yet reached its CR wait (it clears the CR flag on the way).
    fn in_cr_flag_window(&self) -> bool {
        let c = &self.cpu;
        c.reg(R_FLAGS) & 0x01 == 0
            && ROM_IDLE_LOOP.contains(&c.pc())
            && c.pc() != ROM_CR_WAIT
            && c.reg16(R_IN_READ) == c.reg16(R_IN_WRITE)
    }

    /// The ROM sits in its idle loop with empty buffers and no output armed.
    fn rom_idle(&self) -> bool {
        let c = &self.cpu;
        ROM_IDLE_LOOP.contains(&c.pc())
            && c.reg16(R_IN_READ) == c.reg16(R_IN_WRITE)
            && c.reg16(R_OUT_READ) == c.reg16(R_OUT_WRITE)
            && c.iocnt0() & IOCNT0_INT1_ENABLE == 0
    }

    /// Text or allophones still in flight (or the chip is still booting).
    pub fn busy(&self) -> bool {
        self.running
            && (!self.booted
                || self.muting
                || !self.host_fifo.is_empty()
                || self.cpu.bus().latch_full
                || !self.rom_idle())
    }

    pub fn idle(&self) -> bool {
        !self.busy()
    }

    /// Initialising / greeting (input is held back until done).
    pub fn booting(&self) -> bool {
        self.running && (!self.booted || self.muting)
    }

    /// Last allophone the ROM handed to the SP0256.
    pub fn last_allophone(&self) -> u8 {
        self.last_allophone
    }

    /// Recently output allophones, oldest first.
    pub fn recent(&self) -> Vec<u8> {
        self.recent.iter().copied().collect()
    }

    /// The host FIFO cannot take more bytes.
    pub fn fifo_full(&self) -> bool {
        self.host_fifo.len() >= HOST_FIFO_CAP
    }

    /// BUSY* output (port B0 low): the ROM's input buffer is >= 87.5 % full.
    pub fn busy_pin(&self) -> bool {
        self.running && self.cpu.port_out(1) & 0x01 == 0
    }

    /// Characters waiting: host FIFO + latch + the ROM's input buffer.
    pub fn input_pending(&self) -> usize {
        let queued = self.host_fifo.len() + usize::from(self.cpu.bus().latch_full);
        if !self.booted {
            return queued;
        }
        let c = &self.cpu;
        queued
            + ring_len(
                c.reg16(R_IN_START),
                c.reg16(R_OUT_START),
                c.reg16(R_IN_READ),
                c.reg16(R_IN_WRITE),
            )
    }

    /// Allophones waiting in the ROM's output buffer.
    pub fn output_pending(&self) -> usize {
        if !self.booted {
            return 0;
        }
        let c = &self.cpu;
        ring_len(
            c.reg16(R_OUT_START),
            c.reg16(R_OUT_END),
            c.reg16(R_OUT_READ),
            c.reg16(R_OUT_WRITE),
        )
    }
}

/// Fill level of a ring buffer `[start, end)` with read/write pointers.
fn ring_len(start: u16, end: u16, read: u16, write: u16) -> usize {
    let size = usize::from(end.wrapping_sub(start));
    if size == 0 {
        return 0;
    }
    let r = usize::from(read.wrapping_sub(start)) % size;
    let w = usize::from(write.wrapping_sub(start)) % size;
    (w + size - r) % size
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sp0256::{ALLOPHONE_NAMES, CLOCK_DIVIDER};
    use crate::speech_rom::{CTS256A, SP0256_AL2};

    /// Cycles per SP0256 sample at the standard 3.12 MHz crystal.
    const CYCLES_PER_SAMPLE: u32 = CTS_CLOCK_HZ / (3_120_000 / CLOCK_DIVIDER);

    pub(crate) fn names(codes: &[u8]) -> String {
        codes
            .iter()
            .map(|&c| ALLOPHONE_NAMES[usize::from(c & 0x3F)])
            .collect::<Vec<_>>()
            .join(" ")
    }

    struct Rig {
        cts: Cts256,
        sp: Sp0256,
    }

    impl Rig {
        fn new(greeting: bool) -> Self {
            let mut cts = Cts256::new(CTS256A);
            cts.reset(true, greeting);
            Self {
                cts,
                sp: Sp0256::new(SP0256_AL2),
            }
        }

        /// Booted chip with the greeting already out of the way.
        fn quiet() -> Self {
            let mut rig = Self::new(false);
            rig.run_until_idle(10_000);
            rig
        }

        /// One SP0256 sample period of both chips.
        fn sample(&mut self) {
            self.cts.run(&mut self.sp, CYCLES_PER_SAMPLE);
            self.sp.next_sample();
        }

        fn samples(&mut self, n: usize) {
            for _ in 0..n {
                self.sample();
            }
        }

        fn run_until_idle(&mut self, max_samples: usize) -> usize {
            for n in 0..max_samples {
                if self.cts.idle() && self.sp.idle() {
                    return n;
                }
                self.sample();
            }
            panic!(
                "speech did not finish in {max_samples} samples (pc={:04X}, log={})",
                self.cts.cpu.pc(),
                names(&self.cts.log)
            );
        }

        fn push(&mut self, text: &[u8]) {
            for &b in text {
                assert!(self.cts.push_ascii(b));
            }
        }

        fn say(&mut self, text: &[u8]) -> String {
            self.cts.log.clear();
            self.push(text);
            self.run_until_idle(400_000);
            names(&self.cts.log)
        }
    }

    #[test]
    fn boots_and_greets_ok() {
        let mut rig = Rig::new(true);
        assert!(rig.cts.busy(), "booting counts as busy");
        assert!(rig.cts.booting());
        rig.run_until_idle(100_000);
        assert_eq!(names(&rig.cts.log), "OW PA1 PA3 KK1 EY PA3");
        assert!(rig.cts.idle());
        assert!(!rig.cts.booting());
        assert!(!rig.cts.busy_pin());
        assert_eq!(rig.cts.last_allophone(), 0x02);
        assert_eq!(names(&rig.cts.recent()), "OW PA1 PA3 KK1 EY PA3");
    }

    #[test]
    fn greeting_can_be_suppressed() {
        let mut rig = Rig::new(false);
        let n = rig.run_until_idle(10_000);
        // Boot + converting "O-K" takes ~13 ms of CTS time, no speech.
        assert!(n < 500, "muted greeting drains in CTS time ({n} samples)");
        assert!(rig.cts.log.is_empty(), "no greeting: {}", names(&rig.cts.log));
        assert!(rig.sp.idle());
        let mut greeted = Rig::new(true);
        greeted.run_until_idle(100_000);
        assert_eq!(rig.say(b"HI\r"), greeted.say(b"HI\r"));
    }

    #[test]
    fn converts_text_like_the_real_chip() {
        let mut rig = Rig::quiet();
        assert_eq!(
            rig.say(b"HELLO WORLD\r"),
            "HH1 EH LL OW PA2 WW ER1 LL PA2 DD1 PA3"
        );
        assert_eq!(rig.say(b"SHE\r"), "SH IY PA3");
        // Digits are spelled out one by one.
        assert_eq!(
            rig.say(b"123\r"),
            "WW AX AX NN1 PA3 TT2 UW2 TH RR1 IY PA3"
        );
        // "$" is read as "dollars", "." as a full stop.
        assert_eq!(
            rig.say(b"$5.00\r"),
            "DD2 AA LL ER1 ZZ PA1 FF AY VV PA5 PA5 ZZ YR OW ZZ YR OW PA3"
        );
    }

    #[test]
    fn lowercase_input_is_handled_by_the_rom() {
        let mut rig = Rig::quiet();
        let upper = rig.say(b"HELLO WORLD\r");
        assert_eq!(rig.say(b"hello world\r"), upper);
        assert_eq!(rig.say(b"She Sells\r"), rig.say(b"SHE SELLS\r"));
    }

    #[test]
    fn waits_for_carriage_return() {
        let mut rig = Rig::quiet();
        rig.push(b"HELLO WORLD");
        rig.samples(20_000); // 2 s
        assert!(rig.cts.log.is_empty(), "CR-only mode: nothing spoken yet");
        assert!(rig.cts.busy());
        assert_eq!(rig.cts.input_pending(), 11);
        assert_eq!(rig.cts.output_pending(), 0);
        rig.push(b"\r");
        rig.run_until_idle(100_000);
        assert_eq!(
            names(&rig.cts.log),
            "HH1 EH LL OW PA2 WW ER1 LL PA2 DD1 PA3"
        );
        assert_eq!(rig.cts.input_pending(), 0);
    }

    #[test]
    fn escape_dumps_the_buffer() {
        let mut rig = Rig::quiet();
        rig.push(b"HELLO WORLD");
        rig.samples(1_000);
        // ESC flushes the pending text and silences with a PA1.
        assert_eq!(rig.say(b"\x1bSHE\r"), "PA1 SH IY PA3");
    }

    #[test]
    fn backspace_erases_the_last_character() {
        let mut rig = Rig::quiet();
        let she = rig.say(b"SHE\r");
        assert_eq!(rig.say(b"SHX\x08E\r"), she);
    }

    #[test]
    fn strobes_are_paced_by_the_datasheet_holdoff() {
        let mut rig = Rig::quiet();
        rig.push(b"ABCDEFGHIJ");
        let mut n = 0;
        while !rig.cts.host_fifo.is_empty() || rig.cts.cpu.bus().latch_full {
            rig.sample();
            n += 1;
            assert!(n < 1_000);
        }
        // 450 µs between strobes = 4.5 samples at 10 kHz.
        assert!((40..=50).contains(&n), "10 characters took {n} samples");
    }

    #[test]
    fn reset_restarts_the_rom() {
        let mut rig = Rig::new(true);
        rig.run_until_idle(100_000);
        rig.push(b"HELLO\r");
        rig.samples(2_000);
        rig.sp.reset();
        rig.cts.reset(true, true);
        assert!(rig.cts.booting());
        assert_eq!(rig.cts.input_pending(), 0, "reset clears the input");
        assert_eq!(rig.say(b""), "OW PA1 PA3 KK1 EY PA3");
        // Held in reset: nothing runs, input is refused.
        rig.cts.reset(false, true);
        assert!(!rig.cts.push_ascii(b'A'));
        rig.samples(1_000);
        assert!(rig.cts.idle());
    }

    #[test]
    fn full_input_buffer_raises_busy_and_backs_up_in_the_fifo() {
        // A phrase longer than the chip's buffer with no CR: the ROM fills
        // its 1792-byte buffer, drops BUSY* and stops taking input (the real
        // chip's CR-only limitation); the rest waits in the host FIFO.
        let mut rig = Rig::quiet();
        for i in 0..2500u32 {
            let b = if i % 6 == 5 { b' ' } else { b'A' + (i % 26) as u8 };
            rig.cts.push_ascii(b);
        }
        rig.samples(20_000);
        assert!(rig.cts.busy_pin(), "BUSY* asserted");
        assert!(rig.cts.host_fifo.len() > 500, "rest waits in the FIFO");
        assert!(rig.cts.busy());
        // The host FIFO itself is bounded.
        while rig.cts.push_ascii(b'X') {}
        assert!(rig.cts.fifo_full());
        assert_eq!(rig.cts.host_fifo.len(), HOST_FIFO_CAP);
    }
}
