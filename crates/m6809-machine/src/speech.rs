//! Optional SP0256-AL2 / CTS256A-AL2 speech chipset, wired like the AY-3-8910:
//! always present in the [`crate::MachineContainer`] but gated by
//! [`SpeechConfig::enabled`]. Exposes a small MMIO block, produces audio at the
//! host wall-clock sample rate (44.1 kHz) by resampling the chip's internal
//! ~10 kHz stream, and drains into the same audio path as the AY.
//!
//! MMIO (base `$FF50` by default):
//!
//! | Offset | Write                                   | Read                               |
//! |--------|-----------------------------------------|------------------------------------|
//! | `+0`   | allophone (6 bits) -> SP0256 ALD         | status (same as `+1`)              |
//! | `+1`   | -                                       | bit0 LRQ ready, bit1 SBY (standby) |
//! | `+2`   | ASCII byte -> CTS256A-AL2 parallel port | last allophone sent by the CTS     |
//! | `+3`   | bit0 = 1: reset CTS256 + SP0256         | bit0 CTS busy, bit1 input FIFO full |
//!
//! The CTS256A-AL2 runs its real mask ROM (see [`crate::cts256`]) in parallel,
//! carriage-return-only mode: text written to `+2` is spoken once a CR
//! arrives.

use std::collections::VecDeque;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

use crate::ay38910::{AUDIO_BUFFER_CAP, AUDIO_SAMPLE_RATE, HOST_CLOCK_HZ};
use crate::cts256::{Cts256, CTS_CLOCK_HZ};
use crate::sp0256::{Sp0256, ALLOPHONE_NAMES, CLOCK_DIVIDER};
use crate::speech_rom::{CTS256A, SP0256_AL2};

/// Default MMIO base address for the speech block (`$FF50..$FF53`).
pub const DEFAULT_BASE_ADDR: u16 = 0xFF50;
/// Standard SP0256 crystal: internal sample rate = `xtal / 312` ≈ 10 kHz.
pub const DEFAULT_XTAL_HZ: u32 = 3_120_000;
/// Host (6809 E) clock assumed until [`SpeechBox::set_host_clock_hz`] is called.
pub const DEFAULT_HOST_CLOCK_HZ: u32 = HOST_CLOCK_HZ;
/// Playback gain applied to the SP0256's 14-bit output (`limit << 2` /
/// 32768). The loudest allophone (IY) reaches 0.36 FS, so phrases peak at
/// about 0.7 after the reconstruction filter (the AY sits around 0.2-0.5).
/// [`soft_limit`] guards the few impulses above its knee instead of
/// hard-clipping them (the old ×8 + clamp clipped 0.6 % of all samples).
const OUTPUT_GAIN: f32 = 3.0;
/// Soft-limiter knee: linear below, smoothly compressed towards 1.0 above.
const LIMIT_KNEE: f32 = 0.8;

// Status bits reported at `base + 0` / `base + 1`.
const ST_LRQ_READY: u8 = 0x01;
const ST_STANDBY: u8 = 0x02;
// CTS status bits reported at `base + 3`.
const CTS_BUSY: u8 = 0x01;
const CTS_FIFO_FULL: u8 = 0x02;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpeechConfig {
    pub enabled: bool,
    pub base_addr: u16,
    pub xtal_hz: u32,
    /// Enable the CTS256A-AL2 text-to-allophone front end (`base + 2/3`).
    pub cts_enabled: bool,
}

impl Default for SpeechConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_addr: DEFAULT_BASE_ADDR,
            xtal_hz: DEFAULT_XTAL_HZ,
            cts_enabled: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechStateDto {
    pub config: SpeechConfig,
    pub lrq_ready: bool,
    pub standby: bool,
    pub speaking: bool,
    pub last_allophone: u8,
    pub cts_enabled: bool,
    pub cts_busy: bool,
    pub cts_last_allophone: u8,
    /// CTS ROM still initialising / greeting (input is held back).
    #[serde(default)]
    pub cts_booting: bool,
    /// Characters waiting for conversion (host FIFO + latch + ROM buffer).
    #[serde(default)]
    pub cts_input_pending: u32,
    /// Allophones waiting in the ROM's output buffer.
    #[serde(default)]
    pub cts_output_pending: u32,
    /// Host FIFO full: further writes to `base + 2` are dropped.
    #[serde(default)]
    pub cts_fifo_full: bool,
    /// BUSY* pin: the ROM's input buffer is at least 87.5 % full.
    #[serde(default)]
    pub cts_buffer_full: bool,
    /// Allophones recently sent by the CTS to the SP0256, oldest first.
    #[serde(default)]
    pub cts_recent: Vec<u8>,
    /// Delimiter mode of the ROM: carriage-return-only (true) or any delimiter.
    #[serde(default = "default_true")]
    pub cts_cr_only: bool,
    /// The CTS says "O.K." after a reset (hardware behaviour).
    #[serde(default = "default_true")]
    pub cts_greeting: bool,
}

/// Linear below [`LIMIT_KNEE`], then a tanh knee that approaches ±1.0
/// without ever reaching it (no hard clipping).
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMIT_KNEE {
        return x;
    }
    let room = 1.0 - LIMIT_KNEE;
    let y = LIMIT_KNEE + room * ((a - LIMIT_KNEE) / room).tanh();
    y.min(0.999).copysign(x)
}

#[derive(Clone)]
pub struct SpeechBox {
    config: SpeechConfig,
    /// 6809 E clock driving [`Self::tick`].
    host_clock_hz: u32,
    /// Speak "O.K." after a CTS reset.
    cts_greeting: bool,
    sp: Sp0256,
    cts: Cts256,
    last_allophone: u8,
    // Wall-clock accumulator: `+= host_cycles * SAMPLE_RATE`; emit at host clock.
    host_to_sample: u64,
    // CTS state-clock accumulator: `+= CTS_CLOCK_HZ * 312` per SP0256 sample.
    cts_clock_acc: u64,
    // Linear resampler from the chip's ~10 kHz stream to 44.1 kHz.
    resamp_phase: f64,
    s_prev: f32,
    s_next: f32,
    /// One-pole reconstruction LPF (cuts 10 kHz images that sound like hiss).
    lpf: f32,
    audio: VecDeque<f32>,
}

impl SpeechBox {
    /// Build a speech block in its power-on state: the CTS boots (and greets)
    /// as soon as the block is enabled and ticked.
    pub fn new(config: SpeechConfig) -> Self {
        let mut sb = Self {
            config,
            host_clock_hz: DEFAULT_HOST_CLOCK_HZ,
            cts_greeting: true,
            sp: Sp0256::new(SP0256_AL2),
            cts: Cts256::new(CTS256A),
            last_allophone: 0,
            host_to_sample: 0,
            cts_clock_acc: 0,
            resamp_phase: 0.0,
            s_prev: 0.0,
            s_next: 0.0,
            lpf: 0.0,
            audio: VecDeque::new(),
        };
        sb.reset();
        sb
    }

    pub fn config(&self) -> SpeechConfig {
        self.config
    }

    pub fn set_config(&mut self, config: SpeechConfig) {
        let old = self.config;
        self.config = config;
        if config.enabled && !old.enabled {
            self.reset();
        } else if !config.enabled {
            self.audio.clear();
        } else if config.cts_enabled != old.cts_enabled {
            // Insert / remove the CTS chip: boot it or hold it in reset.
            self.cts.reset(config.cts_enabled, self.cts_greeting);
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Set the 6809 E clock that [`Self::tick`] cycles are counted in (the
    /// machine calls this before ticking; e.g. 894,886 Hz CoCo 2,
    /// 1,843,200 Hz MsBasic, doubled by a SAM speed-up).
    pub fn set_host_clock_hz(&mut self, hz: u32) {
        self.host_clock_hz = hz.max(1);
    }

    pub fn host_clock_hz(&self) -> u32 {
        self.host_clock_hz
    }

    /// Whether the CTS speaks "O.K." after a reset (default: true, like the
    /// real chip). Takes effect at the next reset.
    pub fn set_cts_greeting(&mut self, on: bool) {
        self.cts_greeting = on;
    }

    pub fn cts_greeting(&self) -> bool {
        self.cts_greeting
    }

    /// Machine reset: both chips restart (the CTS boots and greets).
    pub fn reset(&mut self) {
        self.sp.reset();
        self.cts.reset(self.config.cts_enabled, self.cts_greeting);
        self.last_allophone = 0;
        self.host_to_sample = 0;
        self.cts_clock_acc = 0;
        self.resamp_phase = 0.0;
        self.s_prev = 0.0;
        self.s_next = 0.0;
        self.lpf = 0.0;
        self.audio.clear();
    }

    pub fn handles(&self, addr: u16) -> bool {
        // 32-bit so a block near $FFFF does not wrap around to $0000.
        let (addr, base) = (u32::from(addr), u32::from(self.config.base_addr));
        self.config.enabled && addr >= base && addr <= base + 3
    }

    fn status_byte(&self) -> u8 {
        let mut s = 0;
        if self.sp.lrq_ready() {
            s |= ST_LRQ_READY;
        }
        if self.sp.standby() {
            s |= ST_STANDBY;
        }
        s
    }

    fn cts_status_byte(&self) -> u8 {
        let mut s = 0;
        if self.cts.busy() {
            s |= CTS_BUSY;
        }
        if self.cts.fifo_full() {
            s |= CTS_FIFO_FULL;
        }
        s
    }

    /// Side-effect-free register view for the debugger.
    pub fn peek(&self, addr: u16) -> u8 {
        self.read(addr)
    }

    /// Register read (status registers only: reading has no side effects).
    pub fn read(&self, addr: u16) -> u8 {
        if !self.handles(addr) {
            return 0xFF;
        }
        match addr.wrapping_sub(self.config.base_addr) {
            0 | 1 => self.status_byte(),
            2 => self.cts.last_allophone(),
            3 => self.cts_status_byte(),
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if !self.handles(addr) {
            return;
        }
        match addr.wrapping_sub(self.config.base_addr) {
            0 => {
                let allophone = value & 0x3F;
                if self.sp.lrq_ready() {
                    self.sp.ald_w(allophone);
                    self.last_allophone = allophone;
                }
            }
            2 => {
                if self.config.cts_enabled {
                    self.cts.push_ascii(value);
                }
            }
            3 => {
                if value & 0x01 != 0 {
                    // Shared RESET line: restart the ROM and abort the speech.
                    self.sp.reset();
                    self.cts.reset(self.config.cts_enabled, self.cts_greeting);
                }
            }
            _ => {}
        }
    }

    /// Feed a whole string through the CTS256 (UI "Speak"), terminated by a
    /// carriage return so the ROM speaks it.
    pub fn say_text(&mut self, text: &str) {
        if !self.config.cts_enabled {
            return;
        }
        for b in text.bytes() {
            self.cts.push_ascii(b);
        }
        self.cts.push_ascii(b'\r');
    }

    #[inline]
    fn internal_rate(&self) -> f64 {
        f64::from(self.config.xtal_hz.max(CLOCK_DIVIDER)) / f64::from(CLOCK_DIVIDER)
    }

    /// Advance the CTS256 + SP0256 by one internal (~10 kHz) sample and return it.
    fn gen_internal(&mut self) -> f32 {
        // The CTS runs for one SP0256 sample period (250 cycles at 3.12 MHz),
        // feeding allophones whenever LRQ is ready; then the SP0256 renders.
        let xtal = u64::from(self.config.xtal_hz.max(CLOCK_DIVIDER));
        self.cts_clock_acc += u64::from(CTS_CLOCK_HZ) * u64::from(CLOCK_DIVIDER);
        let cycles = self.cts_clock_acc / xtal;
        self.cts_clock_acc %= xtal;
        self.cts.run(&mut self.sp, cycles as u32);
        let raw = f32::from(self.sp.next_sample()) / 32768.0;
        soft_limit(raw * OUTPUT_GAIN)
    }

    /// Render one 44.1 kHz output sample.
    fn render_output_sample(&mut self) -> f32 {
        let step = self.internal_rate() / f64::from(AUDIO_SAMPLE_RATE);
        self.resamp_phase += step;
        while self.resamp_phase >= 1.0 {
            self.resamp_phase -= 1.0;
            self.s_prev = self.s_next;
            self.s_next = self.gen_internal();
        }
        let t = self.resamp_phase as f32;
        let raw = self.s_prev + (self.s_next - self.s_prev) * t;
        // fc ≈ 4.5 kHz at 44.1 kHz: 1 - exp(-2π·4500/44100) ≈ 0.47
        self.lpf += 0.47 * (raw - self.lpf);
        self.lpf
    }

    /// Queue an output sample; when the buffer is full the oldest sample is
    /// dropped so chip time never stalls.
    fn push_audio(&mut self, sample: f32) {
        if self.audio.len() >= AUDIO_BUFFER_CAP {
            self.audio.pop_front();
        }
        self.audio.push_back(sample);
    }

    /// Advance by `host_cycles` 6809 E-clock ticks, producing 44.1 kHz audio.
    pub fn tick(&mut self, host_cycles: u32) {
        if !self.config.enabled || host_cycles == 0 {
            return;
        }
        let host = u64::from(self.host_clock_hz.max(1));
        self.host_to_sample = self
            .host_to_sample
            .saturating_add(u64::from(host_cycles) * u64::from(AUDIO_SAMPLE_RATE));
        while self.host_to_sample >= host {
            self.host_to_sample -= host;
            let s = self.render_output_sample();
            self.push_audio(s);
        }
    }

    pub fn poll_irq(&self) -> bool {
        false
    }

    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.audio.drain(..).collect()
    }

    /// Return exactly `count` samples: queued audio first (oldest), then
    /// freshly rendered samples (tests / offline catch-up). Bounded work.
    pub fn take_samples(&mut self, count: usize) -> Vec<f32> {
        if !self.config.enabled || count == 0 {
            return Vec::new();
        }
        let queued = count.min(self.audio.len());
        let mut out: Vec<f32> = self.audio.drain(..queued).collect();
        out.reserve(count - queued);
        while out.len() < count {
            out.push(self.render_output_sample());
        }
        out
    }

    fn chips_idle(&self) -> bool {
        self.sp.idle() && self.cts.idle()
    }

    /// Run the chips until they go idle or `max_samples` output samples were
    /// rendered into the audio queue (paused catch-up). The queue keeps only
    /// the newest [`AUDIO_BUFFER_CAP`] samples; use
    /// [`Self::run_until_idle_collect`] to get everything.
    pub fn run_until_idle(&mut self, max_samples: usize) {
        if !self.config.enabled {
            return;
        }
        let mut produced = 0;
        while produced < max_samples && !self.chips_idle() {
            let s = self.render_output_sample();
            self.push_audio(s);
            produced += 1;
        }
    }

    /// Like [`Self::run_until_idle`], but returns all queued audio followed
    /// by every sample rendered until idle (at most `max_samples` new ones);
    /// nothing is dropped.
    pub fn run_until_idle_collect(&mut self, max_samples: usize) -> Vec<f32> {
        if !self.config.enabled {
            return Vec::new();
        }
        let mut out: Vec<f32> = self.audio.drain(..).collect();
        let mut produced = 0;
        while produced < max_samples && !self.chips_idle() {
            out.push(self.render_output_sample());
            produced += 1;
        }
        out
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let base = self.config.base_addr;
        vec![
            IoRegisterView {
                address: base,
                name: "SP0256 Status".to_string(),
                value: self.status_byte(),
            },
            IoRegisterView {
                address: base.wrapping_add(2),
                name: format!(
                    "CTS256 Out ({})",
                    ALLOPHONE_NAMES[usize::from(self.cts.last_allophone() & 0x3F)]
                ),
                value: self.cts.last_allophone(),
            },
            IoRegisterView {
                address: base.wrapping_add(3),
                name: "CTS256 Status".to_string(),
                value: self.cts_status_byte(),
            },
        ]
    }

    pub fn state_snapshot(&self) -> SpeechStateDto {
        SpeechStateDto {
            config: self.config,
            lrq_ready: self.sp.lrq_ready(),
            standby: self.sp.standby(),
            speaking: !self.sp.standby(),
            last_allophone: self.last_allophone,
            cts_enabled: self.config.cts_enabled,
            cts_busy: self.cts.busy(),
            cts_last_allophone: self.cts.last_allophone(),
            cts_booting: self.cts.booting(),
            cts_input_pending: self.cts.input_pending() as u32,
            cts_output_pending: self.cts.output_pending() as u32,
            cts_fifo_full: self.cts.fifo_full(),
            cts_buffer_full: self.cts.busy_pin(),
            cts_recent: self.cts.recent(),
            cts_cr_only: true,
            cts_greeting: self.cts_greeting,
        }
    }
}

impl Default for SpeechBox {
    fn default() -> Self {
        Self::new(SpeechConfig::default())
    }
}

impl std::fmt::Debug for SpeechBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechBox")
            .field("config", &self.config)
            .field("host_clock_hz", &self.host_clock_hz)
            .field("last_allophone", &self.last_allophone)
            .finish()
    }
}

/// Persisted form: the configuration plus the host clock and greeting flag.
/// Older snapshots stored only the flat [`SpeechConfig`] and still load.
#[derive(Serialize, Deserialize)]
struct SpeechBoxSnapshot {
    #[serde(flatten)]
    config: SpeechConfig,
    #[serde(default = "default_host_clock_hz")]
    host_clock_hz: u32,
    #[serde(default = "default_true")]
    cts_greeting: bool,
}

fn default_host_clock_hz() -> u32 {
    DEFAULT_HOST_CLOCK_HZ
}

// Snapshots persist only the settings; the live chips restart on load (the
// CTS boots per `cts_enabled`, like a power-on).
impl Serialize for SpeechBox {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SpeechBoxSnapshot {
            config: self.config,
            host_clock_hz: self.host_clock_hz,
            cts_greeting: self.cts_greeting,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SpeechBox {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let snap = SpeechBoxSnapshot::deserialize(deserializer)?;
        let mut sb = Self::new(snap.config);
        sb.host_clock_hz = snap.host_clock_hz.max(1);
        sb.cts_greeting = snap.cts_greeting;
        sb.reset();
        Ok(sb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> SpeechBox {
        SpeechBox::new(SpeechConfig {
            enabled: true,
            base_addr: 0xFF50,
            xtal_hz: DEFAULT_XTAL_HZ,
            cts_enabled: true,
        })
    }

    #[test]
    fn disabled_does_not_handle_or_produce_audio() {
        let mut sb = SpeechBox::new(SpeechConfig::default());
        assert!(!sb.handles(0xFF50));
        sb.tick(100_000);
        assert!(sb.drain_audio().is_empty());
    }

    #[test]
    fn maps_four_bytes() {
        let sb = enabled();
        assert!(sb.handles(0xFF50));
        assert!(sb.handles(0xFF53));
        assert!(!sb.handles(0xFF54));
        assert!(!sb.handles(0xFF4F));
    }

    #[test]
    fn status_reports_idle_when_quiet() {
        let sb = enabled();
        let st = sb.read(0xFF51);
        assert_eq!(st & ST_LRQ_READY, ST_LRQ_READY);
        assert_eq!(st & ST_STANDBY, ST_STANDBY);
    }

    #[test]
    fn writing_allophone_marks_busy() {
        let mut sb = enabled();
        sb.write(0xFF50, 0x0B); // voiced vowel
        let st = sb.read(0xFF51);
        assert_eq!(st & ST_LRQ_READY, 0, "LRQ should be busy after ALD write");
        assert_eq!(st & ST_STANDBY, 0, "should not be in standby while pending");
    }

    #[test]
    fn tick_emits_realtime_sample_count() {
        let mut sb = enabled();
        sb.tick(HOST_CLOCK_HZ);
        let n = sb.drain_audio().len();
        assert_eq!(n, AUDIO_SAMPLE_RATE as usize);
    }

    #[test]
    fn host_clock_sets_the_time_base() {
        // One second of E clock at 1.8432 MHz (MsBasic) is 44.1k samples,
        // exactly like one second at the default 894,886 Hz.
        let mut fast = enabled();
        fast.set_host_clock_hz(1_843_200);
        fast.tick(1_843_200);
        let mut coco = enabled();
        coco.tick(894_886);
        assert_eq!(fast.drain_audio().len(), AUDIO_SAMPLE_RATE as usize);
        assert_eq!(coco.drain_audio().len(), AUDIO_SAMPLE_RATE as usize);
        // Speech takes the same wall time on both: after 894,886 cycles of a
        // 1.8432 MHz machine (≈0.49 s) the 0.29 s OY is still speaking.
        let mut sb = SpeechBox::new(SpeechConfig {
            enabled: true,
            cts_enabled: false,
            ..SpeechConfig::default()
        });
        sb.set_host_clock_hz(1_843_200);
        sb.write(0xFF50, 0x05); // OY, 2944 samples ≈ 0.29 s
        sb.tick(400_000); // ≈ 0.22 s
        assert!(!sb.state_snapshot().standby, "still speaking at 0.22 s");
        sb.tick(300_000); // ≈ 0.38 s
        assert!(sb.state_snapshot().standby, "done by 0.38 s");
    }

    #[test]
    fn allophone_produces_audible_energy() {
        let mut sb = enabled();
        sb.write(0xFF50, 0x0B);
        let samples = sb.take_samples(20_000);
        let peak = samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
        assert!(
            peak > 0.15,
            "speech should be in the same ballpark as AY (peak={peak})"
        );
    }

    #[test]
    fn loud_allophones_are_soft_limited_not_clipped() {
        let mut worst = 0.0_f32;
        let mut near_full = 0usize;
        let mut total = 0usize;
        for code in 5u8..64 {
            let mut sb = enabled();
            sb.set_cts_greeting(false);
            sb.write(0xFF53, 0x01);
            sb.write(0xFF50, code);
            let out = sb.run_until_idle_collect(200_000);
            total += out.len();
            for s in out {
                worst = worst.max(s.abs());
                if s.abs() >= 0.99 {
                    near_full += 1;
                }
            }
        }
        assert!(worst < 1.0, "peak {worst} must stay below full scale");
        assert_eq!(near_full, 0, "no flat-topped samples out of {total}");
        assert!(worst > 0.5, "loud vowels still use the range ({worst})");
        // The limiter is transparent below the knee and monotonic above it.
        assert_eq!(soft_limit(0.5), 0.5);
        assert!(soft_limit(1.2) > soft_limit(1.0) && soft_limit(3.0) < 1.0);
        assert_eq!(soft_limit(-1.5), -soft_limit(1.5));
    }

    #[test]
    fn idle_after_utterance_is_silent() {
        let mut sb = enabled();
        sb.reset();
        sb.run_until_idle(400_000);
        let _ = sb.drain_audio();
        assert!(sb.state_snapshot().standby);
        // Continuous host ticks (AY run-loop) must not keep leaking LPC leftovers.
        let rest = sb.take_samples(8_000);
        let peak = rest.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
        assert!(
            peak < 0.02,
            "idle speech must not hiss under the AY mix (peak={peak})"
        );
    }

    #[test]
    fn snapshot_reports_last_allophone() {
        let mut sb = enabled();
        sb.write(0xFF50, 0x1B);
        let st = sb.state_snapshot();
        assert_eq!(st.last_allophone, 0x1B);
    }

    #[test]
    fn say_text_produces_audio_then_goes_idle() {
        let mut sb = enabled();
        sb.reset();
        // Drain the "O.K." greeting first.
        sb.run_until_idle(400_000);
        let _ = sb.drain_audio();

        sb.say_text("Hello");
        let mut audio = Vec::new();
        // Render with a bounded catch-up.
        for _ in 0..20 {
            sb.run_until_idle(20_000);
            audio.extend(sb.drain_audio());
            let st = sb.state_snapshot();
            if st.standby && !st.cts_busy {
                break;
            }
        }
        let peak = audio.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
        assert!(
            peak > 0.15,
            "say_text should be in the same ballpark as AY (peak={peak})"
        );
        let st = sb.state_snapshot();
        assert!(st.standby, "chip should return to standby after speaking");
        assert!(!st.cts_busy, "CTS should be idle after the utterance");
    }

    #[test]
    fn ff52_ascii_path_drives_cts() {
        let mut sb = enabled();
        sb.reset();
        sb.run_until_idle(400_000);
        // Write ASCII "HI" + CR via the CTS MMIO byte.
        sb.write(0xFF52, b'H');
        sb.write(0xFF52, b'I');
        sb.write(0xFF52, b'\r');
        // CTS should report busy (bit0 of $FF53) with queued conversion.
        assert_eq!(sb.read(0xFF53) & CTS_BUSY, CTS_BUSY);
        sb.run_until_idle(200_000);
        assert_eq!(sb.read(0xFF53) & CTS_BUSY, 0, "CTS should finish");
    }

    #[test]
    fn cts_greets_on_enable() {
        let mut sb = enabled();
        sb.reset();
        // A freshly reset+enabled chip boots and greets (busy).
        assert_eq!(sb.read(0xFF53) & CTS_BUSY, CTS_BUSY);
        assert!(sb.state_snapshot().cts_booting);
        sb.run_until_idle(400_000);
        assert_eq!(names(&sb.cts.log), "OW PA1 PA3 KK1 EY PA3");
    }

    fn names(codes: &[u8]) -> String {
        crate::cts256::tests::names(codes)
    }

    /// Enabled block, greeting off, CTS booted and idle.
    fn quiet() -> SpeechBox {
        let mut sb = enabled();
        sb.set_cts_greeting(false);
        sb.reset();
        sb.run_until_idle(100_000);
        let _ = sb.drain_audio();
        sb.cts.log.clear();
        sb
    }

    /// Write `text` to $FF52, ticking `gap` E cycles after every byte, then
    /// let the chips finish; returns the allophones the CTS produced.
    fn speak_via_mmio(text: &[u8], gap: u32) -> Vec<u8> {
        let mut sb = quiet();
        for &b in text {
            sb.write(0xFF52, b);
            sb.tick(gap);
        }
        sb.run_until_idle(3_000_000);
        assert!(sb.state_snapshot().standby && !sb.state_snapshot().cts_busy);
        sb.cts.log.clone()
    }

    #[test]
    fn allophones_do_not_depend_on_write_speed() {
        let text = b"HELLO WORLD. SHE SELLS 123 SEA SHELLS, $5.00!\rTHE END\r";
        // A tight 6809 STA loop (~12 E cycles per byte) ...
        let fast = speak_via_mmio(text, 12);
        // ... one byte per video frame, and a slow typist (5 bytes/s).
        let frame = speak_via_mmio(text, 14_915);
        let slow = speak_via_mmio(text, 178_977);
        assert!(fast.len() > 40, "spoke: {}", names(&fast));
        assert_eq!(names(&fast), names(&frame));
        assert_eq!(names(&fast), names(&slow));
        assert!(names(&fast).starts_with("HH1 EH LL OW PA2 WW ER1 LL PA2 DD1"));
    }

    #[test]
    fn restored_session_keeps_the_cts_enabled() {
        let mut sb = enabled();
        sb.set_host_clock_hz(1_843_200);
        sb.set_cts_greeting(false);
        let json = serde_json::to_value(&sb).expect("serialize");
        let mut restored: SpeechBox = serde_json::from_value(json).expect("restore");
        assert_eq!(restored.config(), sb.config());
        assert_eq!(restored.host_clock_hz(), 1_843_200);
        assert!(!restored.cts_greeting());
        restored.run_until_idle(100_000);
        for &b in b"HI\r" {
            restored.write(0xFF52, b);
        }
        assert_eq!(restored.read(0xFF53) & CTS_BUSY, CTS_BUSY);
        restored.run_until_idle(400_000);
        assert_eq!(names(&restored.cts.log), "HH1 AY PA3");
        assert_eq!(restored.read(0xFF53) & CTS_BUSY, 0);

        // Snapshots from before the host clock was stored still load, with
        // the CTS running (and greeting, like a power-on).
        let old = serde_json::json!({
            "enabled": true, "base_addr": 0xFF50, "xtal_hz": 3_120_000, "cts_enabled": true
        });
        let mut sb2: SpeechBox = serde_json::from_value(old).expect("old snapshot");
        assert_eq!(sb2.host_clock_hz(), DEFAULT_HOST_CLOCK_HZ);
        assert!(sb2.cts_greeting());
        sb2.run_until_idle(400_000);
        assert_eq!(names(&sb2.cts.log), "OW PA1 PA3 KK1 EY PA3");
    }

    #[test]
    fn take_samples_is_bounded_past_the_buffer_cap() {
        let mut sb = enabled();
        sb.tick(HOST_CLOCK_HZ * 3); // queue full (cap = 2 s)
        assert_eq!(sb.audio.len(), AUDIO_BUFFER_CAP);
        let out = sb.take_samples(AUDIO_BUFFER_CAP + 1_000);
        assert_eq!(out.len(), AUDIO_BUFFER_CAP + 1_000);
        assert!(sb.audio.is_empty());
    }

    #[test]
    fn chip_time_advances_with_a_full_audio_buffer() {
        let mut sb = SpeechBox::new(SpeechConfig {
            enabled: true,
            cts_enabled: false,
            ..SpeechConfig::default()
        });
        sb.tick(HOST_CLOCK_HZ * 3); // nobody drains the queue
        sb.write(0xFF50, 0x05); // OY ≈ 0.29 s
        // A 6809 polling LRQ/SBY in step mode: tiny ticks, no draining.
        let mut polls = 0u32;
        while sb.read(0xFF51) & ST_STANDBY == 0 {
            sb.tick(20);
            polls += 1;
            assert!(polls < 100_000, "speech chip stalled");
        }
        assert!(polls > 10_000, "took real time ({polls} polls)");
        assert_eq!(sb.audio.len(), AUDIO_BUFFER_CAP, "oldest samples dropped");
        let newest = sb.drain_audio();
        let tail = &newest[newest.len() - 20_000..];
        assert!(tail.iter().any(|s| s.abs() > 0.05), "newest audio is the OY");
    }

    #[test]
    fn collect_returns_everything_beyond_the_cap() {
        let mut sb = quiet();
        sb.say_text("THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG. HELLO WORLD.");
        let out = sb.run_until_idle_collect(1_000_000);
        assert!(
            out.len() > AUDIO_BUFFER_CAP,
            "utterance longer than the 2 s queue ({} samples)",
            out.len()
        );
        assert!(sb.state_snapshot().standby && !sb.state_snapshot().cts_busy);
        assert!(sb.audio.is_empty());
        // The plain variant keeps only the newest samples.
        let mut sb = quiet();
        sb.say_text("THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG. HELLO WORLD.");
        sb.run_until_idle(1_000_000);
        assert_eq!(sb.drain_audio().len(), AUDIO_BUFFER_CAP);
    }

    #[test]
    fn cts_reset_register_aborts_speech_and_reboots() {
        let mut sb = quiet();
        sb.say_text("HELLO WORLD");
        sb.tick(HOST_CLOCK_HZ / 5);
        assert!(!sb.state_snapshot().standby, "speaking");
        sb.set_cts_greeting(true);
        sb.write(0xFF53, 0x01);
        let st = sb.state_snapshot();
        assert!(st.standby && st.lrq_ready, "SP0256 aborted");
        assert!(st.cts_booting && st.cts_busy, "CTS restarts");
        sb.cts.log.clear();
        sb.run_until_idle(400_000);
        assert_eq!(names(&sb.cts.log), "OW PA1 PA3 KK1 EY PA3");
    }

    #[test]
    fn status_reports_a_full_input_fifo() {
        let mut sb = quiet();
        for _ in 0..crate::cts256::HOST_FIFO_CAP {
            sb.write(0xFF52, b'A');
        }
        assert_eq!(sb.read(0xFF53), CTS_BUSY | CTS_FIFO_FULL);
        assert!(sb.state_snapshot().cts_fifo_full);
        assert_eq!(
            sb.state_snapshot().cts_input_pending as usize,
            crate::cts256::HOST_FIFO_CAP
        );
        // Reads are side-effect free.
        assert_eq!(sb.peek(0xFF53), sb.read(0xFF53));
    }
}
