//! Virtual printers.
//!
//! CoCo: the ROM bit-bangs RS-232 on PIA1 PA1 (idle/mark = high; start bit
//! low, 8 data bits LSB first, stop bit high; 600 baud by default, set by the
//! delay constant at `$95`) and waits for PIA1 PB0 = 0 (printer ready) before
//! and after every byte. [`SerialReceiver`] decodes that stream: frames are
//! sampled in the middle of each bit from the recorded edge times, starting
//! at 1/600 s per bit. The bit time adapts to other rates:
//! a run clearly shorter than one bit inside a frame means a faster rate
//! (the frame is decoded with the new estimate), and a history in which all
//! runs are whole multiples of a longer cell means a slower rate.
//!
//! Dragon 32: Centronics port — data on PIA0 port B, STROBE is PIA1 PA1
//! (latched on the rising edge), BUSY on PIA1 PB0 (never busy here) and a
//! short active-low ACK pulse on PIA1 CA1 after each byte.

use serde::{Deserialize, Serialize};

/// ROM default rate.
const DEFAULT_BAUD: f64 = 600.0;
/// Shortest plausible bit time. The ROM's fastest setting is ~96 E cycles
/// (~107 us) per bit; PIA set-up glitches (e.g. DDR written before the data
/// register at power-on, ~30 E cycles) are shorter.
const MIN_BIT_SECONDS: f64 = 1.0 / 11_000.0;
/// Consecutive framing errors after which the bit time is re-learnt.
const MAX_FRAMING_ERRORS: u32 = 4;
/// Runs remembered for the slower-rate check.
const RUN_HISTORY: usize = 32;
/// Runs needed before a slower rate is adopted.
const RUN_QUORUM: usize = 24;
/// Maximum kept output (older text is dropped).
const MAX_TEXT: usize = 256 * 1024;
/// Dragon ACK pulse width.
const ACK_SECONDS: f64 = 7e-6;

/// Asynchronous serial receiver with bit-rate adaptation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SerialReceiver {
    /// Last line level (true = mark).
    #[serde(default = "mark")]
    line: bool,
    /// Time stamp (in `1/clock_hz` s units) of the last edge.
    #[serde(default)]
    last_edge: u64,
    /// Current bit-time estimate in time units (0 = not yet set).
    #[serde(default)]
    bit_time: f64,
    #[serde(default)]
    in_frame: bool,
    #[serde(default)]
    frame_start: u64,
    /// Edge times inside the current frame, relative to its start edge.
    #[serde(default)]
    edges: Vec<u64>,
    /// Recent in-frame run lengths (time units).
    #[serde(default)]
    runs: Vec<f64>,
    /// Framing errors in a row.
    #[serde(default)]
    errors: u32,
}

fn mark() -> bool {
    true
}

impl Default for SerialReceiver {
    fn default() -> Self {
        Self {
            line: true,
            last_edge: 0,
            bit_time: 0.0,
            in_frame: false,
            frame_start: 0,
            edges: Vec::new(),
            runs: Vec::new(),
            errors: 0,
        }
    }
}

impl SerialReceiver {
    /// Line level `level` observed at time `now` (in units of `1/clock_hz`
    /// seconds); returns a received byte.
    pub fn sample(&mut self, now: u64, clock_hz: u32, level: bool) -> Option<u8> {
        if self.bit_time <= 0.0 || self.errors >= MAX_FRAMING_ERRORS {
            // Start over from the ROM default rate.
            self.bit_time = f64::from(clock_hz.max(1)) / DEFAULT_BAUD;
            self.runs.clear();
            self.errors = 0;
        }
        if level != self.line {
            let run = now.saturating_sub(self.last_edge) as f64;
            let was_in_frame = self.in_frame;
            self.line = level;
            self.last_edge = now;
            if was_in_frame {
                self.note_run(run, f64::from(clock_hz) * MIN_BIT_SECONDS);
                if self.in_frame {
                    self.edges.push(now - self.frame_start);
                }
            } else if !level {
                self.in_frame = true;
                self.frame_start = now;
                self.edges.clear();
            }
        }
        if self.in_frame && (now - self.frame_start) as f64 >= 9.5 * self.bit_time {
            return self.finish_frame();
        }
        None
    }

    fn note_run(&mut self, run: f64, min_bit: f64) {
        if run < min_bit {
            return; // glitch
        }
        if run < 0.7 * self.bit_time {
            // Faster than assumed: the shortest run is one bit.
            self.bit_time = run;
            self.runs.clear();
            return;
        }
        if run <= 10.0 * self.bit_time {
            if self.runs.len() >= RUN_HISTORY {
                self.runs.remove(0);
            }
            self.runs.push(run);
        }
    }

    /// Level inside the current frame at `t` time units after the start edge.
    fn level_at(&self, t: f64) -> bool {
        let mut level = false; // start bit
        for &e in &self.edges {
            if (e as f64) <= t {
                level = !level;
            } else {
                break;
            }
        }
        level
    }

    fn finish_frame(&mut self) -> Option<u8> {
        let t = self.bit_time;
        let mut value = 0u8;
        for i in 0..8 {
            if self.level_at((1.5 + f64::from(i)) * t) {
                value |= 1 << i;
            }
        }
        let stop = 9.5 * t;
        let valid = !self.level_at(0.5 * t) && self.level_at(stop);
        // After a late switch to a shorter bit time, a falling edge past the
        // stop bit (odd index: high→low) already starts the next frame.
        let next = self
            .edges
            .iter()
            .enumerate()
            .find(|&(i, &e)| i % 2 == 1 && e as f64 > stop)
            .map(|(i, &e)| (i, e));
        match next {
            Some((i, e)) => {
                self.edges = self.edges[i + 1..].iter().map(|&x| x - e).collect();
                self.frame_start += e;
            }
            None => {
                self.in_frame = false;
                self.edges.clear();
            }
        }
        self.adapt_slower();
        if valid {
            self.errors = 0;
            Some(value)
        } else {
            self.errors += 1;
            None
        }
    }

    /// Adopt a longer bit time when every recent run is a whole multiple of it.
    fn adapt_slower(&mut self) {
        if self.runs.len() < RUN_QUORUM {
            return;
        }
        let m = self.runs.iter().copied().fold(f64::INFINITY, f64::min);
        if m < 1.6 * self.bit_time {
            return;
        }
        let whole = self.runs.iter().all(|&r| {
            let q = r / m;
            (q - q.round()).abs() < 0.2
        });
        if whole {
            self.bit_time = m;
            self.runs.clear();
        }
    }

    /// Current bit time estimate (time units).
    #[cfg(test)]
    pub fn bit_time(&self) -> f64 {
        self.bit_time
    }

    pub fn reset(&mut self) {
        self.in_frame = false;
        self.edges.clear();
        self.line = true;
        self.errors = 0;
    }
}

/// Printer text sink shared by both machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Printer {
    #[serde(default)]
    text: String,
    #[serde(default)]
    last_cr: bool,
    /// CoCo serial receiver.
    #[serde(default)]
    serial: SerialReceiver,
    /// Dragon: last STROBE level (PIA1 PA1). Starts high so the pulled-up
    /// pin at power-on (DDR = input) is not taken for a strobe.
    #[serde(default = "mark")]
    strobe: bool,
    /// Dragon: ACK held low until this time stamp.
    #[serde(default)]
    ack_until: u64,
    /// Bytes received in total.
    #[serde(default)]
    bytes: u64,
}

impl Default for Printer {
    fn default() -> Self {
        Self {
            text: String::new(),
            last_cr: false,
            serial: SerialReceiver::default(),
            strobe: true,
            ack_until: 0,
            bytes: 0,
        }
    }
}

impl Printer {
    /// CoCo: PIA1 PA1 level after a CPU step ending at time `now`.
    pub fn serial_tick(&mut self, now: u64, clock_hz: u32, tx: bool) {
        if let Some(byte) = self.serial.sample(now, clock_hz, tx) {
            self.push_byte(byte);
        }
    }

    /// Dragon: STROBE (PIA1 PA1) and data (PIA0 port B) after a CPU step.
    pub fn parallel_tick(&mut self, now: u64, clock_hz: u32, strobe: bool, data: u8) {
        if strobe && !self.strobe {
            self.push_byte(data);
            let ack = (ACK_SECONDS * f64::from(clock_hz)).ceil() as u64;
            self.ack_until = now + ack.max(1);
        }
        self.strobe = strobe;
    }

    /// Dragon ACK line level (active low) at time `now`.
    pub fn ack_line(&self, now: u64) -> bool {
        now >= self.ack_until
    }

    fn push_byte(&mut self, byte: u8) {
        self.bytes += 1;
        match byte {
            b'\r' => {
                self.text.push('\n');
                self.last_cr = true;
                return;
            }
            b'\n' => {
                if !self.last_cr {
                    self.text.push('\n');
                }
            }
            b'\t' => self.text.push('\t'),
            0x0C => self.text.push('\u{0C}'),
            0x20..=0x7E => self.text.push(char::from(byte)),
            // NUL padding, other control codes, graphics: not printable text.
            _ => return,
        }
        self.last_cr = false;
        if self.text.len() > MAX_TEXT {
            let cut = self.text.len() - MAX_TEXT / 2;
            let cut = (cut..self.text.len())
                .find(|&i| self.text.is_char_boundary(i))
                .unwrap_or(self.text.len());
            self.text.drain(..cut);
        }
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.text)
    }

    #[cfg(test)]
    pub fn bytes_received(&self) -> u64 {
        self.bytes
    }

    pub fn reset(&mut self) {
        self.serial.reset();
        // RESET makes PA1 an input again: pulled high, not a strobe.
        self.strobe = true;
        self.ack_until = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOCK: u32 = 894_886;

    /// Line levels of a serial stream, one entry per `step` cycles.
    fn serial_wave(text: &[u8], bit: f64, gap: f64) -> Vec<(u64, bool)> {
        let mut edges = Vec::new(); // (time, level) changes
        let mut t = 2.0 * bit;
        for &b in text {
            edges.push((t, false));
            for i in 0..8 {
                edges.push((t + bit * f64::from(i + 1), (b >> i) & 1 != 0));
            }
            edges.push((t + 9.0 * bit, true));
            t += 10.0 * bit + gap;
        }
        edges.into_iter().map(|(t, l)| (t as u64, l)).collect()
    }

    fn run_serial(text: &[u8], bit: f64, gap: f64, step: u64) -> (Printer, String) {
        let wave = serial_wave(text, bit, gap);
        let end = wave.last().map(|e| e.0).unwrap_or(0) + (20.0 * bit) as u64;
        let mut p = Printer::default();
        let mut level = true;
        let mut idx = 0;
        let mut now = 0;
        while now < end {
            now += step;
            while idx < wave.len() && wave[idx].0 <= now {
                level = wave[idx].1;
                idx += 1;
            }
            p.serial_tick(now, CLOCK, level);
        }
        let out = p.take_output();
        (p, out)
    }

    #[test]
    fn decodes_rom_rate_600_baud_llist_output() {
        // ROM timing: ~1488 cycles per bit, ~40 cycles between frames.
        let (p, out) = run_serial(b"10 PRINT \"HELLO\"\r20 GOTO 10\r", 1488.0, 1488.0 + 40.0, 5);
        assert_eq!(out, "10 PRINT \"HELLO\"\n20 GOTO 10\n");
        assert_eq!(p.bytes_received(), 28);
    }

    #[test]
    fn adapts_to_a_faster_rate_without_losing_characters() {
        // POKE 150,41 → ~1200 baud.
        let bit = f64::from(CLOCK) / 1200.0;
        let (p, out) = run_serial(b"1200 BAUD OK\r", bit, bit, 3);
        assert_eq!(out, "1200 BAUD OK\n");
        assert!((p.serial.bit_time() - bit).abs() < 8.0);
        // Double-speed CPU running the 600 baud ROM loop: same cycle timing,
        // half the clock-derived estimate... and back at 2400 baud.
        let bit = f64::from(CLOCK) / 2400.0;
        let (_, out) = run_serial(b"FAST!\r", bit, bit, 2);
        assert_eq!(out, "FAST!\n");
    }

    #[test]
    fn adapts_to_a_slower_rate_after_a_few_characters() {
        let bit = f64::from(CLOCK) / 300.0;
        let text = b"THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG\rTHE QUICK BROWN FOX JUMPS OVER THE LAZY DOG\r";
        let (p, out) = run_serial(text, bit, bit, 7);
        assert!(
            out.ends_with("THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG\n"),
            "got {out:?}"
        );
        assert!((p.serial.bit_time() - bit).abs() < 10.0);
    }

    #[test]
    fn recovers_from_a_wrong_bit_time() {
        // A 90-cycle pulse (plausible 9600 baud bit) sets a far too short
        // estimate; framing errors bring the receiver back to 600 baud.
        let mut p = Printer::default();
        let mut now = 1000;
        p.serial_tick(now, CLOCK, true);
        now += 5;
        p.serial_tick(now, CLOCK, false);
        now += 90;
        p.serial_tick(now, CLOCK, true);
        now += 200;
        p.serial_tick(now, CLOCK, false);
        now += 90;
        p.serial_tick(now, CLOCK, true);
        assert!(p.serial.bit_time() < 100.0);
        let wave = serial_wave(b"RECOVERED RECOVERED\r", 1488.0, 1528.0);
        let mut level = true;
        let mut idx = 0;
        let base = now;
        let end = base + wave.last().unwrap().0 + 30_000;
        while now < end {
            now += 5;
            while idx < wave.len() && base + wave[idx].0 <= now {
                level = wave[idx].1;
                idx += 1;
            }
            p.serial_tick(now, CLOCK, level);
        }
        let out = p.take_output();
        assert!(out.ends_with("RECOVERED\n"), "got {out:?}");
    }

    #[test]
    fn framing_errors_and_glitches_are_ignored() {
        let mut p = Printer::default();
        let mut now = 0;
        // A 10-cycle glitch on the line, then silence.
        for level in [false, true] {
            now += 10;
            p.serial_tick(now, CLOCK, level);
        }
        for _ in 0..10_000 {
            now += 5;
            p.serial_tick(now, CLOCK, true);
        }
        // A "break" (line low for a long time) yields no byte.
        for _ in 0..10_000 {
            now += 5;
            p.serial_tick(now, CLOCK, false);
        }
        assert_eq!(p.take_output(), "");
    }

    #[test]
    fn text_normalisation() {
        let mut p = Printer::default();
        for &b in b"A\r\nB\rC\nD\x00\x01\x80\xFFE\tF\x0C" {
            p.push_byte(b);
        }
        assert_eq!(p.take_output(), "A\nB\nC\nDE\tF\u{0C}");
        assert_eq!(p.take_output(), "");
    }

    #[test]
    fn output_is_bounded() {
        let mut p = Printer::default();
        for _ in 0..(MAX_TEXT + 10) {
            p.push_byte(b'X');
        }
        let out = p.take_output();
        assert!(out.len() <= MAX_TEXT && out.len() >= MAX_TEXT / 2);
    }

    #[test]
    fn dragon_parallel_latches_on_strobe_rising_edge_and_pulses_ack() {
        let mut p = Printer::default();
        let mut now = 100;
        // Power-on: PA1 pulled high is not a strobe.
        p.parallel_tick(now, 888_625, true, b'X');
        assert!(p.ack_line(now));
        // Data first, then the ROM's STB #$02 / CLR pulse.
        p.parallel_tick(now, 888_625, false, b'H');
        assert!(p.ack_line(now));
        now += 5;
        p.parallel_tick(now, 888_625, true, b'H');
        assert!(!p.ack_line(now), "ACK low right after the strobe");
        now += 7;
        p.parallel_tick(now, 888_625, false, b'H');
        now += 5;
        assert!(p.ack_line(now), "ACK back high after ~7 µs");
        // Holding STROBE high does not print again.
        for _ in 0..3 {
            now += 5;
            p.parallel_tick(now, 888_625, true, b'I');
        }
        now += 5;
        p.parallel_tick(now, 888_625, false, b'I');
        now += 5;
        p.parallel_tick(now, 888_625, true, b'\r');
        assert_eq!(p.take_output(), "HI\n");
    }

    #[test]
    fn serde_round_trip() {
        let mut p = Printer::default();
        p.push_byte(b'Q');
        let json = serde_json::to_value(&p).unwrap();
        let mut back: Printer = serde_json::from_value(json).unwrap();
        assert_eq!(back.take_output(), "Q");
        let empty: Printer = serde_json::from_str("{}").unwrap();
        assert!(empty.serial.line, "idle line defaults to mark");
    }
}
