//! General Instrument AY-3-8910 programmable sound generator.
//!
//! Chip model (AY-3-8910 datasheet, cross-checked with MAME `ay8910.cpp` and
//! Ayumi):
//! * All generators run from one tick every 8 master clocks. Tone, noise and
//!   envelope counters count **up** and fire when `count >= period` (period 0
//!   acts as 1), so a period write takes effect immediately.
//! * Tone: the output toggles every `TP` ticks -> `fT = fCLK / (16 * TP)`.
//! * Noise: the 17-bit LFSR (input = bit0 XOR bit3, output = bit0) shifts every
//!   `2 * NP` ticks -> `fN = fCLK / (16 * NP)`.
//! * Envelope: one of 16 levels per `2 * EP` ticks (16 * EP master clocks), so
//!   a full ramp takes `256 * EP` clocks (`fE = fCLK / (256 * EP)`). Writing
//!   R13 restarts the envelope with a full-length first step. All 16 shapes
//!   follow the datasheet (CONT=0 shapes are one ramp, then hold at 0).
//! * Registers are stored masked and read back masked
//!   (`FF 0F FF 0F FF 0F 1F FF 1F 1F 1F FF FF 0F FF FF`). R14/R15 read the port
//!   pins: in input mode the external value ([`Ay38910::set_port_input`], $FF =
//!   pull-ups), in output mode the output latch (no external contention).
//! * The address latch selects the chip only when the latched upper nibble
//!   (A7-A4) is 0; otherwise data reads return $FF and data writes are ignored.
//! * Output levels follow the measured AY-3-8910 DAC curve; level 0 is silent.
//!
//! Audio pipeline: the piecewise-constant chip output is integrated exactly
//! over [`OVERSAMPLE`]x oversampled bins (drift-free integer time base), then
//! decimated to [`AUDIO_SAMPLE_RATE`] by a Blackman-windowed FIR low-pass and
//! DC-blocked. [`Ay38910::tick`] converts host (CPU E) cycles to wall time with
//! the clock set by [`Ay38910::set_host_clock_hz`].

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::OnceLock;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_ADDR: u16 = 0xFF40;
/// Typical AY clock used by the bundled examples (pitch tables assume 1 MHz).
pub const DEFAULT_CHIP_CLOCK_HZ: u32 = 1_000_000;
/// Default rate of the host cycles passed to [`Ay38910::tick`] (CoCo 2 E clock).
/// The machine sets its real (possibly changing) clock with
/// [`Ay38910::set_host_clock_hz`].
pub const HOST_CLOCK_HZ: u32 = 894_886;
pub const AUDIO_SAMPLE_RATE: u32 = 44_100;
/// Output queue capacity (2 s); when full the oldest samples are dropped.
pub const AUDIO_BUFFER_CAP: usize = 88_200;

/// Read-back / storage mask of R0-R15 (unused bits read as 0).
const REG_MASK: [u8; 16] = [
    0xFF, 0x0F, 0xFF, 0x0F, 0xFF, 0x0F, 0x1F, 0xFF, 0x1F, 0x1F, 0x1F, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF,
];

// Register names for the IoPanel debugger view.
const REG_NAMES: [&str; 16] = [
    "AY R0  ToneA Fine", "AY R1  ToneA Coarse", "AY R2  ToneB Fine",
    "AY R3  ToneB Coarse", "AY R4  ToneC Fine",  "AY R5  ToneC Coarse",
    "AY R6  Noise Period", "AY R7  Mixer/IO Dir", "AY R8  AmpA/EnvA",
    "AY R9  AmpB/EnvB",    "AY R10 AmpC/EnvC",    "AY R11 Env Fine",
    "AY R12 Env Coarse",   "AY R13 Env Shape",    "AY R14 PortA Data",
    "AY R15 PortB Data",
];

/// Measured AY-3-8910 output levels (volume 0-15), normalised to 0..1.
/// Values as published with Peter Sovietov's Ayumi; they agree with MAME's
/// resistor model and M. Westcott's measurements. Level 0 is silent.
const DAC_LEVELS: [f32; 16] = [
    0.0, 0.009_995, 0.014_450, 0.021_057, 0.030_701, 0.045_548, 0.064_500, 0.107_362,
    0.126_589, 0.204_990, 0.292_210, 0.372_839, 0.492_531, 0.635_325, 0.805_585, 1.0,
];

/// Mix gain for three unipolar channels (DC is removed after decimation).
const OUTPUT_GAIN: f32 = 0.55 / 3.0;
/// Master clocks per generator tick.
const CLOCKS_PER_TICK: u64 = 8;
/// Integration bins per output sample.
const OVERSAMPLE: u32 = 8;
const BIN_RATE: u64 = AUDIO_SAMPLE_RATE as u64 * OVERSAMPLE as u64;
/// One generator tick in integration units of `1 / (chip_hz * BIN_RATE)` s;
/// one bin is `chip_hz` units. Exact for any chip clock.
const TICK_UNITS: u64 = CLOCKS_PER_TICK * BIN_RATE;
const FIR_TAPS: usize = 192;
/// -6 dB point of the decimation filter.
const FIR_CUTOFF_HZ: f64 = 21_000.0;
/// One-pole DC blocker coefficient (~21 Hz at 44.1 kHz).
const DC_BLOCK_R: f32 = 0.997;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AyConfig {
    pub enabled: bool,
    pub base_addr: u16,
    pub chip_clock_hz: u32,
}

impl Default for AyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_addr: DEFAULT_BASE_ADDR,
            chip_clock_hz: DEFAULT_CHIP_CLOCK_HZ,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AyStateDto {
    pub config: AyConfig,
    /// R0-R15 as stored (masked; R14/R15 are the output latches).
    pub registers: [u8; 16],
    pub selected_register: u8,
    pub port_a_in: u8,
    pub port_b_in: u8,
    /// Last byte written to the address latch.
    #[serde(default)]
    pub address_latch: u8,
    /// Latched upper nibble was 0: the chip responds to data accesses.
    #[serde(default)]
    pub chip_selected: bool,
    /// Current envelope generator level (0-15).
    #[serde(default)]
    pub envelope_level: u8,
}

/// FIR history (double-length ring so the dot product reads a contiguous slice).
#[derive(Debug, Clone)]
struct Decimator {
    hist: [f32; 2 * FIR_TAPS],
    pos: usize,
}

impl Default for Decimator {
    fn default() -> Self {
        Self {
            hist: [0.0; 2 * FIR_TAPS],
            pos: 0,
        }
    }
}

impl Decimator {
    fn push(&mut self, x: f32) {
        self.hist[self.pos] = x;
        self.hist[self.pos + FIR_TAPS] = x;
        self.pos = (self.pos + 1) % FIR_TAPS;
    }

    fn output(&self) -> f32 {
        let window = &self.hist[self.pos..self.pos + FIR_TAPS];
        window
            .iter()
            .zip(fir_coeffs().iter())
            .map(|(x, h)| x * h)
            .sum()
    }
}

/// Blackman-windowed sinc low-pass at `BIN_RATE`, unity DC gain.
fn fir_coeffs() -> &'static [f32; FIR_TAPS] {
    static COEFFS: OnceLock<[f32; FIR_TAPS]> = OnceLock::new();
    COEFFS.get_or_init(|| {
        use std::f64::consts::PI;
        let n = FIR_TAPS as f64;
        let fc = FIR_CUTOFF_HZ / BIN_RATE as f64;
        let mid = (n - 1.0) / 2.0;
        let mut taps = [0.0_f64; FIR_TAPS];
        for (i, tap) in taps.iter_mut().enumerate() {
            let x = i as f64 - mid;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * PI * fc * x).sin() / (PI * x)
            };
            let phase = 2.0 * PI * i as f64 / (n - 1.0);
            let window = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
            *tap = sinc * window;
        }
        let sum: f64 = taps.iter().sum();
        let mut out = [0.0_f32; FIR_TAPS];
        for (o, t) in out.iter_mut().zip(taps.iter()) {
            *o = (t / sum) as f32;
        }
        out
    })
}

/// Snapshot layout of [`AyState`]; older snapshots (no field) are migrated.
const STATE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AyState {
    /// Missing in pre-v2 snapshots, which then deserialize as 0.
    #[serde(default)]
    version: u32,
    registers: [u8; 16],
    /// Last byte written to the address latch.
    address_latch: u8,
    selected_register: u8,
    /// The latched upper nibble matched (0): data accesses reach the chip.
    active: bool,
    port_a_input: u8,
    port_b_input: u8,
    // Generators (one tick = 8 master clocks).
    tone_count: [u32; 3],
    tone_output: [bool; 3],
    noise_count: u32,
    noise_lfsr: u32,
    env_count: u32,
    /// Counts down 15..0 within a ramp; level = step ^ attack.
    env_step: i8,
    env_attack: u8,
    env_hold: bool,
    env_alternate: bool,
    env_holding: bool,
    /// Mixed output level of the current tick (unipolar, before filtering).
    level: f32,
    // Timing.
    host_clock_hz: u32,
    /// `+= host_cycles * AUDIO_SAMPLE_RATE`; one sample is due per `host_clock_hz`.
    host_acc: u64,
    /// Samples already rendered ahead of the host timeline by
    /// `take_samples` / `fill_audio_to`; `tick` skips that many.
    lead_samples: u64,
    /// Integration units left in the current generator tick.
    tick_remaining: u64,
    dc_x: f32,
    dc_y: f32,
    #[serde(skip)]
    decimator: Decimator,
    #[serde(skip)]
    audio: VecDeque<f32>,
}

impl Default for AyState {
    fn default() -> Self {
        let mut state = Self {
            version: STATE_VERSION,
            registers: [0; 16],
            address_latch: 0,
            selected_register: 0,
            active: false,
            port_a_input: 0xFF,
            port_b_input: 0xFF,
            tone_count: [0; 3],
            tone_output: [false; 3],
            noise_count: 0,
            noise_lfsr: 1,
            env_count: 0,
            env_step: 15,
            env_attack: 0,
            env_hold: true,
            env_alternate: false,
            env_holding: false,
            level: 0.0,
            host_clock_hz: HOST_CLOCK_HZ,
            host_acc: 0,
            lead_samples: 0,
            tick_remaining: TICK_UNITS,
            dc_x: 0.0,
            dc_y: 0.0,
            decimator: Decimator::default(),
            audio: VecDeque::new(),
        };
        env_restart(&mut state, 0);
        state
    }
}

#[derive(Debug)]
pub struct Ay38910 {
    config: AyConfig,
    state: RefCell<AyState>,
}

#[derive(Serialize, Deserialize)]
struct AySnapshot {
    config: AyConfig,
    state: AyState,
}

impl Serialize for Ay38910 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AySnapshot {
            config: self.config,
            state: self.state.borrow().clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Ay38910 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut snap = AySnapshot::deserialize(deserializer)?;
        if snap.state.version < STATE_VERSION {
            migrate_legacy_state(&mut snap.state);
        }
        Ok(Self {
            config: snap.config,
            state: RefCell::new(snap.state),
        })
    }
}

/// Pre-v2 snapshots stored unmasked registers, had no chip select and a
/// different envelope model: mask, select, and restart the envelope from R13.
fn migrate_legacy_state(state: &mut AyState) {
    for (reg, mask) in state.registers.iter_mut().zip(REG_MASK) {
        *reg &= mask;
    }
    state.selected_register &= 0x0F;
    state.address_latch = state.selected_register;
    state.active = true;
    state.tone_count = [0; 3];
    state.noise_count = 0;
    if state.noise_lfsr & 0x1_FFFF == 0 {
        state.noise_lfsr = 1;
    }
    state.tick_remaining = TICK_UNITS;
    let shape = state.registers[13];
    env_restart(state, shape);
    state.level = mix_level(state);
    state.version = STATE_VERSION;
}

impl Clone for Ay38910 {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            state: RefCell::new(self.state.borrow().clone()),
        }
    }
}

impl Default for Ay38910 {
    fn default() -> Self {
        Self::new(AyConfig::default())
    }
}

impl Ay38910 {
    pub fn new(config: AyConfig) -> Self {
        Self {
            config,
            state: RefCell::new(AyState::default()),
        }
    }

    pub fn config(&self) -> AyConfig {
        self.config
    }

    pub fn set_config(&mut self, config: AyConfig) {
        let was_enabled = self.config.enabled;
        self.config = config;
        if config.enabled && !was_enabled {
            self.apply_power_on();
        } else if !config.enabled {
            // Clear audio buffer when disabled.
            let mut state = self.state.borrow_mut();
            state.audio.clear();
            state.lead_samples = 0;
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Rate of the cycles passed to [`Self::tick`] (the machine's current CPU E
    /// clock). Only affects how many samples are due per host cycle; the pitch
    /// follows `chip_clock_hz`.
    pub fn set_host_clock_hz(&self, hz: u32) {
        self.state.borrow_mut().host_clock_hz = hz.max(1);
    }

    pub fn host_clock_hz(&self) -> u32 {
        self.state.borrow().host_clock_hz
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled
            && (addr == self.config.base_addr || addr == self.config.base_addr.wrapping_add(1))
    }

    /// CPU read. `base` (address latch) is write-only and reads $FF; `base+1`
    /// reads the selected register (masked) or the port pins for R14/R15.
    /// Reads have no side effects on the AY-3-8910.
    pub fn read(&self, addr: u16) -> u8 {
        if !self.handles(addr) || addr == self.config.base_addr {
            return 0xFF;
        }
        data_read(&self.state.borrow())
    }

    /// Side-effect-free register view for the debugger (identical to [`Self::read`]).
    pub fn peek(&self, addr: u16) -> u8 {
        self.read(addr)
    }

    /// Hardware /RESET: R0-R15 = 0 (both ports become inputs), address latch
    /// cleared and the chip deselected until the next latch write, generators
    /// restarted. Port input pins, the host clock and the audio pipeline
    /// (queue, filters, timeline) are kept so playback stays continuous.
    pub fn reset(&mut self) {
        let mut state = self.state.borrow_mut();
        state.address_latch = 0;
        state.selected_register = 0;
        state.active = false;
        state.tone_count = [0; 3];
        state.tone_output = [false; 3];
        state.noise_count = 0;
        state.noise_lfsr = 1;
        for reg in 0..16 {
            write_register(&mut state, reg, 0);
        }
        state.env_count = 0;
        state.level = mix_level(&state);
    }

    pub fn write(&self, addr: u16, value: u8) {
        if !self.handles(addr) {
            return;
        }
        let mut state = self.state.borrow_mut();
        if addr == self.config.base_addr {
            // Address latch: A7-A4 must match the mask-programmed code (0000).
            state.address_latch = value;
            state.active = value >> 4 == 0;
            if state.active {
                state.selected_register = value & 0x0F;
            }
            return;
        }
        if !state.active {
            return;
        }
        let reg = usize::from(state.selected_register & 0x0F);
        write_register(&mut state, reg, value);
    }

    /// Advance by `host_cycles` host (CPU E) cycles.
    ///
    /// Output length is locked to wall time (`host_clock_hz` ->
    /// [`AUDIO_SAMPLE_RATE`]); the chip pitch follows `chip_clock_hz`.
    /// Samples already rendered ahead by [`Self::take_samples`] or
    /// [`Self::fill_audio_to`] are skipped, so the chip timeline never runs twice.
    pub fn tick(&self, host_cycles: u32) {
        if !self.config.enabled || host_cycles == 0 {
            return;
        }
        let chip = u64::from(self.config.chip_clock_hz.max(1));
        let mut state = self.state.borrow_mut();
        let host = u64::from(state.host_clock_hz.max(1));
        state.host_acc = state
            .host_acc
            .saturating_add(u64::from(host_cycles) * u64::from(AUDIO_SAMPLE_RATE));
        while state.host_acc >= host {
            state.host_acc -= host;
            if state.lead_samples > 0 {
                state.lead_samples -= 1;
                continue;
            }
            let sample = render_sample(&mut state, chip);
            push_sample(&mut state, sample);
        }
    }

    pub fn poll_irq(&self) -> bool {
        false
    }

    /// Debugger view: the address latch and data port at their bus addresses,
    /// followed by R0-R15 (accessed through the data port at `base+1`).
    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        let base = self.config.base_addr;
        let data = base.wrapping_add(1);
        let sel = state.selected_register & 0x0F;
        let mut regs = Vec::with_capacity(18);
        regs.push(IoRegisterView {
            address: base,
            name: if state.active {
                format!("AY Addr Latch (R{sel})")
            } else {
                "AY Addr Latch (deselected)".to_string()
            },
            value: state.address_latch,
        });
        regs.push(IoRegisterView {
            address: data,
            name: format!("AY Data (R{sel})"),
            value: data_read(&state),
        });
        regs.extend(REG_NAMES.iter().enumerate().map(|(i, name)| IoRegisterView {
            address: data,
            name: name.to_string(),
            value: state.registers[i],
        }));
        regs
    }

    pub fn state_snapshot(&self) -> AyStateDto {
        let state = self.state.borrow();
        AyStateDto {
            config: self.config,
            registers: state.registers,
            selected_register: state.selected_register,
            port_a_in: state.port_a_input,
            port_b_in: state.port_b_input,
            address_latch: state.address_latch,
            chip_selected: state.active,
            envelope_level: envelope_level(&state),
        }
    }

    /// Everything queued since the last drain (at most [`AUDIO_BUFFER_CAP`]
    /// samples; older samples were dropped).
    pub fn drain_audio(&self) -> Vec<f32> {
        let mut state = self.state.borrow_mut();
        state.audio.drain(..).collect()
    }

    /// Render samples into the queue until it holds `target_count` (capped at
    /// [`AUDIO_BUFFER_CAP`]). The rendered time is credited against the host
    /// timeline, so later [`Self::tick`] calls do not render it again.
    pub fn fill_audio_to(&self, target_count: usize) {
        if !self.config.enabled {
            return;
        }
        let target = target_count.min(AUDIO_BUFFER_CAP);
        let chip = u64::from(self.config.chip_clock_hz.max(1));
        let mut state = self.state.borrow_mut();
        while state.audio.len() < target {
            let sample = render_sample(&mut state, chip);
            push_sample(&mut state, sample);
            state.lead_samples += 1;
        }
    }

    /// Render exactly `count` samples and return them (tests / offline
    /// catch-up); samples queued by [`Self::tick`] are left alone. Like
    /// [`Self::fill_audio_to`], the rendered time is credited against the host
    /// timeline.
    pub fn take_samples(&self, count: usize) -> Vec<f32> {
        if !self.config.enabled || count == 0 {
            return Vec::new();
        }
        let chip = u64::from(self.config.chip_clock_hz.max(1));
        let mut state = self.state.borrow_mut();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(render_sample(&mut state, chip));
        }
        state.lead_samples += count as u64;
        out
    }

    /// Set the external input value on an I/O port (Port A = 'a', Port B = 'b').
    /// Read back through R14/R15 while R7 marks the port as an input.
    pub fn set_port_input(&self, port: char, value: u8) {
        let mut state = self.state.borrow_mut();
        match port {
            'a' | 'A' => state.port_a_input = value,
            'b' | 'B' => state.port_b_input = value,
            _ => {}
        }
    }

    fn apply_power_on(&mut self) {
        let mut state = self.state.borrow_mut();
        let host_clock_hz = state.host_clock_hz;
        *state = AyState::default();
        state.host_clock_hz = host_clock_hz;
    }
}

/// Data-port read of the selected register.
fn data_read(state: &AyState) -> u8 {
    if !state.active {
        return 0xFF; // chip not selected: bus floats
    }
    let reg = usize::from(state.selected_register & 0x0F);
    match reg {
        14 if state.registers[7] & 0x40 == 0 => state.port_a_input,
        15 if state.registers[7] & 0x80 == 0 => state.port_b_input,
        _ => state.registers[reg],
    }
}

fn write_register(state: &mut AyState, reg: usize, value: u8) {
    let value = value & REG_MASK[reg];
    state.registers[reg] = value;
    if reg == 13 {
        env_restart(state, value);
    }
    // Volume / mixer changes are heard from the current tick on.
    state.level = mix_level(state);
}

/// R13 write: select the shape and restart with a full-length first step.
/// CONT=0 shapes are mapped to their CONT=1 equivalents (hold, and ALT=ATT so
/// the held level is 0).
fn env_restart(state: &mut AyState, shape: u8) {
    state.env_attack = if shape & 0x04 != 0 { 0x0F } else { 0x00 };
    if shape & 0x08 == 0 {
        state.env_hold = true;
        state.env_alternate = state.env_attack != 0;
    } else {
        state.env_hold = shape & 0x01 != 0;
        state.env_alternate = shape & 0x02 != 0;
    }
    state.env_step = 15;
    state.env_holding = false;
    state.env_count = 0;
}

fn envelope_level(state: &AyState) -> u8 {
    (state.env_step as u8 ^ state.env_attack) & 0x0F
}

fn env_advance(state: &mut AyState) {
    state.env_step -= 1;
    if state.env_step >= 0 {
        return;
    }
    if state.env_alternate {
        state.env_attack ^= 0x0F;
    }
    if state.env_hold {
        state.env_holding = true;
        state.env_step = 0;
    } else {
        state.env_step = 15;
    }
}

/// One generator tick (8 master clocks).
fn tick_generators(state: &mut AyState) {
    let regs = state.registers;
    for ch in 0..3 {
        let period = ((u32::from(regs[2 * ch + 1]) << 8) | u32::from(regs[2 * ch])).max(1);
        state.tone_count[ch] += 1;
        if state.tone_count[ch] >= period {
            state.tone_count[ch] = 0;
            state.tone_output[ch] = !state.tone_output[ch];
        }
    }

    let noise_period = u32::from(regs[6]).max(1);
    state.noise_count += 1;
    if state.noise_count >= 2 * noise_period {
        state.noise_count = 0;
        let feedback = (state.noise_lfsr ^ (state.noise_lfsr >> 3)) & 1;
        state.noise_lfsr = ((state.noise_lfsr >> 1) | (feedback << 16)) & 0x1_FFFF;
    }

    if !state.env_holding {
        let env_period = ((u32::from(regs[12]) << 8) | u32::from(regs[11])).max(1);
        state.env_count += 1;
        if state.env_count >= 2 * env_period {
            state.env_count = 0;
            env_advance(state);
        }
    }

    state.level = mix_level(state);
}

/// Mixer + DAC: per channel `(tone | tone_off) & (noise | noise_off)` gates
/// the fixed or envelope volume.
fn mix_level(state: &AyState) -> f32 {
    let regs = &state.registers;
    let mixer = regs[7];
    let noise = state.noise_lfsr & 1 != 0;
    let env = envelope_level(state);
    let mut mixed = 0.0_f32;
    for ch in 0..3 {
        let tone_off = mixer & (1 << ch) != 0;
        let noise_off = mixer & (1 << (ch + 3)) != 0;
        if (state.tone_output[ch] || tone_off) && (noise || noise_off) {
            let amp = regs[8 + ch];
            let volume = if amp & 0x10 != 0 { env } else { amp & 0x0F };
            mixed += DAC_LEVELS[usize::from(volume)];
        }
    }
    mixed * OUTPUT_GAIN
}

/// Exact area integral of the output over one bin (`chip_hz` units).
fn integrate_bin(state: &mut AyState, chip_hz: u64) -> f32 {
    if state.tick_remaining == 0 || state.tick_remaining > TICK_UNITS {
        state.tick_remaining = TICK_UNITS;
    }
    let mut need = chip_hz;
    let mut acc = 0.0_f64;
    while need > 0 {
        let take = need.min(state.tick_remaining);
        acc += f64::from(state.level) * take as f64;
        need -= take;
        state.tick_remaining -= take;
        if state.tick_remaining == 0 {
            tick_generators(state);
            state.tick_remaining = TICK_UNITS;
        }
    }
    (acc / chip_hz as f64) as f32
}

/// Advance the chip by one output sample period and return the filtered sample.
fn render_sample(state: &mut AyState, chip_hz: u64) -> f32 {
    for _ in 0..OVERSAMPLE {
        let bin = integrate_bin(state, chip_hz);
        state.decimator.push(bin);
    }
    let y = state.decimator.output();
    dc_block(state, y)
}

fn dc_block(state: &mut AyState, x: f32) -> f32 {
    let y = x - state.dc_x + DC_BLOCK_R * state.dc_y;
    state.dc_x = x;
    state.dc_y = y;
    y
}

fn push_sample(state: &mut AyState, sample: f32) {
    if state.audio.len() >= AUDIO_BUFFER_CAP {
        state.audio.pop_front();
    }
    state.audio.push_back(sample);
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u16 = 0xFF40;
    const DATA: u16 = 0xFF41;

    fn ay_with_clock(chip_clock_hz: u32) -> Ay38910 {
        Ay38910::new(AyConfig {
            enabled: true,
            base_addr: BASE,
            chip_clock_hz,
        })
    }

    fn enabled_ay() -> Ay38910 {
        ay_with_clock(DEFAULT_CHIP_CLOCK_HZ)
    }

    fn set(ay: &Ay38910, reg: u8, value: u8) {
        ay.write(BASE, reg);
        ay.write(DATA, value);
    }

    fn get(ay: &Ay38910, reg: u8) -> u8 {
        ay.write(BASE, reg);
        ay.read(DATA)
    }

    /// Run `n` generator ticks (8 master clocks each) directly.
    fn run_ticks(ay: &Ay38910, n: u32) {
        let mut state = ay.state.borrow_mut();
        for _ in 0..n {
            tick_generators(&mut state);
        }
    }

    fn env_level(ay: &Ay38910) -> u8 {
        ay.state_snapshot().envelope_level
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn rising_crossings(samples: &[f32]) -> u32 {
        samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count() as u32
    }

    /// Tone A only, fixed volume 15.
    fn tone_a(ay: &Ay38910, period: u16) {
        set(ay, 0, (period & 0xFF) as u8);
        set(ay, 1, (period >> 8) as u8);
        set(ay, 8, 0x0F);
        set(ay, 7, 0x3E);
    }

    #[test]
    fn address_latch_selects_register() {
        let ay = enabled_ay();
        ay.write(BASE, 7); // select R7
        ay.write(DATA, 0x38); // all noise disabled
        let snap = ay.state_snapshot();
        assert_eq!(snap.selected_register, 7);
        assert_eq!(snap.registers[7], 0x38);
        assert!(snap.chip_selected);
    }

    #[test]
    fn every_register_reads_back_with_datasheet_mask() {
        let ay = enabled_ay();
        for reg in 0..16u8 {
            set(&ay, reg, 0xFF);
        }
        // R7 = $FF: both ports are outputs, so R14/R15 read their latches.
        for reg in 0..16u8 {
            assert_eq!(get(&ay, reg), REG_MASK[usize::from(reg)], "R{reg}");
        }
        assert_eq!(
            ay.state_snapshot().registers,
            REG_MASK,
            "snapshot shows the masked (stored) values"
        );
    }

    #[test]
    fn mixer_read_modify_write_keeps_channels() {
        let ay = enabled_ay();
        set(&ay, 7, 0x38); // tones on, noise off
        let r7 = get(&ay, 7);
        assert_eq!(r7, 0x38, "R7 must be readable (not $FF)");
        set(&ay, 7, r7 | 0x40); // make port A an output
        assert_eq!(ay.state_snapshot().registers[7] & 0x07, 0, "tones stay enabled");
    }

    #[test]
    fn latch_with_upper_nibble_deselects_chip() {
        let ay = enabled_ay();
        set(&ay, 7, 0x3F);
        ay.write(BASE, 0x17); // A7-A4 = 0001: another chip's address
        assert!(!ay.state_snapshot().chip_selected);
        assert_eq!(ay.read(DATA), 0xFF, "deselected chip floats the bus");
        ay.write(DATA, 0x00);
        assert_eq!(ay.state_snapshot().registers[7], 0x3F, "write must be ignored");
        assert_eq!(ay.state_snapshot().selected_register, 7, "latch keeps R7");
        ay.write(BASE, 0x07);
        assert_eq!(ay.read(DATA), 0x3F);
    }

    #[test]
    fn chip_is_deselected_after_power_on_until_latched() {
        let ay = enabled_ay();
        assert_eq!(ay.read(DATA), 0xFF);
        ay.write(DATA, 0x12);
        assert_eq!(ay.state_snapshot().registers, [0; 16]);
    }

    #[test]
    fn address_latch_port_reads_ff() {
        let ay = enabled_ay();
        ay.write(BASE, 3);
        assert_eq!(ay.read(BASE), 0xFF);
    }

    #[test]
    fn port_a_input_read_when_direction_is_input() {
        let ay = enabled_ay();
        set(&ay, 7, 0x00); // bit 6 = 0 -> port A input
        ay.set_port_input('a', 0x5A);
        assert_eq!(get(&ay, 14), 0x5A);
    }

    #[test]
    fn port_a_read_returns_output_register_when_direction_output() {
        let ay = enabled_ay();
        set(&ay, 7, 0x40); // bit 6 = 1 -> port A output
        set(&ay, 14, 0x99);
        assert_eq!(get(&ay, 14), 0x99);
        ay.set_port_input('a', 0x12);
        assert_eq!(get(&ay, 14), 0x99, "output mode reads the latch");
    }

    #[test]
    fn port_b_follows_r7_bit7() {
        let ay = enabled_ay();
        ay.set_port_input('b', 0x33);
        set(&ay, 15, 0xC4);
        set(&ay, 7, 0x00);
        assert_eq!(get(&ay, 15), 0x33);
        set(&ay, 7, 0x80);
        assert_eq!(get(&ay, 15), 0xC4);
    }

    #[test]
    fn peek_matches_read_without_side_effects() {
        let ay = enabled_ay();
        set(&ay, 2, 0xAB);
        ay.write(BASE, 2);
        let before = ay.state_snapshot().registers;
        assert_eq!(ay.peek(DATA), 0xAB);
        assert_eq!(ay.peek(BASE), 0xFF);
        assert_eq!(ay.state_snapshot().registers, before);
    }

    #[test]
    fn all_sixteen_envelope_shapes_match_datasheet() {
        let decay: Vec<u8> = (0..16).rev().collect();
        let attack: Vec<u8> = (0..16).collect();
        let low = vec![0u8; 16];
        let high = vec![15u8; 16];
        let expect = |a: &Vec<u8>, b: &Vec<u8>, c: &Vec<u8>| -> Vec<u8> {
            a.iter().chain(b).chain(c).copied().collect()
        };
        for shape in 0..16u8 {
            let expected = match shape {
                0x00..=0x03 | 0x09 => expect(&decay, &low, &low),
                0x04..=0x07 | 0x0F => expect(&attack, &low, &low),
                0x08 => expect(&decay, &decay, &decay),
                0x0A => expect(&decay, &attack, &decay),
                0x0B => expect(&decay, &high, &high),
                0x0C => expect(&attack, &attack, &attack),
                0x0D => expect(&attack, &high, &high),
                0x0E => expect(&attack, &decay, &attack),
                _ => unreachable!(),
            };
            let ay = enabled_ay();
            set(&ay, 11, 1); // EP = 1 -> one step per 2 ticks (16 clocks)
            set(&ay, 12, 0);
            set(&ay, 13, shape);
            let mut seen = Vec::new();
            for _ in 0..48 {
                seen.push(env_level(&ay));
                run_ticks(&ay, 2);
            }
            assert_eq!(seen, expected, "envelope shape ${shape:02X}");
        }
    }

    #[test]
    fn envelope_steps_every_16_ep_master_clocks() {
        let ay = enabled_ay();
        set(&ay, 11, 0xE8);
        set(&ay, 12, 0x03); // EP = 1000 -> 16,000 clocks = 2,000 ticks per step
        set(&ay, 13, 0x00);
        run_ticks(&ay, 1999);
        assert_eq!(env_level(&ay), 15);
        run_ticks(&ay, 1);
        assert_eq!(env_level(&ay), 14);
        // A full ramp takes 256 * EP clocks: 0.256 s at 1 MHz (aymusic example).
        run_ticks(&ay, 2000 * 14);
        assert_eq!(env_level(&ay), 0);
    }

    #[test]
    fn envelope_rate_in_real_time() {
        let ay = enabled_ay();
        set(&ay, 11, 0xE8);
        set(&ay, 12, 0x03);
        set(&ay, 13, 0x00);
        // 0.1 s = 100,000 clocks = 12,500 ticks = 6 full steps.
        let _ = ay.take_samples(4_410);
        assert_eq!(env_level(&ay), 9);
        // 0.256 s: the decay has finished and holds at 0.
        let _ = ay.take_samples(11_290 - 4_410);
        assert_eq!(env_level(&ay), 0);
    }

    #[test]
    fn r13_write_restarts_with_full_length_first_step() {
        let ay = enabled_ay();
        set(&ay, 11, 10); // EP = 10 -> 20 ticks per step
        set(&ay, 12, 0);
        set(&ay, 13, 0x00);
        run_ticks(&ay, 15); // part-way through the first step
        set(&ay, 13, 0x00);
        assert_eq!(env_level(&ay), 15);
        run_ticks(&ay, 19);
        assert_eq!(env_level(&ay), 15, "first step after R13 must be full length");
        run_ticks(&ay, 1);
        assert_eq!(env_level(&ay), 14);
    }

    #[test]
    fn envelope_period_change_takes_effect_immediately() {
        let ay = enabled_ay();
        set(&ay, 11, 0xFF);
        set(&ay, 12, 0xFF); // EP = 65535
        set(&ay, 13, 0x08);
        run_ticks(&ay, 100);
        set(&ay, 12, 0);
        set(&ay, 11, 1); // EP = 1: count (100) >= 2 -> steps on the next tick
        run_ticks(&ay, 1);
        assert_eq!(env_level(&ay), 14);
    }

    #[test]
    fn noise_lfsr_shifts_every_16_np_master_clocks() {
        let ay = enabled_ay();
        set(&ay, 6, 5); // NP = 5 -> shift every 10 ticks (fN = fCLK / 80)
        let mut shifts = 0;
        let mut last = ay.state.borrow().noise_lfsr;
        for _ in 0..1000 {
            run_ticks(&ay, 1);
            let now = ay.state.borrow().noise_lfsr;
            if now != last {
                shifts += 1;
            }
            last = now;
        }
        assert_eq!(shifts, 100);
    }

    #[test]
    fn noise_period_zero_acts_as_one() {
        let ay = enabled_ay();
        set(&ay, 6, 0);
        let start = ay.state.borrow().noise_lfsr;
        run_ticks(&ay, 1);
        assert_eq!(ay.state.borrow().noise_lfsr, start);
        run_ticks(&ay, 1);
        assert_ne!(ay.state.borrow().noise_lfsr, start);
    }

    #[test]
    fn noise_lfsr_is_17_bit_maximal_length() {
        let mut state = AyState::default();
        let start = state.noise_lfsr;
        let mut period = 0u32;
        loop {
            let feedback = (state.noise_lfsr ^ (state.noise_lfsr >> 3)) & 1;
            state.noise_lfsr = ((state.noise_lfsr >> 1) | (feedback << 16)) & 0x1_FFFF;
            period += 1;
            if state.noise_lfsr == start {
                break;
            }
        }
        assert_eq!(period, (1 << 17) - 1);
    }

    #[test]
    fn tone_period_change_takes_effect_immediately() {
        let ay = enabled_ay();
        set(&ay, 0, 0xFF);
        set(&ay, 1, 0x0F); // TP = 4095
        run_ticks(&ay, 100);
        let before = ay.state.borrow().tone_output[0];
        set(&ay, 1, 0x00);
        set(&ay, 0, 0x01); // TP = 1
        run_ticks(&ay, 1);
        assert_ne!(ay.state.borrow().tone_output[0], before, "must toggle on the next tick");
        run_ticks(&ay, 1);
        assert_eq!(ay.state.borrow().tone_output[0], before);
    }

    #[test]
    fn tone_toggles_every_tp_ticks() {
        let ay = enabled_ay();
        set(&ay, 2, 3); // channel B, TP = 3
        let mut toggles = Vec::new();
        let mut last = ay.state.borrow().tone_output[1];
        for tick in 1..=12 {
            run_ticks(&ay, 1);
            let now = ay.state.borrow().tone_output[1];
            if now != last {
                toggles.push(tick);
            }
            last = now;
        }
        assert_eq!(toggles, vec![3, 6, 9, 12]);
    }

    #[test]
    fn take_samples_produces_audio() {
        let ay = enabled_ay();
        tone_a(&ay, 200);
        let audio = ay.take_samples(4410); // 100 ms
        assert_eq!(audio.len(), 4410);
        assert!(
            audio.iter().any(|&s| s.abs() > 1e-6),
            "audio should contain non-zero samples"
        );
    }

    #[test]
    fn tone_frequency_matches_datasheet() {
        // fT = fCLOCK / (16 * TP). With TP=100 at 1 MHz -> 625 Hz.
        let ay = enabled_ay();
        let period: u16 = 100;
        let expected_hz = f64::from(DEFAULT_CHIP_CLOCK_HZ) / (16.0 * f64::from(period));
        tone_a(&ay, period);

        let n = AUDIO_SAMPLE_RATE as usize;
        ay.fill_audio_to(n);
        let audio = ay.drain_audio();
        assert_eq!(audio.len(), n);
        // Skip the filter/DC-blocker start-up transient.
        let measured = f64::from(rising_crossings(&audio[4410..])) / 0.9;
        let err = (measured - expected_hz).abs() / expected_hz;
        assert!(
            err < 0.01,
            "tone frequency off: measured {measured:.1} Hz, expected {expected_hz:.1} Hz"
        );
    }

    #[test]
    fn tick_emits_realtime_samples_independent_of_chip_clock() {
        for chip in [894_886_u32, 1_000_000, 1_773_400, 2_000_000] {
            let ay = ay_with_clock(chip);
            ay.tick(HOST_CLOCK_HZ);
            let n = ay.drain_audio().len();
            assert_eq!(
                n, AUDIO_SAMPLE_RATE as usize,
                "chip {chip} Hz should emit exactly 1 s of audio per host second, got {n}"
            );
        }
    }

    #[test]
    fn host_clock_sets_sample_count_but_not_pitch() {
        let render = |host_hz: u32| {
            let ay = enabled_ay();
            ay.set_host_clock_hz(host_hz);
            tone_a(&ay, 227);
            // Feed one second in CPU-instruction-sized chunks.
            let mut left = host_hz;
            while left > 0 {
                let step = left.min(7);
                ay.tick(step);
                left -= step;
            }
            ay.drain_audio()
        };
        let coco = render(894_886);
        let sbc = render(1_843_200);
        assert_eq!(sbc.len(), AUDIO_SAMPLE_RATE as usize);
        assert_eq!(coco.len(), AUDIO_SAMPLE_RATE as usize);
        assert_eq!(coco, sbc, "same chip clock -> identical waveform and pitch");

        let ay = enabled_ay();
        assert_eq!(ay.host_clock_hz(), HOST_CLOCK_HZ, "default host clock");
        ay.set_host_clock_hz(2 * 894_886); // SAM speed-up doubles the E clock
        ay.tick(894_886);
        assert_eq!(ay.drain_audio().len(), AUDIO_SAMPLE_RATE as usize / 2);
    }

    #[test]
    fn host_clock_survives_snapshot_and_defaults_for_old_snapshots() {
        let ay = enabled_ay();
        ay.set_host_clock_hz(1_843_200);
        let json = serde_json::to_value(&ay).unwrap();
        let back: Ay38910 = serde_json::from_value(json).unwrap();
        assert_eq!(back.host_clock_hz(), 1_843_200);
        assert!(!back.state_snapshot().chip_selected, "current snapshots are not migrated");

        // Pre-v2 layout: unmasked registers, no chip select, old envelope fields.
        let old = serde_json::json!({
            "config": { "enabled": true, "base_addr": 0xFF40, "chip_clock_hz": 1_000_000 },
            "state": {
                "registers": [1, 0xF2, 3, 4, 5, 6, 0xE7, 0x38, 9, 10, 11, 12, 13, 0x1D, 15, 16],
                "selected_register": 7, "tone_counter": [0, 0, 0], "env_pos": 31,
                "env_hold": true, "env_prescale": 3, "div8_acc": 5, "host_to_sample": 0
            }
        });
        let restored: Ay38910 = serde_json::from_value(old).unwrap();
        assert_eq!(restored.host_clock_hz(), HOST_CLOCK_HZ);
        let snap = restored.state_snapshot();
        assert_eq!(snap.selected_register, 7);
        assert!(snap.chip_selected, "old sessions keep working without a new latch write");
        assert_eq!(snap.registers[1], 0x02, "masked on migration");
        assert_eq!(snap.registers[6], 0x07);
        assert_eq!(snap.registers[13], 0x0D);
        assert_eq!(snap.envelope_level, 0, "envelope restarted from R13 ($0D attack)");
        assert_eq!(restored.read(0xFF41), 0x38);
    }

    #[test]
    fn slow_chip_clock_does_not_run_fast() {
        // 8 kHz master clock -> 1,000 ticks/s; TP = 1 toggles every tick.
        let ay = ay_with_clock(8_000);
        tone_a(&ay, 1);
        let before = ay.state.borrow().tone_output[0];
        let _ = ay.take_samples(AUDIO_SAMPLE_RATE as usize);
        let mut state = ay.state.borrow_mut();
        // After exactly 1,000 ticks (an even number of toggles) the output
        // is back where it started, and the next tick is a full tick away.
        assert_eq!(state.tone_output[0], before);
        assert_eq!(state.tick_remaining, TICK_UNITS);
        tick_generators(&mut state);
        assert_ne!(state.tone_output[0], before);
    }

    #[test]
    fn take_and_fill_do_not_desync_the_timeline() {
        let ay = enabled_ay();
        let _ = ay.take_samples(1_000);
        ay.fill_audio_to(500);
        assert_eq!(ay.drain_audio().len(), 500);
        // One host second: 1,500 samples were already rendered ahead.
        ay.tick(HOST_CLOCK_HZ);
        assert_eq!(ay.drain_audio().len(), AUDIO_SAMPLE_RATE as usize - 1_500);
        ay.tick(HOST_CLOCK_HZ);
        assert_eq!(ay.drain_audio().len(), AUDIO_SAMPLE_RATE as usize);
    }

    #[test]
    fn fill_audio_to_is_capped() {
        let ay = enabled_ay();
        ay.fill_audio_to(AUDIO_BUFFER_CAP * 3);
        assert_eq!(ay.drain_audio().len(), AUDIO_BUFFER_CAP);
    }

    #[test]
    fn full_buffer_drops_oldest_samples() {
        let ay = enabled_ay();
        ay.tick(2 * HOST_CLOCK_HZ); // 2 s of silence fills the queue
        tone_a(&ay, 100);
        ay.tick(HOST_CLOCK_HZ / 2); // 0.5 s of tone
        let audio = ay.drain_audio();
        assert_eq!(audio.len(), AUDIO_BUFFER_CAP);
        let tail = &audio[audio.len() - 11_025..];
        let head = &audio[..11_025];
        assert!(rms(tail) > 0.05, "newest samples (tone) must be kept");
        assert!(rms(head) < 1e-6, "oldest samples are silence");
        assert!(ay.drain_audio().is_empty(), "drain returns everything");
    }

    #[test]
    fn high_tone_above_nyquist_is_filtered_not_aliased() {
        // 2 MHz clock, TP = 4 -> 31.25 kHz square wave: above Nyquist, so it
        // must not fold back as an audible 12.85 kHz tone.
        let reference = ay_with_clock(2_000_000);
        tone_a(&reference, 100); // 1.25 kHz
        let ay = ay_with_clock(2_000_000);
        tone_a(&ay, 4);
        let _ = reference.take_samples(4_410);
        let _ = ay.take_samples(4_410);
        let in_band = rms(&reference.take_samples(22_050));
        let aliased = rms(&ay.take_samples(22_050));
        assert!(
            aliased < in_band * 0.01,
            "alias {aliased} vs tone {in_band}: anti-aliasing is too weak"
        );
    }

    #[test]
    fn dac_uses_measured_curve() {
        assert_eq!(DAC_LEVELS[0], 0.0, "level 0 is silent");
        assert_eq!(DAC_LEVELS[15], 1.0);
        assert!(DAC_LEVELS.windows(2).all(|w| w[0] < w[1]));
        // Measured AY curve, not the ideal -3 dB/step (0.178 at level 10).
        assert!((DAC_LEVELS[10] - 0.292).abs() < 0.01);
        assert!((DAC_LEVELS[7] - 0.107).abs() < 0.01);

        let ay = enabled_ay();
        set(&ay, 7, 0x3F); // gates open: output = volume
        set(&ay, 8, 0x00);
        assert_eq!(ay.state.borrow().level, 0.0);
        set(&ay, 8, 0x0A);
        let level = ay.state.borrow().level;
        assert!((level - 0.292_210 * OUTPUT_GAIN).abs() < 1e-6);
    }

    #[test]
    fn envelope_mode_uses_envelope_level() {
        let ay = enabled_ay();
        set(&ay, 7, 0x3F);
        set(&ay, 9, 0x10); // channel B from the envelope
        set(&ay, 13, 0x0D); // attack: starts at level 0
        assert_eq!(ay.state.borrow().level, 0.0);
        set(&ay, 13, 0x00); // decay: starts at level 15
        assert!((ay.state.borrow().level - OUTPUT_GAIN).abs() < 1e-6);
    }

    #[test]
    fn reset_clears_registers_and_deselects() {
        let mut ay = enabled_ay();
        ay.set_port_input('a', 0x42);
        for reg in 0..16u8 {
            set(&ay, reg, 0xFF);
        }
        ay.reset();
        let snap = ay.state_snapshot();
        assert_eq!(snap.registers, [0; 16]);
        assert!(!snap.chip_selected);
        assert_eq!(snap.port_a_in, 0x42, "external pins are not reset");
        assert_eq!(ay.read(DATA), 0xFF);
        assert_eq!(get(&ay, 14), 0x42, "port A is an input after reset");
    }

    #[test]
    fn r13_envelope_shape_retriggers_envelope() {
        let ay = enabled_ay();
        set(&ay, 11, 0x01);
        set(&ay, 12, 0x00);
        set(&ay, 13, 0x09);
        set(&ay, 8, 0x10);
        ay.tick(2000);
        assert!(ay.state.borrow().env_holding, "decay finished and holds");
        set(&ay, 13, 0x00);
        let state = ay.state.borrow();
        assert!(!state.env_holding);
        assert_eq!(state.env_count, 0);
        assert_eq!(envelope_level(&state), 15);
    }

    #[test]
    fn disabled_chip_produces_no_audio() {
        let ay = Ay38910::new(AyConfig {
            enabled: false,
            base_addr: BASE,
            chip_clock_hz: DEFAULT_CHIP_CLOCK_HZ,
        });
        ay.tick(100_000);
        assert!(ay.drain_audio().is_empty());
        assert!(ay.take_samples(10).is_empty());
    }

    #[test]
    fn io_registers_show_ports_at_bus_addresses_and_all_registers() {
        let ay = enabled_ay();
        set(&ay, 5, 0x0C);
        set(&ay, 7, 0x38);
        let regs = ay.io_registers();
        assert_eq!(regs.len(), 18);
        assert_eq!(regs[0].address, BASE);
        assert_eq!(regs[0].value, 7, "latch holds the last register number");
        assert_eq!(regs[1].address, DATA);
        assert_eq!(regs[1].value, 0x38, "data port reads the selected register");
        assert_eq!(regs[2].name, "AY R0  ToneA Fine");
        assert_eq!(regs[2 + 5].value, 0x0C);
        assert_eq!(regs[2 + 7].name, "AY R7  Mixer/IO Dir");
        assert_eq!(regs[2 + 15].name, "AY R15 PortB Data");
        assert!(regs[2..].iter().all(|r| r.address == DATA));
    }

    #[test]
    fn io_registers_empty_when_disabled() {
        let ay = Ay38910::new(AyConfig::default());
        assert!(ay.io_registers().is_empty());
    }

    #[test]
    fn poll_irq_always_false() {
        let ay = enabled_ay();
        ay.tick(1000);
        assert!(!ay.poll_irq());
    }

    #[test]
    fn output_is_bipolar() {
        let ay = enabled_ay();
        tone_a(&ay, 50);
        ay.fill_audio_to(2000);
        let audio = ay.drain_audio();
        let has_pos = audio.iter().any(|&s| s > 0.01);
        let has_neg = audio.iter().any(|&s| s < -0.01);
        assert!(has_pos && has_neg, "output should swing both sides of zero");
    }

    #[test]
    fn fir_has_unity_dc_gain_and_strong_stopband() {
        let h = fir_coeffs();
        let dc: f32 = h.iter().sum();
        assert!((dc - 1.0).abs() < 1e-5);
        let gain = |f: f64| {
            let (mut re, mut im) = (0.0_f64, 0.0_f64);
            for (i, &c) in h.iter().enumerate() {
                let w = 2.0 * std::f64::consts::PI * f / BIN_RATE as f64 * i as f64;
                re += f64::from(c) * w.cos();
                im -= f64::from(c) * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        assert!(gain(1_000.0) > 0.99);
        assert!(gain(10_000.0) > 0.98);
        for f in [30_000.0, 44_100.0, 88_000.0, 150_000.0, 170_000.0] {
            assert!(gain(f) < 1e-3, "stopband leak at {f} Hz: {}", gain(f));
        }
    }
}
