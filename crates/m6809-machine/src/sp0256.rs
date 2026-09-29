//! GI SP0256-AL2 "Narrator" speech processor core.
//!
//! This is a clean-room Rust reimplementation of the publicly documented
//! SP0256 microsequencer + 12-pole LPC lattice filter. The algorithm, the
//! coefficient quantisation table (`QTBL`), the microsequencer data-format
//! tables and the exact filter topology follow the public reverse engineering
//! by Joseph Zbiciak, as also implemented in MAME's `sp0256.cpp`
//! (license: BSD-3-Clause, copyright Joseph Zbiciak / Tim Lindner), which is
//! license-compatible with this MIT project. No GPL sources were used.
//!
//! The chip runs an internal micro-program stored in a 2 KiB mask ROM. Each
//! allophone is an entry point in a jump table at ROM byte `$1000`. The
//! microsequencer decodes variable-width bit fields into a bank of 16 registers
//! that drive a cascade of six 2-pole IIR sections excited by periodic impulses
//! (voiced) or pseudo-random noise (unvoiced), producing samples at
//! `xtal / 312` (~10 kHz for the standard 3.12 MHz crystal).
//!
//! Register widths match MAME (8-bit encoded registers, 16-bit wrapping filter
//! accumulator, 16-bit delay line), and an expired repeat count moves straight
//! on to the next frame, so every allophone renders bit-identically to MAME.
//! Unlike MAME, a halted chip outputs silence instead of the stale filter.

/// Clock divider from crystal to internal sample rate (`6 * 4 * 13`).
pub const CLOCK_DIVIDER: u32 = 312;

/// SP0256-AL2 allophone mnemonics, indexed by allophone address.
#[rustfmt::skip]
pub const ALLOPHONE_NAMES: [&str; 64] = [
    "PA1", "PA2", "PA3", "PA4", "PA5", "OY",  "AY",  "EH",
    "KK3", "PP",  "JH",  "NN1", "IH",  "TT2", "RR1", "AX",
    "MM",  "TT1", "DH1", "IY",  "EY",  "DD1", "UW1", "AO",
    "AA",  "YY2", "AE",  "HH1", "BB1", "TH",  "UH",  "UW2",
    "AW",  "DD2", "GG3", "VV",  "GG1", "SH",  "ZH",  "RR2",
    "FF",  "KK2", "KK1", "ZZ",  "NG",  "LL",  "WW",  "XR",
    "WH",  "YY1", "CH",  "ER1", "ER2", "OW",  "DH2", "SS",
    "NN2", "HH2", "OR",  "AR",  "YR",  "GG2", "EL",  "BB2",
];

const PER_PAUSE: u8 = 64;
const PER_NOISE: i32 = 64;
/// ROM byte offset where the SP0256-AL2 allophone jump table lives.
const ROM_BASE: usize = 0x1000;
/// Bit address of the SPB640 FIFO (never reached for the AL2 setup).
const FIFO_ADDR: u32 = 0x1800 << 3;

// ---- Coefficient quantisation table (SP0250/SP0256 data sheet) ----
#[rustfmt::skip]
const QTBL: [i16; 128] = [
    0,      9,      17,     25,     33,     41,     49,     57,
    65,     73,     81,     89,     97,     105,    113,    121,
    129,    137,    145,    153,    161,    169,    177,    185,
    193,    201,    209,    217,    225,    233,    241,    249,
    257,    265,    273,    281,    289,    297,    301,    305,
    309,    313,    317,    321,    325,    329,    333,    337,
    341,    345,    349,    353,    357,    361,    365,    369,
    373,    377,    381,    385,    389,    393,    397,    401,
    405,    409,    413,    417,    421,    425,    427,    429,
    431,    433,    435,    437,    439,    441,    443,    445,
    447,    449,    451,    453,    455,    457,    459,    461,
    463,    465,    467,    469,    471,    473,    475,    477,
    479,    481,    482,    483,    484,    485,    486,    487,
    488,    489,    490,    491,    492,    493,    494,    495,
    496,    497,    498,    499,    500,    501,    502,    503,
    504,    505,    506,    507,    508,    509,    510,    511,
];

// Register indices in the filter bank.
const AM: u16 = 0;
const PR: u16 = 1;
const B0: u16 = 2;
const F0: u16 = 3;
const B1: u16 = 4;
const F1: u16 = 5;
const B2: u16 = 6;
const F2: u16 = 7;
const B3: u16 = 8;
const F3: u16 = 9;
const B4: u16 = 10;
const F4: u16 = 11;
const B5: u16 = 12;
const F5: u16 = 13;
const IA: u16 = 14;
const IP: u16 = 15;

/// Pack a microsequencer control word: length, left-shift, param, delta,
/// field-replace, clear-5, clear-all.
const fn cr(l: u16, s: u16, p: u16, d: u16, f: u16, c5: u16, ca: u16) -> u16 {
    (l & 15) | ((s & 15) << 4) | ((p & 15) << 8) | ((d & 1) << 12) | ((f & 1) << 13) | ((c5 & 1) << 14) | ((ca & 1) << 15)
}

const fn cr_len(c: u16) -> u32 { (c & 15) as u32 }
const fn cr_shf(c: u16) -> u32 { ((c >> 4) & 15) as u32 }
const fn cr_prm(c: u16) -> usize { ((c >> 8) & 15) as usize }
const CR_DELTA: u16 = 1 << 12;
const CR_FIELD: u16 = 1 << 13;
const CR_CLR5: u16 = 1 << 14;
const CR_CLRA: u16 = 1 << 15;

#[rustfmt::skip]
const DATAFMT: [u16; 177] = [
    /* 0 PAUSE */ cr(0,0,0,0,0,0,1),
    /* LOADALL */
    cr(8,0,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(8,0,B0,0,0,0,0), cr(8,0,F0,0,0,0,0),
    cr(8,0,B1,0,0,0,0), cr(8,0,F1,0,0,0,0), cr(8,0,B2,0,0,0,0), cr(8,0,F2,0,0,0,0),
    cr(8,0,B3,0,0,0,0), cr(8,0,F3,0,0,0,0), cr(8,0,B4,0,0,0,0), cr(8,0,F4,0,0,0,0),
    cr(8,0,B5,0,0,0,0), cr(8,0,F5,0,0,0,0), cr(8,0,IA,0,0,0,0), cr(8,0,IP,0,0,0,0),
    /* LOAD_4 mode 00/01 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(4,3,B3,0,0,0,0), cr(6,2,F3,0,0,0,0),
    cr(7,1,B4,0,0,0,0), cr(6,2,F4,0,0,0,0), cr(8,0,B5,0,0,0,0), cr(8,0,F5,0,0,0,0),
    /* LOAD_4 mode 10/11 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(6,1,B3,0,0,0,0), cr(7,1,F3,0,0,0,0),
    cr(8,0,B4,0,0,0,0), cr(8,0,F4,0,0,0,0), cr(8,0,B5,0,0,0,0), cr(8,0,F5,0,0,0,0),
    /* SETMSB_6 mode 00/01 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(6,2,F3,0,1,0,0), cr(6,2,F4,0,1,0,0),
    cr(8,0,F5,0,1,0,0),
    /* SETMSB_6 mode 10/11 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(7,1,F3,0,1,0,0), cr(8,0,F4,0,1,0,0),
    cr(8,0,F5,0,1,0,0),
    /* 43,44 unused */ 0, 0,
    /* DELTA_9 mode 00/01 */
    cr(4,2,AM,1,0,0,0), cr(5,0,PR,1,0,0,0), cr(3,4,B0,1,0,0,0), cr(3,3,F0,1,0,0,0),
    cr(3,4,B1,1,0,0,0), cr(3,3,F1,1,0,0,0), cr(3,4,B2,1,0,0,0), cr(3,3,F2,1,0,0,0),
    cr(3,3,B3,1,0,0,0), cr(4,2,F3,1,0,0,0), cr(4,1,B4,1,0,0,0), cr(4,2,F4,1,0,0,0),
    cr(5,0,B5,1,0,0,0), cr(5,0,F5,1,0,0,0),
    /* DELTA_9 mode 10/11 */
    cr(4,2,AM,1,0,0,0), cr(5,0,PR,1,0,0,0), cr(4,1,B0,1,0,0,0), cr(4,2,F0,1,0,0,0),
    cr(4,1,B1,1,0,0,0), cr(4,2,F1,1,0,0,0), cr(4,1,B2,1,0,0,0), cr(4,2,F2,1,0,0,0),
    cr(4,1,B3,1,0,0,0), cr(5,1,F3,1,0,0,0), cr(5,0,B4,1,0,0,0), cr(5,0,F4,1,0,0,0),
    cr(5,0,B5,1,0,0,0), cr(5,0,F5,1,0,0,0),
    /* SETMSB_A mode 00/01 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(5,3,F0,0,1,0,0), cr(5,3,F1,0,1,0,0),
    cr(5,3,F2,0,1,0,0),
    /* SETMSB_A mode 10/11 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(6,2,F0,0,1,0,0), cr(6,2,F1,0,1,0,0),
    cr(6,2,F2,0,1,0,0),
    /* LOAD_2/LOAD_C mode 00 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(3,4,B0,0,0,0,0), cr(5,3,F0,0,0,0,0),
    cr(3,4,B1,0,0,0,0), cr(5,3,F1,0,0,0,0), cr(3,4,B2,0,0,0,0), cr(5,3,F2,0,0,0,0),
    cr(4,3,B3,0,0,0,0), cr(6,2,F3,0,0,0,0), cr(7,1,B4,0,0,0,0), cr(6,2,F4,0,0,0,0),
    cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
    /* LOAD_2/LOAD_C mode 10 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(6,1,B0,0,0,0,0), cr(6,2,F0,0,0,0,0),
    cr(6,1,B1,0,0,0,0), cr(6,2,F1,0,0,0,0), cr(6,1,B2,0,0,0,0), cr(6,2,F2,0,0,0,0),
    cr(6,1,B3,0,0,0,0), cr(7,1,F3,0,0,0,0), cr(8,0,B4,0,0,0,0), cr(8,0,F4,0,0,0,0),
    cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
    /* DELTA_D mode 00/01 */
    cr(4,2,AM,1,0,0,0), cr(5,0,PR,1,0,0,0), cr(3,3,B3,1,0,0,0), cr(4,2,F3,1,0,0,0),
    cr(4,1,B4,1,0,0,0), cr(4,2,F4,1,0,0,0), cr(5,0,B5,1,0,0,0), cr(5,0,F5,1,0,0,0),
    /* DELTA_D mode 10/11 */
    cr(4,2,AM,1,0,0,0), cr(5,0,PR,1,0,0,0), cr(4,1,B3,1,0,0,0), cr(5,1,F3,1,0,0,0),
    cr(5,0,B4,1,0,0,0), cr(5,0,F4,1,0,0,0), cr(5,0,B5,1,0,0,0), cr(5,0,F5,1,0,0,0),
    /* LOAD_E */
    cr(6,2,AM,0,0,0,0), cr(8,0,PR,0,0,0,0),
    /* LOAD_2/LOAD_C mode 01 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(3,4,B0,0,0,0,0), cr(5,3,F0,0,0,0,0),
    cr(3,4,B1,0,0,0,0), cr(5,3,F1,0,0,0,0), cr(3,4,B2,0,0,0,0), cr(5,3,F2,0,0,0,0),
    cr(4,3,B3,0,0,0,0), cr(6,2,F3,0,0,0,0), cr(7,1,B4,0,0,0,0), cr(6,2,F4,0,0,0,0),
    cr(8,0,B5,0,0,0,0), cr(8,0,F5,0,0,0,0), cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
    /* LOAD_2/LOAD_C mode 11 */
    cr(6,2,AM,0,0,0,1), cr(8,0,PR,0,0,0,0), cr(6,1,B0,0,0,0,0), cr(6,2,F0,0,0,0,0),
    cr(6,1,B1,0,0,0,0), cr(6,2,F1,0,0,0,0), cr(6,1,B2,0,0,0,0), cr(6,2,F2,0,0,0,0),
    cr(6,1,B3,0,0,0,0), cr(7,1,F3,0,0,0,0), cr(8,0,B4,0,0,0,0), cr(8,0,F4,0,0,0,0),
    cr(8,0,B5,0,0,0,0), cr(8,0,F5,0,0,0,0), cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
    /* SETMSB_3/SETMSB_5 mode 00/01 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(8,0,PR,0,0,0,0), cr(5,3,F0,0,1,0,0),
    cr(5,3,F1,0,1,0,0), cr(5,3,F2,0,1,0,0), cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
    /* SETMSB_3/SETMSB_5 mode 10/11 */
    cr(0,0,0,0,0,1,0), cr(6,2,AM,0,0,0,0), cr(8,0,PR,0,0,0,0), cr(6,2,F0,0,1,0,0),
    cr(6,2,F1,0,1,0,0), cr(6,2,F2,0,1,0,0), cr(5,0,IA,0,0,0,0), cr(5,0,IP,0,0,0,0),
];

#[rustfmt::skip]
const DF_IDX: [i16; 16 * 8] = [
    /* 0000 */ -1,-1, -1,-1, -1,-1, -1,-1,
    /* 1000 */ -1,-1, -1,-1, -1,-1, -1,-1,
    /* 0100 */ 17,22, 17,24, 25,30, 25,32,
    /* 1100 */ 83,94, 129,142, 97,108, 145,158,
    /* 0010 */ 83,96, 129,144, 97,110, 145,160,
    /* 1010 */ 73,77, 74,77, 78,82, 79,82,
    /* 0110 */ 33,36, 34,37, 38,41, 39,42,
    /* 1110 */ 127,128, 127,128, 127,128, 127,128,
    /* 0001 */ 1,14, 1,16, 1,14, 1,16,
    /* 1001 */ 45,56, 45,58, 59,70, 59,72,
    /* 0101 */ 161,166, 162,166, 169,174, 170,174,
    /* 1101 */ 111,116, 111,118, 119,124, 119,126,
    /* 0011 */ 161,168, 162,168, 169,176, 170,176,
    /* 1011 */ -1,-1, -1,-1, -1,-1, -1,-1,
    /* 0111 */ -1,-1, -1,-1, -1,-1, -1,-1,
    /* 1111 */ 0,0, 0,0, 0,0, 0,0,
];

#[inline]
fn bitrev8(v: u8) -> u8 {
    let v = ((v & 0xF0) >> 4) | ((v & 0x0F) << 4);
    let v = ((v & 0xCC) >> 2) | ((v & 0x33) << 2);
    ((v & 0xAA) >> 1) | ((v & 0x55) << 1)
}

#[inline]
fn bitrev32(v: u32) -> u32 {
    v.reverse_bits()
}

/// 14-bit output limiter ("high quality" variant).
#[inline]
fn limit(s: i16) -> i16 {
    s.clamp(-8192, 8191)
}

/// Inverse-quantise an encoded filter coefficient.
#[inline]
fn iq(x: u8) -> i16 {
    if x & 0x80 != 0 {
        QTBL[(0x7F & x.wrapping_neg()) as usize]
    } else {
        -QTBL[x as usize]
    }
}

#[derive(Clone)]
struct Lpc12 {
    /// Repeat counter and period down-counter.
    rpt: i32,
    cnt: i32,
    /// Pitch period decoded from the 8-bit period register.
    per: u32,
    rng: u32,
    amp: i32,
    f_coef: [i16; 6],
    b_coef: [i16; 6],
    /// Filter delay line (16-bit like the hardware accumulator).
    z: [[i16; 2]; 6],
    /// The encoded 8-bit register set.
    r: [u8; 16],
    interp: bool,
}

impl Default for Lpc12 {
    fn default() -> Self {
        Self {
            rpt: -1,
            cnt: 0,
            per: 0,
            rng: 1,
            amp: 0,
            f_coef: [0; 6],
            b_coef: [0; 6],
            z: [[0; 2]; 6],
            r: [0; 16],
            interp: false,
        }
    }
}

impl Lpc12 {
    fn decode_amp(r0: u8) -> i32 {
        i32::from(r0 & 0x1F) << ((r0 & 0xE0) >> 5)
    }

    fn regdec(&mut self) {
        self.amp = Self::decode_amp(self.r[0]);
        self.cnt = 0;
        self.per = u32::from(self.r[1]);
        for i in 0..6 {
            self.b_coef[i] = iq(self.r[2 + 2 * i]);
            self.f_coef[i] = iq(self.r[3 + 2 * i]);
        }
        self.interp = self.r[14] != 0 || self.r[15] != 0;
    }

    /// Produce a single output sample, or `None` if the repeat count expired
    /// before a sample could be generated (the caller then fetches the next
    /// frame, as MAME does).
    fn step(&mut self) -> Option<i16> {
        let mut do_int = false;
        let mut samp: u16;
        if self.per != 0 {
            if self.cnt <= 0 {
                self.cnt += self.per as i32;
                samp = self.amp as u16;
                self.rpt -= 1;
                do_int = self.interp;
                self.z = [[0; 2]; 6];
            } else {
                samp = 0;
                self.cnt -= 1;
            }
        } else {
            self.cnt -= 1;
            if self.cnt <= 0 {
                do_int = self.interp;
                self.cnt = PER_NOISE;
                self.rpt -= 1;
                self.z = [[0; 2]; 6];
            }
            let bit = self.rng & 1 != 0;
            self.rng = (self.rng >> 1) ^ (if bit { 0x4001 } else { 0 });
            samp = if bit {
                self.amp as u16
            } else {
                (self.amp as u16).wrapping_neg()
            };
        }

        if do_int {
            self.r[0] = self.r[0].wrapping_add(self.r[14]);
            self.r[1] = self.r[1].wrapping_add(self.r[15]);
            self.amp = Self::decode_amp(self.r[0]);
            self.per = u32::from(self.r[1]);
        }

        if self.rpt <= 0 {
            return None;
        }

        for j in 0..6 {
            let b = (i32::from(self.b_coef[j]) * i32::from(self.z[j][1])) >> 9;
            let f = (i32::from(self.f_coef[j]) * i32::from(self.z[j][0])) >> 8;
            samp = samp.wrapping_add(b as u16).wrapping_add(f as u16);
            self.z[j][1] = self.z[j][0];
            self.z[j][0] = samp as i16;
        }

        Some(limit(samp as i16) << 2)
    }
}

/// The SP0256-AL2 speech processor.
#[derive(Clone)]
pub struct Sp0256 {
    rom: Vec<u8>,
    filt: Lpc12,
    pc: u32,
    stack: u32,
    page: u32,
    ald: u32,
    mode: u32,
    halted: bool,
    lrq: bool,
    silent: bool,
    sby_line: bool,
}

impl Sp0256 {
    /// Build from a raw SP0256-AL2 dump (2 KiB, bit-reversed per byte as
    /// distributed by spatula-city). The ROM is placed at byte `$1000`.
    pub fn new(rom_al2: &[u8]) -> Self {
        let mut rom = vec![0u8; 0x1_0000];
        for (i, &b) in rom_al2.iter().enumerate() {
            let addr = ROM_BASE + i;
            if addr < rom.len() {
                rom[addr] = bitrev8(b);
            }
        }
        let mut chip = Self {
            rom,
            filt: Lpc12::default(),
            pc: 0,
            stack: 0,
            page: 0x1000 << 3,
            ald: 0,
            mode: 0,
            halted: true,
            lrq: true,
            silent: true,
            sby_line: true,
        };
        chip.reset();
        chip
    }

    pub fn reset(&mut self) {
        self.filt = Lpc12::default();
        self.halted = true;
        self.lrq = true;
        self.ald = 0;
        self.pc = 0;
        self.stack = 0;
        self.mode = 0;
        self.page = 0x1000 << 3;
        self.silent = true;
        self.sby_line = true;
    }

    /// True when the host may load a new allophone address (LRQ ready).
    pub fn lrq_ready(&self) -> bool {
        self.lrq
    }

    /// True when the chip is in standby (idle, not speaking).
    pub fn standby(&self) -> bool {
        self.sby_line
    }

    /// True when the chip is fully idle with no command pending.
    pub fn idle(&self) -> bool {
        self.halted && self.lrq
    }

    /// Load an allophone command via the ALD strobe. Dropped if the chip is
    /// busy (the SP0256 has only a single-entry address latch).
    pub fn ald_w(&mut self, data: u8) {
        if !self.lrq {
            return;
        }
        self.lrq = false;
        self.ald = u32::from(data) << 4;
        self.sby_line = false;
    }

    fn getb(&mut self, len: u32) -> u32 {
        let idx0 = (self.pc >> 3) as usize;
        let idx1 = ((self.pc + 8) >> 3) as usize;
        let d0 = u32::from(self.rom[idx0 & 0xffff]);
        let d1 = u32::from(self.rom[idx1 & 0xffff]);
        let data = ((d1 << 8) | d0) >> (self.pc & 7);
        self.pc = self.pc.wrapping_add(len);
        data & ((1u32 << len) - 1)
    }

    /// Emulate the microsequencer until the filter has a repeat block queued
    /// (or the chip halts).
    fn micro(&mut self) {
        let mut guard = 0u32;
        while self.filt.rpt <= 0 {
            guard += 1;
            if guard > 100_000 {
                // Corrupt ROM / runaway: force idle rather than hang.
                self.halted = true;
                self.filt.rpt = -1;
                self.sby_line = true;
                return;
            }

            // Load a pending command when halted.
            if self.halted && !self.lrq {
                self.pc = self.ald | (0x1000 << 3);
                self.halted = false;
                self.lrq = true;
                self.ald = 0;
                self.filt.r = [0; 16];
            }

            if self.halted {
                self.filt.rpt = -1;
                self.lrq = true;
                self.ald = 0;
                self.filt.r = [0; 16];
                self.silent = true;
                self.sby_line = true;
                return;
            }

            let immed4 = self.getb(4) as u8;
            let opcode = self.getb(4) as u8;
            let mut repeat = 0i32;
            let mut ctrl_xfer = false;

            match opcode {
                0x0 => {
                    if immed4 != 0 {
                        // SETPAGE
                        self.page = bitrev32(u32::from(immed4)) >> 13;
                    } else {
                        // RTS, or HLT when the stack is empty
                        let btrg = self.stack;
                        self.stack = 0;
                        if btrg == 0 {
                            self.halted = true;
                            self.pc = 0;
                        } else {
                            self.pc = btrg;
                        }
                        ctrl_xfer = true;
                    }
                }
                0xE | 0xD => {
                    // JMP / JSR
                    let hi = bitrev32(u32::from(immed4)) >> 17;
                    let lo = bitrev32(self.getb(8)) >> 21;
                    let btrg = self.page | hi | lo;
                    ctrl_xfer = true;
                    if opcode == 0xD {
                        self.stack = (self.pc + 7) & !7;
                    }
                    self.pc = btrg;
                }
                0x1 => {
                    // SETMODE
                    self.mode = u32::from(((immed4 & 8) >> 2) | (immed4 & 4) | ((immed4 & 3) << 4));
                }
                _ => {
                    repeat = i32::from(immed4) | (self.mode & 0x30) as i32;
                }
            }

            if opcode != 1 {
                self.mode &= 0xF;
            }

            if ctrl_xfer {
                // AL2 has no SPB640 FIFO; jumps only target ROM.
                let _fifo = self.pc == FIFO_ADDR;
                continue;
            }

            if repeat == 0 {
                continue;
            }

            self.filt.rpt = repeat + 1;

            let base = ((opcode as usize) << 3) | (self.mode as usize & 6);
            let idx0 = DF_IDX[base];
            let idx1 = DF_IDX[base + 1];
            debug_assert!(idx0 >= 0 && idx1 >= idx0);

            let mut i = idx0;
            while i <= idx1 {
                let word = DATAFMT[i as usize];
                let len = cr_len(word);
                let shf = cr_shf(word);
                let prm = cr_prm(word);

                if word & CR_CLRA != 0 {
                    self.filt.r = [0; 16];
                    self.silent = true;
                }
                if word & CR_CLR5 != 0 {
                    self.filt.r[B5 as usize] = 0;
                    self.filt.r[F5 as usize] = 0;
                }

                if len == 0 {
                    i += 1;
                    continue;
                }

                // The field is an 8-bit signed quantity after sign-extension
                // (delta updates) and scaling, exactly like the hardware.
                let mut v = self.getb(len) as i32;
                if word & CR_DELTA != 0 && (v & (1 << (len - 1))) != 0 {
                    v |= -1i32 << len;
                }
                if shf != 0 {
                    v <<= shf;
                }
                let value = v as u8;

                self.silent = false;

                let reg = &mut self.filt.r[prm];
                if word & CR_FIELD != 0 {
                    let keep = ((1u16 << shf) - 1) as u8;
                    *reg = (*reg & keep) | value;
                } else if word & CR_DELTA != 0 {
                    *reg = reg.wrapping_add(value);
                } else {
                    *reg = value;
                }
                i += 1;
            }

            if opcode == 0xF {
                self.silent = true;
                self.filt.r[PR as usize] = PER_PAUSE;
            }

            self.filt.regdec();
            break;
        }
    }

    /// Generate the next internal (~10 kHz) sample.
    pub fn next_sample(&mut self) -> i16 {
        // A fresh frame always yields a sample (its repeat count is >= 2), so
        // at most one expired frame is skipped per call.
        for _ in 0..3 {
            // A pending ALD must be accepted even if the last idle pass left
            // `rpt` in a non-zero state; otherwise the latch is never consumed.
            if self.filt.rpt <= 0 || (self.halted && !self.lrq) {
                self.micro();
            }
            if self.halted {
                return 0;
            }
            if let Some(s) = self.filt.step() {
                return s;
            }
            // The repeat count expired: go straight on to the next frame
            // instead of emitting a filler sample.
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech_rom::SP0256_AL2;

    fn chip() -> Sp0256 {
        Sp0256::new(SP0256_AL2)
    }

    /// FNV-1a over the little-endian 16-bit samples.
    fn fnv(samples: &[i16]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for s in samples {
            for byte in s.to_le_bytes() {
                h ^= u64::from(byte);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// Render one allophone from a freshly reset chip until it halts.
    fn render(code: u8) -> Vec<i16> {
        let mut sp = chip();
        sp.ald_w(code);
        let mut out = Vec::new();
        for _ in 0..100_000 {
            let s = sp.next_sample();
            if sp.halted {
                break;
            }
            out.push(s);
        }
        assert!(sp.idle(), "allophone {code} should finish");
        out
    }

    /// Sample count and FNV-1a hash of every AL2 allophone as rendered by a
    /// MAME port of `sp0256.cpp` (`lpc12_update` + `micro`).
    #[rustfmt::skip]
    const MAME_REFERENCE: [(usize, u64); 64] = [
        (65, 0xf0cfce200093c9ed), (260, 0x7270ce3a3ef261c5),
        (455, 0x10d908cdd619d0dd), (975, 0x24a9dee73ff9901d),
        (2015, 0xec2c165108e27e9d), (2944, 0x04cfce19bc02b19d),
        (1748, 0x91d4722a30605d3b), (552, 0xc331829a1f91e1ed),
        (772, 0xb567074245d80be2), (1485, 0x528e2e2c620aea05),
        (992, 0xc6bf316ca4ef7ca1), (1748, 0x90445be11843a6e4),
        (460, 0x7bd971e009c24d71), (965, 0x92ca9cc141fa2e4b),
        (1288, 0xdbe29bbdc0704246), (552, 0x396c778cf94fca4d),
        (1840, 0x7dd59fc75ffe28cd), (774, 0xec53fd7e13db1bc9),
        (1380, 0xa5d124dfe5a6facb), (1748, 0x9d802ccf16f128b6),
        (2024, 0xf18c3af49cc0fe58), (460, 0x5f2f7a947abebbb3),
        (644, 0xd9b2302ed4132def), (736, 0xb6ffac0680476745),
        (644, 0xe166a719fb8af7e4), (1288, 0x519abd3c197ad8d6),
        (828, 0x1217217a4e8874b5), (896, 0x565fba1163d25c19),
        (368, 0x2020f357fd4dcda0), (1280, 0x304dfe708815e2ff),
        (736, 0x8b4e990e44f7c9a5), (1748, 0xeb36fa1c7b3eee37),
        (2576, 0xe787796af693582f), (728, 0x4e17460d28dc2f76),
        (1116, 0x6c143b57c4389192), (1288, 0x3169c26ca7344725),
        (728, 0x3662fb26d366b771), (1984, 0xe2bbf1985af9aea3),
        (1348, 0xbd5e5643c021a72c), (828, 0x1897c96bb6d75755),
        (1088, 0xb2decc15ecfeda8a), (1362, 0x2e6e905a8589d5a4),
        (1159, 0xb0fcdbc6b5ec9aef), (1496, 0x38add1ff76d4715b),
        (2024, 0x2139e1dca1901a7f), (828, 0x10d2e5dae307b8c4),
        (1472, 0x7506ee748ea9f667), (2484, 0xb8992faa4ccceedb),
        (1456, 0x574f88fbf809bc28), (920, 0x10b86d196c864a24),
        (1477, 0xe0fe13788bb5b23f), (1104, 0xe4ea4618352b3e34),
        (2116, 0x82a27c61ec4bb5f2), (1748, 0xfe03f9a19590d085),
        (1840, 0x87ecadef1923c62b), (640, 0xe6ca5a7bd64171bf),
        (1380, 0xc0b8339661686a1b), (1264, 0xe11ac8a4ae96392c),
        (2392, 0x029bdda07dddba85), (2024, 0x07182f7e23d035e2),
        (2484, 0xc0425a3a3c64d4c2), (700, 0x82fc3231a23aa383),
        (1380, 0x724f44c8d17ca24d), (508, 0x729613f08c5d1f25),
    ];

    #[test]
    fn every_allophone_matches_the_mame_reference() {
        for (code, &(len, hash)) in MAME_REFERENCE.iter().enumerate() {
            let out = render(code as u8);
            assert_eq!(out.len(), len, "sample count of allophone {code}");
            assert_eq!(fnv(&out), hash, "samples of allophone {code}");
        }
    }

    #[test]
    fn expired_repeat_count_emits_no_filler_sample() {
        // OY: 10 frames; the old core added one zero per frame (2954).
        assert_eq!(render(0x05).len(), 2944);
    }

    #[test]
    fn period_register_is_unsigned() {
        // A LOADALL frame with period byte 0xC8 (200 samples per pitch pulse)
        // must be a long pitch period, not a negative one that fires an
        // impulse every sample.
        let mut f = Lpc12::default();
        f.r[AM as usize] = 0x3F; // some amplitude
        f.r[PR as usize] = 0xC8;
        f.regdec();
        assert_eq!(f.per, 200);
        f.rpt = 10;
        let mut impulses = 0;
        for _ in 0..400 {
            if f.cnt <= 0 {
                impulses += 1;
            }
            let _ = f.step();
        }
        assert_eq!(impulses, 2, "one impulse per 201 samples");
        // Interpolation wraps the 8-bit registers instead of growing them.
        let mut g = Lpc12::default();
        g.r[PR as usize] = 0xFF;
        g.r[IP as usize] = 0x02;
        g.r[AM as usize] = 0x01;
        g.regdec();
        g.rpt = 5;
        let _ = g.step(); // impulse + interpolation
        assert_eq!(g.r[PR as usize], 0x01);
        assert_eq!(g.per, 1);
    }

    #[test]
    fn starts_idle() {
        let sp = chip();
        assert!(sp.idle());
        assert!(sp.standby());
        assert!(sp.lrq_ready());
    }

    #[test]
    fn idle_chip_is_silent() {
        let mut sp = chip();
        for _ in 0..1000 {
            assert_eq!(sp.next_sample(), 0);
        }
    }

    #[test]
    fn halted_chip_is_silent_after_speaking() {
        let mut sp = chip();
        sp.ald_w(0x13); // IY, the loudest allophone
        for _ in 0..3000 {
            sp.next_sample();
        }
        assert!(sp.idle());
        for _ in 0..1000 {
            assert_eq!(sp.next_sample(), 0, "no stale filter output");
        }
    }

    #[test]
    fn ald_marks_busy_then_speaks_and_returns_to_standby() {
        let mut sp = chip();
        sp.ald_w(0x0B);
        assert!(!sp.lrq_ready(), "LRQ should be busy right after ALD");
        assert!(!sp.standby(), "SBY should drop while a command is pending");

        let mut peak = 0i32;
        let mut idx = 0;
        for i in 0..20_000 {
            let s = i32::from(sp.next_sample());
            peak = peak.max(s.abs());
            if sp.idle() {
                idx = i;
                break;
            }
        }
        assert!(sp.idle(), "chip should return to standby after speaking");
        assert!(idx > 100, "allophone should last more than a handful of samples");
        assert!(peak > 200, "voiced allophone should produce audible energy (peak={peak})");
    }

    #[test]
    fn pause_allophone_is_short_and_quiet() {
        let out = render(0x00); // PA1
        assert!(out.len() < 200, "PA1 should be a short pause, got {}", out.len());
        assert!(out.iter().all(|&s| s == 0), "PA1 should be silent");
    }

    #[test]
    fn multiple_allophones_play_back_to_back() {
        // HH1 EH LL OW PA2 fed through the LRQ handshake: the concatenation
        // of the individual renders (no gaps, no filler samples).
        let seq = [0x1B, 0x07, 0x2D, 0x35, 0x01];
        let mut sp = chip();
        let mut fed = 0usize;
        let mut out = Vec::new();
        for _ in 0..200_000 {
            if sp.lrq_ready() && fed < seq.len() {
                sp.ald_w(seq[fed]);
                fed += 1;
            }
            let s = sp.next_sample();
            if sp.halted {
                if fed == seq.len() {
                    break;
                }
                continue;
            }
            out.push(s);
        }
        assert_eq!(fed, seq.len(), "all allophones should be accepted");
        assert!(sp.idle(), "should finish the whole sequence");
        let expected: usize = seq.iter().map(|&c| MAME_REFERENCE[c as usize].0).sum();
        assert_eq!(out.len(), expected);
        assert_eq!(fnv(&out), 0xde53_a6f5_d2da_f1d0, "MAME reference stream");
    }
}
