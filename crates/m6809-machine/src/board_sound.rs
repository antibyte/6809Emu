//! CoCo 2 / Dragon 32 board sound: the 6-bit DAC (PIA1 PA2-PA7) and the
//! cassette input routed through the analog MUX (PIA0 CA2 = SEL1 / LSB,
//! PIA0 CB2 = SEL2 / MSB) and enabled by PIA1 CB2, plus the single-bit sound
//! output on PIA1 PB1.
//!
//! The analog level is integrated over emulated E cycles and box-filtered
//! into mono samples at [`AUDIO_SAMPLE_RATE`]. Time is kept in integer
//! sub-cycle units (1 E cycle = `AUDIO_SAMPLE_RATE` units, 1 sample =
//! `clock_hz` units), so the sample count per emulated second is exact and
//! never drifts. Output is DC-blocked; the buffer keeps at most
//! [`AUDIO_BUFFER_CAP`] samples and drops the oldest ones.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::ay38910::AUDIO_SAMPLE_RATE;

/// About two seconds of audio; older samples are dropped first.
pub(crate) const AUDIO_BUFFER_CAP: usize = 2 * AUDIO_SAMPLE_RATE as usize;

/// Analog MUX sources (SEL2:SEL1).
pub(crate) const MUX_DAC: u8 = 0;
pub(crate) const MUX_TAPE: u8 = 1;
/// MUX source 2 (cartridge SND) has no emulated source; 3 is grounded.
const MUX_CART: u8 = 2;

/// Full-scale voltage of the audio output stage.
const MAX_V: f32 = 4.70;

/// Audio output voltage = `raw * gain + offset`, per MUX source, for the
/// single-bit sound pin low (index 0) or high (index 1). The single-bit pin
/// loads the MUX output through a resistor, so it changes gain and offset.
/// Values are the voltages measured on a real Dragon, as documented by
/// XRoar's sound model (the CoCo 1/2 audio circuit is the same design).
const MUX_GAIN_V: [[f32; 2]; 4] = [
    [2.84, 3.40], // DAC
    [0.40, 0.50], // cassette
    [2.84, 3.40], // cartridge SND
    [0.00, 0.00], // grounded
];
const MUX_OFFSET_V: [[f32; 2]; 4] = [
    [0.18, 1.30],
    [1.60, 2.35],
    [0.18, 1.30],
    [0.00, 0.00],
];
/// MUX inhibited (sound disabled): only the single-bit output drives the line.
const SBS_ONLY_V: [f32; 2] = [0.00, 3.90];

/// Overall output gain (-3 dBFS headroom).
const OUTPUT_GAIN: f32 = 0.7;

/// DC blocker pole (~21 Hz corner at 44.1 kHz).
const DC_POLE: f32 = 0.997;

/// Snapshot of the analog inputs that shape the audio output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AnalogInputs {
    /// 6-bit DAC value (PIA1 PA2-PA7).
    pub dac: u8,
    /// Cassette input level 0.0..=1.0 (0.5 = silence).
    pub tape: f32,
    /// PIA1 PB1 single-bit sound pin.
    pub sbs_high: bool,
    /// MUX select (0 = DAC, 1 = cassette, 2 = cartridge, 3 = none).
    pub mux: u8,
    /// PIA1 CB2: MUX output enabled to the audio amplifier.
    pub sound_enabled: bool,
}

/// Normalised output level (before DC blocking) for the given inputs.
pub(crate) fn output_level(inputs: &AnalogInputs) -> f32 {
    let sbs = usize::from(inputs.sbs_high);
    let volts = if inputs.sound_enabled {
        let source = usize::from(inputs.mux & 3);
        let raw = match inputs.mux & 3 {
            MUX_DAC => f32::from(inputs.dac & 0x3F) / 63.0,
            MUX_TAPE => inputs.tape.clamp(0.0, 1.0),
            // No cartridge sound source is emulated; source 3 is grounded.
            MUX_CART | 3.. => 0.0,
        };
        raw * MUX_GAIN_V[source][sbs] + MUX_OFFSET_V[source][sbs]
    } else {
        SBS_ONLY_V[sbs]
    };
    volts / MAX_V * OUTPUT_GAIN
}

/// Cycle-locked resampler + DC blocker + bounded sample queue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BoardSound {
    /// Level currently driven (held until the next `set_level`).
    #[serde(default)]
    level: f32,
    /// Progress into the current output sample, in sub-cycle units.
    #[serde(default)]
    phase: u64,
    /// Clock the `phase` is expressed in (0 = none yet).
    #[serde(default)]
    clock: u32,
    /// Integral of the level over `phase`.
    #[serde(default)]
    area: f64,
    #[serde(default)]
    dc_x: f32,
    #[serde(default)]
    dc_y: f32,
    #[serde(skip)]
    buffer: VecDeque<f32>,
}

impl BoardSound {
    /// Integrate the current level over `cycles` E cycles at `clock_hz`,
    /// emitting every completed output sample.
    pub fn advance(&mut self, cycles: u32, clock_hz: u32) {
        if cycles == 0 || clock_hz == 0 {
            return;
        }
        if self.clock != clock_hz {
            if self.clock != 0 && self.phase != 0 {
                // Keep the elapsed fraction of the pending sample.
                let scale = f64::from(clock_hz) / f64::from(self.clock);
                self.phase = ((self.phase as f64) * scale) as u64;
                self.area *= scale;
                self.phase = self.phase.min(u64::from(clock_hz) - 1);
            }
            self.clock = clock_hz;
        }
        let per_sample = u64::from(clock_hz);
        let mut remaining = u64::from(cycles) * u64::from(AUDIO_SAMPLE_RATE);
        let level = f64::from(self.level);
        while remaining > 0 {
            let take = remaining.min(per_sample - self.phase);
            self.area += level * take as f64;
            self.phase += take;
            remaining -= take;
            if self.phase == per_sample {
                let sample = (self.area / per_sample as f64) as f32;
                self.phase = 0;
                self.area = 0.0;
                self.push(sample);
            }
        }
    }

    pub fn set_level(&mut self, level: f32) {
        self.level = level;
    }

    #[cfg(test)]
    pub fn level(&self) -> f32 {
        self.level
    }

    fn push(&mut self, raw: f32) {
        let y = raw - self.dc_x + DC_POLE * self.dc_y;
        self.dc_x = raw;
        self.dc_y = y;
        if self.buffer.len() >= AUDIO_BUFFER_CAP {
            self.buffer.pop_front();
        }
        self.buffer.push_back(y);
    }

    /// Take all buffered samples.
    pub fn drain(&mut self) -> Vec<f32> {
        self.buffer.drain(..).collect()
    }

    #[cfg(test)]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOCK: u32 = 894_886;

    fn dac_inputs(dac: u8) -> AnalogInputs {
        AnalogInputs {
            dac,
            tape: 0.5,
            sbs_high: true,
            mux: MUX_DAC,
            sound_enabled: true,
        }
    }

    /// Dominant frequency by counting sign changes of the DC-blocked signal.
    fn zero_crossing_hz(samples: &[f32]) -> f64 {
        let mut crossings = 0usize;
        for pair in samples.windows(2) {
            if (pair[0] < 0.0) != (pair[1] < 0.0) {
                crossings += 1;
            }
        }
        crossings as f64 / 2.0 / (samples.len() as f64 / f64::from(AUDIO_SAMPLE_RATE))
    }

    /// Goertzel power of `freq` in `samples`.
    fn goertzel(samples: &[f32], freq: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / f64::from(AUDIO_SAMPLE_RATE);
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in samples {
            let s0 = f64::from(x) + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - coeff * s1 * s2
    }

    #[test]
    fn one_emulated_second_yields_exactly_the_sample_rate() {
        let mut snd = BoardSound::default();
        let mut left = CLOCK;
        while left > 0 {
            let step = left.min(7);
            snd.advance(step, CLOCK);
            left -= step;
        }
        assert_eq!(snd.buffered(), AUDIO_SAMPLE_RATE as usize);
        // Dragon clock: still one second of audio per clock_hz cycles.
        let mut snd = BoardSound::default();
        for _ in 0..888_625 / 5 {
            snd.advance(5, 888_625);
        }
        assert_eq!(snd.buffered(), AUDIO_SAMPLE_RATE as usize);
    }

    #[test]
    fn sample_stream_does_not_depend_on_step_granularity() {
        let run = |steps: &[u32]| {
            let mut snd = BoardSound::default();
            let mut t = 0u64;
            let mut idx = 0;
            while t < 200_000 {
                let n = steps[idx % steps.len()];
                idx += 1;
                // Square wave with a 500-cycle half period, sampled at step ends.
                let level = if (t / 500) % 2 == 0 { 0.2 } else { 0.8 };
                snd.set_level(level);
                snd.advance(n, CLOCK);
                t += u64::from(n);
            }
            snd.drain()
        };
        // Step sizes whose partial sums hit every 500-cycle toggle exactly.
        let a = run(&[1]);
        let b = run(&[5]);
        let c = run(&[2, 3]);
        assert_eq!(a.len(), (200_000u64 * 44_100 / u64::from(CLOCK)) as usize);
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), c.len());
        for i in 0..a.len() {
            assert!((a[i] - b[i]).abs() < 1e-5, "sample {i}: {} vs {}", a[i], b[i]);
            assert!((a[i] - c[i]).abs() < 1e-5, "sample {i}: {} vs {}", a[i], c[i]);
        }
    }

    #[test]
    fn dac_square_wave_has_expected_dominant_frequency() {
        let mut snd = BoardSound::default();
        // 1000 Hz square wave: toggle every clock/2000 cycles (fractional).
        let half = f64::from(CLOCK) / 2000.0;
        let mut t = 0u64;
        let mut next_toggle = half;
        let mut high = false;
        while t < u64::from(CLOCK) {
            if t as f64 >= next_toggle {
                high = !high;
                next_toggle += half;
            }
            snd.set_level(output_level(&dac_inputs(if high { 63 } else { 0 })));
            snd.advance(4, CLOCK);
            t += 4;
        }
        let samples = snd.drain();
        assert_eq!(samples.len(), 44_100);
        let tail = &samples[4_410..]; // skip the DC blocker settling
        let hz = zero_crossing_hz(tail);
        assert!((hz - 1000.0).abs() < 5.0, "zero-crossing frequency {hz}");
        let p1000 = goertzel(tail, 1000.0);
        for other in [500.0, 700.0, 1500.0, 2000.0] {
            assert!(p1000 > 20.0 * goertzel(tail, other), "1 kHz must dominate {other} Hz");
        }
        let peak = tail.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak > 0.15 && peak <= 1.0, "sensible amplitude, peak={peak}");
    }

    #[test]
    fn buffer_keeps_the_newest_two_seconds() {
        let mut snd = BoardSound::default();
        snd.set_level(0.0);
        for _ in 0..3 {
            snd.advance(CLOCK, CLOCK);
        }
        assert_eq!(snd.buffered(), AUDIO_BUFFER_CAP);
        // A step at the very end must still be in the (full) buffer.
        snd.set_level(0.5);
        snd.advance(CLOCK / 100, CLOCK);
        let samples = snd.drain();
        assert_eq!(samples.len(), AUDIO_BUFFER_CAP);
        assert!(samples[samples.len() - 400] > 0.3, "newest samples retained");
        assert!(snd.drain().is_empty());
    }

    #[test]
    fn clock_change_keeps_real_time_rate() {
        let mut snd = BoardSound::default();
        snd.advance(CLOCK / 2, CLOCK);
        // Double speed: twice the cycles per second of wall time.
        snd.advance(CLOCK, CLOCK * 2);
        let n = snd.drain().len() as i64;
        assert!((n - 44_100).abs() <= 1, "got {n}");
    }

    #[test]
    fn mux_and_enable_gate_the_dac() {
        let base = dac_inputs(0);
        let hi = AnalogInputs { dac: 63, ..base };
        assert!(output_level(&hi) > output_level(&base) + 0.4);
        // Sound disabled: the DAC is not heard.
        let off_lo = AnalogInputs { sound_enabled: false, ..base };
        let off_hi = AnalogInputs { sound_enabled: false, ..hi };
        assert_eq!(output_level(&off_lo), output_level(&off_hi));
        // MUX on the cassette input: the DAC is not heard either.
        let tape_lo = AnalogInputs { mux: MUX_TAPE, ..base };
        let tape_hi = AnalogInputs { mux: MUX_TAPE, ..hi };
        assert_eq!(output_level(&tape_lo), output_level(&tape_hi));
        // ... but the tape is (quietly).
        let tape_on = AnalogInputs { mux: MUX_TAPE, tape: 1.0, ..base };
        let tape_off = AnalogInputs { mux: MUX_TAPE, tape: 0.0, ..base };
        assert!(output_level(&tape_on) > output_level(&tape_off));
        // Single-bit sound is audible with the MUX disabled.
        let sbs_lo = AnalogInputs { sound_enabled: false, sbs_high: false, ..base };
        let sbs_hi = AnalogInputs { sound_enabled: false, sbs_high: true, ..base };
        assert!(output_level(&sbs_hi) - output_level(&sbs_lo) > 0.4);
        // Cartridge / grounded sources are silent (constant).
        let cart = AnalogInputs { mux: MUX_CART, ..hi };
        assert_eq!(output_level(&cart), output_level(&AnalogInputs { mux: MUX_CART, ..base }));
    }

    #[test]
    fn serde_round_trip_skips_the_buffer() {
        let mut snd = BoardSound::default();
        snd.set_level(0.3);
        snd.advance(1000, CLOCK);
        let json = serde_json::to_value(&snd).unwrap();
        let back: BoardSound = serde_json::from_value(json).unwrap();
        assert_eq!(back.buffered(), 0);
        assert_eq!(back.level(), 0.3);
        let empty: BoardSound = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.level(), 0.0);
    }
}
