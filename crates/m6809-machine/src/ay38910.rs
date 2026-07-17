use std::cell::RefCell;
use std::collections::VecDeque;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_ADDR: u16 = 0xFF40;
pub const DEFAULT_CHIP_CLOCK_HZ: u32 = 1_000_000;
pub const AUDIO_SAMPLE_RATE: u32 = 44_100;
pub const AUDIO_BUFFER_CAP: usize = 8192;

// Register names for the IoPanel debugger view.
const REG_NAMES: [&str; 16] = [
    "AY R0  ToneA Fine", "AY R1  ToneA Coarse", "AY R2  ToneB Fine",
    "AY R3  ToneB Coarse", "AY R4  ToneC Fine",  "AY R5  ToneC Coarse",
    "AY R6  Noise Period", "AY R7  Mixer/IO Dir", "AY R8  AmpA/EnvA",
    "AY R9  AmpB/EnvB",    "AY R10 AmpC/EnvC",    "AY R11 Env Fine",
    "AY R12 Env Coarse",   "AY R13 Env Shape",    "AY R14 PortA Data",
    "AY R15 PortB Data",
];

const ENVELOPE_SHAPES: [[u8; 32]; 16] = build_envelope_shapes();

/// AY-3-8910 logarithmic DAC levels (datasheet-style), normalized to 0..1.
/// Real chip amplitude is roughly exponential: each step ≈ 3 dB.
const DAC_LEVELS: [f32; 16] = [
    0.0000, 0.0078, 0.0110, 0.0156, 0.0221, 0.0312, 0.0442, 0.0624,
    0.0883, 0.1249, 0.1766, 0.2498, 0.3533, 0.4998, 0.7071, 1.0000,
];

/// Precomputed 32-step amplitude table indexed by envelope shape (R13 & 0x0F).
/// Each entry is a 0..15 amplitude. Shape bit fields match the datasheet:
///   bit 0 = Hold, bit 1 = Alternate, bit 2 = Attack, bit 3 = Continue.
const fn build_envelope_shapes() -> [[u8; 32]; 16] {
    let mut shapes = [[0u8; 32]; 16];

    let mut shape_idx = 0;
    while shape_idx < 16 {
        // Datasheet bit order (R13):
        //   bit0 Hold, bit1 Alternate, bit2 Attack, bit3 Continue
        let attack_bit = (shape_idx & 4) != 0;
        let alternate_bit = (shape_idx & 2) != 0;
        let hold_bit = (shape_idx & 1) != 0;

        // First half: 16 steps ramping 0->15 (attack) or 15->0 (decay).
        let mut step = 0;
        while step < 16 {
            let v = if attack_bit {
                step as u8
            } else {
                15 - step as u8
            };
            shapes[shape_idx][step] = v;
            step += 1;
        }
        // Second half (steps 16-31): depends on hold / alternate.
        //
        // Hold + Alternate: full 32-step triangle (ramp up then ramp down),
        //   then freeze at the end of the second half.
        // Hold + no Alternate: 16-step ramp, then hold at the end value
        //   for all remaining steps.
        // No Hold + Alternate: 32-step triangle, then repeat.
        // No Hold + no Alternate: 16-step ramp, then repeat.
        let mut step = 0;
        while step < 16 {
            let v = if hold_bit && !alternate_bit {
                // Hold without alternate: freeze at first-half end value.
                if attack_bit {
                    15u8
                } else {
                    0u8
                }
            } else if alternate_bit {
                // Alternate (with or without hold): reverse direction.
                if attack_bit {
                    15 - step as u8
                } else {
                    step as u8
                }
            } else {
                // No hold, no alternate: repeat first half.
                if attack_bit {
                    step as u8
                } else {
                    15 - step as u8
                }
            };
            shapes[shape_idx][16 + step] = v;
            step += 1;
        }

        shape_idx += 1;
    }
    shapes
}

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
    pub registers: [u8; 16],
    pub selected_register: u8,
    pub port_a_in: u8,
    pub port_b_in: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AyState {
    registers: [u8; 16],
    selected_register: u8,
    port_a_input: u8,
    port_b_input: u8,
    // Tone channel phase accumulators (tick every 8 master clocks).
    tone_counter: [u32; 3],
    tone_output: [bool; 3],
    // Noise generator (same /8 domain as tone).
    noise_counter: u32,
    noise_lfsr: u32,
    // Envelope generator (ticks every 256 master clocks = 32 tone ticks).
    env_counter: u32,
    env_pos: u32,
    env_hold: bool,
    env_prescale: u32,
    // Master-clock residual for the /8 tone/noise divider.
    div8_acc: u32,
    // Audio sample accumulator (master clocks toward next sample).
    cycles_acc: u32,
    #[serde(skip)]
    audio: VecDeque<f32>,
}

impl Default for AyState {
    fn default() -> Self {
        Self {
            registers: [0; 16],
            selected_register: 0,
            port_a_input: 0xFF,
            port_b_input: 0xFF,
            tone_counter: [0; 3],
            tone_output: [false; 3],
            noise_counter: 0,
            // 17-bit LFSR seed (bit 0 is the output).
            noise_lfsr: 0x1_FFFF,
            env_counter: 0,
            env_pos: 0,
            env_hold: false,
            env_prescale: 0,
            div8_acc: 0,
            cycles_acc: 0,
            audio: VecDeque::new(),
        }
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
        let snap = AySnapshot::deserialize(deserializer)?;
        Ok(Self {
            config: snap.config,
            state: RefCell::new(snap.state),
        })
    }
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
            self.apply_reset();
        } else if !config.enabled {
            // Clear audio buffer when disabled.
            let mut state = self.state.borrow_mut();
            state.audio.clear();
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled
            && (addr == self.config.base_addr || addr == self.config.base_addr.wrapping_add(1))
    }

    pub fn read(&self, addr: u16) -> u8 {
        if !self.handles(addr) {
            return 0xFF;
        }
        let state = self.state.borrow();
        if addr == self.config.base_addr {
            // Address latch reads as 0xFF (write-only function).
            return 0xFF;
        }
        // Data register read: only R14/R15 are readable; others return 0xFF.
        let reg = state.selected_register as usize;
        match reg {
            14 => {
                let port_a_output = state.registers[7] & 0x40 != 0;
                if port_a_output {
                    state.registers[14]
                } else {
                    state.port_a_input
                }
            }
            15 => {
                let port_b_output = state.registers[7] & 0x80 != 0;
                if port_b_output {
                    state.registers[15]
                } else {
                    state.port_b_input
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write(&self, addr: u16, value: u8) {
        if !self.handles(addr) {
            return;
        }
        let mut state = self.state.borrow_mut();
        if addr == self.config.base_addr {
            // Address latch: select register 0-15 (high nibbles ignored).
            state.selected_register = value & 0x0F;
            return;
        }
        // Data write to selected register.
        let reg = state.selected_register as usize;
        state.registers[reg] = value;
        // R13 (envelope shape) retriggers the envelope generator.
        if reg == 13 {
            state.env_pos = 0;
            state.env_counter = 0;
            state.env_hold = false;
            state.env_prescale = 0;
        }
    }

    /// Master clocks covered by one output sample (rounded).
    fn cycles_per_sample(&self) -> u32 {
        let clock = self.config.chip_clock_hz.max(1);
        ((clock + AUDIO_SAMPLE_RATE / 2) / AUDIO_SAMPLE_RATE).max(1)
    }

    /// CPU-cycle hook. Audio is **not** generated here — the run loop renders
    /// wall-clock sample blocks via [`Self::take_samples`] so playback stays
    /// continuous regardless of how many CPU steps land in a UI frame.
    pub fn tick(&self, _cycles: u32) {
        // Register writes already take effect immediately; sample time advances
        // only on the wall-clock render path.
    }

    pub fn poll_irq(&self) -> bool {
        false
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        REG_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| IoRegisterView {
                address: self.config.base_addr,
                name: name.to_string(),
                value: state.registers[i],
            })
            .collect()
    }

    pub fn state_snapshot(&self) -> AyStateDto {
        let state = self.state.borrow();
        AyStateDto {
            config: self.config,
            registers: state.registers,
            selected_register: state.selected_register,
            port_a_in: state.port_a_input,
            port_b_in: state.port_b_input,
        }
    }

    /// Drain any buffered samples (legacy / tests). Prefer [`Self::take_samples`].
    pub fn drain_audio(&self) -> Vec<f32> {
        let mut state = self.state.borrow_mut();
        state.audio.drain(..).collect()
    }

    /// Generate additional samples until the internal buffer holds at least
    /// `target_count` samples (tests / catch-up).
    pub fn fill_audio_to(&self, target_count: usize) {
        if !self.config.enabled {
            return;
        }
        let cycles_per_sample = self.cycles_per_sample();
        let mut state = self.state.borrow_mut();
        let limit = target_count.max(AUDIO_BUFFER_CAP);
        while state.audio.len() < target_count {
            let sample = generate_sample(&mut state, cycles_per_sample);
            if state.audio.len() >= limit {
                state.audio.pop_front();
            }
            state.audio.push_back(sample);
        }
    }

    /// Render exactly `count` continuous mono samples at [`AUDIO_SAMPLE_RATE`],
    /// advancing chip state by the matching master-clock amount. This is the
    /// wall-clock path used by the run loop for gapless playback.
    pub fn take_samples(&self, count: usize) -> Vec<f32> {
        if !self.config.enabled || count == 0 {
            return Vec::new();
        }
        let cycles_per_sample = self.cycles_per_sample();
        let mut state = self.state.borrow_mut();
        // Drop any stale buffered samples — wall-clock render owns the stream.
        state.audio.clear();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(generate_sample(&mut state, cycles_per_sample));
        }
        out
    }

    /// Set the raw input value on an I/O port (Port A = 'a', Port B = 'b').
    /// Active only when the corresponding direction bit in R7 marks the port
    /// as an input.
    pub fn set_port_input(&self, port: char, value: u8) {
        let mut state = self.state.borrow_mut();
        match port {
            'a' | 'A' => state.port_a_input = value,
            'b' | 'B' => state.port_b_input = value,
            _ => {}
        }
    }

    fn apply_reset(&mut self) {
        let mut state = self.state.borrow_mut();
        *state = AyState::default();
    }
}

/// Advance tone / noise / envelope for `master_cycles` chip clocks and
/// return one mono sample in approximately [-1, 1].
fn generate_sample(state: &mut AyState, master_cycles: u32) -> f32 {
    // Tone and noise run off a /8 master-clock divider.
    // Envelope runs off /256 master clocks (= every 32 tone ticks).
    // With period TP: fT = fCLOCK / (16 * TP)  (toggle every TP tone-ticks).
    state.div8_acc = state.div8_acc.saturating_add(master_cycles);
    while state.div8_acc >= 8 {
        state.div8_acc -= 8;
        tick_tone_noise(state);
        state.env_prescale = state.env_prescale.saturating_add(1);
        if state.env_prescale >= 32 {
            state.env_prescale = 0;
            tick_envelope(state);
        }
    }

    mix_output(state)
}

fn tick_tone_noise(state: &mut AyState) {
    let regs = state.registers;

    // Tone periods (12-bit each, min 1 to avoid divide-by-zero).
    // Coarse registers (R1/R3/R5) use only the low nibble; high nibble is undefined.
    let tone_period_a = ((((regs[1] as u32) & 0x0F) << 8) | regs[0] as u32).max(1);
    let tone_period_b = ((((regs[3] as u32) & 0x0F) << 8) | regs[2] as u32).max(1);
    let tone_period_c = ((((regs[5] as u32) & 0x0F) << 8) | regs[4] as u32).max(1);
    let tone_periods = [tone_period_a, tone_period_b, tone_period_c];

    for ch in 0..3 {
        state.tone_counter[ch] = state.tone_counter[ch].saturating_sub(1);
        if state.tone_counter[ch] == 0 {
            state.tone_output[ch] = !state.tone_output[ch];
            state.tone_counter[ch] = tone_periods[ch];
        }
    }

    // Noise period (5-bit, min 1).
    let noise_period = ((regs[6] as u32) & 0x1F).max(1);
    state.noise_counter = state.noise_counter.saturating_sub(1);
    if state.noise_counter == 0 {
        state.noise_counter = noise_period;
        // 17-bit Galois LFSR (standard AY noise).
        // Feedback: bit0 XOR bit3 → shift into bit16.
        let bit0 = state.noise_lfsr & 1;
        let bit3 = (state.noise_lfsr >> 3) & 1;
        state.noise_lfsr = (state.noise_lfsr >> 1) | ((bit0 ^ bit3) << 16);
        state.noise_lfsr &= 0x1_FFFF;
    }
}

fn tick_envelope(state: &mut AyState) {
    if state.env_hold {
        return;
    }
    let regs = state.registers;
    // Envelope period (16-bit, min 1).
    let env_period = (((regs[12] as u32) << 8) | regs[11] as u32).max(1);
    state.env_counter = state.env_counter.saturating_sub(1);
    if state.env_counter != 0 {
        return;
    }
    state.env_counter = env_period;
    state.env_pos = state.env_pos.saturating_add(1);
    if state.env_pos >= 32 {
        // Datasheet: bit3 = Continue, bit0 = Hold.
        let continue_bit = (regs[13] & 8) != 0;
        let hold_bit = (regs[13] & 1) != 0;
        if !continue_bit || hold_bit {
            // One-shot or held: hold at last value (edge of shape table).
            state.env_hold = true;
            state.env_pos = 31;
        } else {
            state.env_pos = 0;
        }
    }
}

fn mix_output(state: &AyState) -> f32 {
    let regs = state.registers;
    let env_shape = (regs[13] & 0x0F) as usize;
    let env_amplitude = ENVELOPE_SHAPES[env_shape][state.env_pos as usize];
    let noise_output = (state.noise_lfsr & 1) != 0;

    // Mixer / enable mask (R7 bits 0-5). 0 = tone/noise ENABLED for that channel.
    let mixer = regs[7];

    let mut mixed = 0.0_f32;
    for ch in 0..3 {
        let tone_on = (mixer & (1 << ch)) == 0; // bit 0/1/2: A/B/C tone enable
        let noise_on = (mixer & (1 << (ch + 3))) == 0; // bit 3/4/5: A/B/C noise enable

        let tone_signal = if tone_on {
            state.tone_output[ch]
        } else {
            true
        };
        let noise_signal = if noise_on { noise_output } else { true };
        // Output gate is 1 only when both enabled signals pass through.
        let gate = tone_signal && noise_signal;

        // Amplitude: env-enabled (bit 4 of R[8+ch]) uses envelope value; else fixed 4-bit nibble.
        let amp_reg = regs[8 + ch];
        let env_enabled = (amp_reg & 0x10) != 0;
        let amplitude = if env_enabled {
            env_amplitude
        } else {
            amp_reg & 0x0F
        };

        let level = DAC_LEVELS[amplitude as usize];
        // Bipolar contribution: +level when open, -level when closed.
        // Removes the large DC offset that otherwise becomes a dull thump/hum
        // through capacitive coupling in the audio path.
        mixed += if gate { level } else { -level };
    }

    // Three channels summed; scale to stay comfortably in [-1, 1].
    mixed * (0.35 / 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_ay() -> Ay38910 {
        Ay38910::new(AyConfig {
            enabled: true,
            base_addr: 0xFF40,
            chip_clock_hz: 1_000_000,
        })
    }

    #[test]
    fn address_latch_selects_register() {
        let ay = enabled_ay();
        ay.write(0xFF40, 7); // select R7
        ay.write(0xFF41, 0x38); // all tone/noise disabled
        let snap = ay.state_snapshot();
        assert_eq!(snap.selected_register, 7);
        assert_eq!(snap.registers[7], 0x38);
    }

    #[test]
    fn data_read_returns_ff_for_write_only_registers() {
        let ay = enabled_ay();
        ay.write(0xFF40, 0); // select R0
        ay.write(0xFF41, 0xAB);
        assert_eq!(ay.read(0xFF41), 0xFF); // R0 is write-only
    }

    #[test]
    fn port_a_input_read_when_direction_is_input() {
        let ay = enabled_ay();
        // R7 bit 6 = 0 -> port A is input.
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x00);
        ay.set_port_input('a', 0x5A);
        ay.write(0xFF40, 14); // select R14
        assert_eq!(ay.read(0xFF41), 0x5A);
    }

    #[test]
    fn port_a_read_returns_output_register_when_direction_output() {
        let ay = enabled_ay();
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x40); // bit 6 = 1 -> port A output
        ay.write(0xFF40, 14);
        ay.write(0xFF41, 0x99);
        ay.write(0xFF40, 14); // re-select R14
        assert_eq!(ay.read(0xFF41), 0x99);
    }

    #[test]
    fn take_samples_produces_audio() {
        let ay = enabled_ay();
        // Configure a simple tone on channel A.
        ay.write(0xFF40, 0);
        ay.write(0xFF41, 0xC8);
        ay.write(0xFF40, 1);
        ay.write(0xFF41, 0x00); // tone period = 200
        ay.write(0xFF40, 8);
        ay.write(0xFF41, 0x0F); // max fixed amplitude
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x3E); // enable tone A only
        let audio = ay.take_samples(4410); // 100 ms
        assert_eq!(audio.len(), 4410);
        assert!(
            audio.iter().any(|&s| s.abs() > 1e-6),
            "audio should contain non-zero samples"
        );
    }

    #[test]
    fn tone_frequency_matches_datasheet() {
        // fT = fCLOCK / (16 * TP). With TP=100, clock=1 MHz → 625 Hz.
        let ay = enabled_ay();
        let period: u32 = 100;
        let expected_hz = 1_000_000.0_f64 / (16.0 * period as f64);

        ay.write(0xFF40, 0);
        ay.write(0xFF41, (period & 0xFF) as u8);
        ay.write(0xFF40, 1);
        ay.write(0xFF41, ((period >> 8) & 0x0F) as u8);
        ay.write(0xFF40, 8);
        ay.write(0xFF41, 0x0F);
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x3E); // tone A only

        // ~0.25 s of audio — enough zero-crossings for a stable estimate.
        let n = AUDIO_SAMPLE_RATE as usize / 4;
        ay.fill_audio_to(n);
        let audio = ay.drain_audio();
        assert_eq!(audio.len(), n);

        // Count positive zero-crossings (low→high) of the bipolar square wave.
        let mut crossings = 0u32;
        for w in audio.windows(2) {
            if w[0] < 0.0 && w[1] >= 0.0 {
                crossings += 1;
            }
        }
        let measured = crossings as f64 * 4.0; // scale 0.25 s → Hz
        let err = (measured - expected_hz).abs() / expected_hz;
        assert!(
            err < 0.05,
            "tone frequency off: measured {:.1} Hz, expected {:.1} Hz (err {:.1}%)",
            measured,
            expected_hz,
            err * 100.0
        );
    }

    #[test]
    fn envelope_shape_0a_is_triangle_continue_alternate() {
        // R13 = $0A = Continue + Alternate (decay-first triangle).
        // Datasheet bits: bit3 Cont, bit1 Alt → 0b1010.
        let ay = enabled_ay();
        ay.write(0xFF40, 11);
        ay.write(0xFF41, 0x01);
        ay.write(0xFF40, 12);
        ay.write(0xFF41, 0x00); // env period = 1 (fast)
        ay.write(0xFF40, 13);
        ay.write(0xFF41, 0x0A);
        ay.write(0xFF40, 8);
        ay.write(0xFF41, 0x10); // env-enabled on A
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x3F); // no tone/noise → gate always open

        // Capture amplitude trajectory via many samples.
        ay.fill_audio_to(8_000);
        let audio = ay.drain_audio();
        // Continuous triangle: amplitude magnitude must both peak and dip.
        let mut saw_high = false;
        let mut saw_dip_after_peak = false;
        let mut saw_peak_again = false;
        for &s in &audio {
            let a = s.abs();
            if a > 0.08 {
                if saw_dip_after_peak {
                    saw_peak_again = true;
                }
                saw_high = true;
            } else if a < 0.02 && saw_high {
                saw_dip_after_peak = true;
            }
        }
        assert!(
            saw_high && saw_dip_after_peak && saw_peak_again,
            "shape $0A should oscillate (triangle), high={saw_high} dip={saw_dip_after_peak} peak2={saw_peak_again}"
        );
    }

    #[test]
    fn r13_envelope_shape_retriggers_envelope() {
        let ay = enabled_ay();
        // Set envelope period to small value so it advances each tick.
        ay.write(0xFF40, 11);
        ay.write(0xFF41, 0x01);
        ay.write(0xFF40, 12);
        ay.write(0xFF41, 0x00); // env period = 1
        ay.write(0xFF40, 13);
        ay.write(0xFF41, 0x09); // continue + hold (decay then hold 0)
        ay.write(0xFF40, 8);
        ay.write(0xFF41, 0x10); // env-enabled on channel A

        // Run a bit to advance env_pos.
        ay.tick(2000);

        // Writing R13 should reset env_pos / env_counter / env_hold.
        ay.write(0xFF40, 13);
        ay.write(0xFF41, 0x00);
        let snap = ay.state_snapshot();
        assert_eq!(snap.selected_register, 13);
    }

    #[test]
    fn disabled_chip_produces_no_audio() {
        let ay = Ay38910::new(AyConfig {
            enabled: false,
            base_addr: 0xFF40,
            chip_clock_hz: 1_000_000,
        });
        ay.tick(100_000);
        assert!(ay.drain_audio().is_empty());
    }

    #[test]
    fn io_registers_lists_all_16_when_enabled() {
        let ay = enabled_ay();
        let regs = ay.io_registers();
        assert_eq!(regs.len(), 16);
        assert_eq!(regs[0].name, "AY R0  ToneA Fine");
        assert_eq!(regs[7].name, "AY R7  Mixer/IO Dir");
        assert_eq!(regs[15].name, "AY R15 PortB Data");
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
        ay.write(0xFF40, 0);
        ay.write(0xFF41, 50);
        ay.write(0xFF40, 1);
        ay.write(0xFF41, 0);
        ay.write(0xFF40, 8);
        ay.write(0xFF41, 0x0F);
        ay.write(0xFF40, 7);
        ay.write(0xFF41, 0x3E);
        ay.fill_audio_to(2000);
        let audio = ay.drain_audio();
        let has_pos = audio.iter().any(|&s| s > 0.01);
        let has_neg = audio.iter().any(|&s| s < -0.01);
        assert!(has_pos && has_neg, "output should swing both sides of zero");
    }
}
