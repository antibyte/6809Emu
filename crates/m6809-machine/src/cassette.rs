//! Cassette interface for the CoCo 2 / Dragon 32.
//!
//! Playback: a `.CAS` image (the raw byte stream the ROM reads: `$55` leader,
//! `$3C` sync, blocks of type/length/data/checksum) is turned into the FSK
//! waveform the BASIC ROM expects on PIA1 PA0 while the relay (PIA1 CA2) is
//! closed: one cycle of 2400 Hz per `1` bit and one cycle of 1200 Hz per `0`
//! bit, least significant bit first, each cycle high half first. Timing is in
//! real time (from the elapsed-time ratio the board passes), like a tape: a
//! CPU sped up by the SAM sees the same tape as the ROM would.
//!
//! Most `.CAS` files carry no gaps and often short leaders, while the ROM
//! stops the motor after the filename block and then spends ~0.6 s in a
//! blind motor-on delay before it looks for the next leader. Like MAME's
//! `coco_cas` loader, the player therefore inserts one second of silence and
//! a fresh 128-byte leader before every filename block and before the first
//! block after a filename or end-of-file block. Blocks are found at any bit
//! offset and validated by their checksum; everything after the first
//! undecodable block is played verbatim.
//!
//! Recording: while the motor runs, the 6-bit DAC output is watched for
//! rising crossings of its mid level (with hysteresis). The time between two
//! rising crossings is one bit cell (real time, from the tick/clock ratio):
//! shorter than the split is a `1`, longer a `0`. The ROM writes ~2050 Hz /
//! ~1100 Hz sine cycles; the split starts at 1/1500 s and follows the
//! midpoint of the shortest and longest recent cells whenever both kinds are
//! present (e.g. in the leader), so a CSAVE made with the SAM speed-up (a
//! twice as fast, cycle-timed waveform) still decodes. Bits are assembled
//! LSB first; bytes are aligned on the `$3C` sync byte and blocks are
//! tracked through their length byte, so the result is a standard `.CAS`
//! image (leader, sync, block, trailing leader).

use serde::{Deserialize, Serialize};

use crate::peripherals::hex_bytes;

/// Tape time unit: half a cycle of the 2400 Hz tone.
pub(crate) const UNITS_PER_SECOND: u64 = 4_800;
/// Half-cycle length of a `1` bit (2400 Hz).
const HALF_ONE: u32 = 1;
/// Half-cycle length of a `0` bit (1200 Hz).
const HALF_ZERO: u32 = 2;
/// Silence inserted where a real recording has a motor stop/start gap.
const GAP_UNITS: u32 = UNITS_PER_SECOND as u32;
/// Leader length written by CSAVE (and re-inserted before gapped blocks).
const LEADER_BYTES: usize = 128;
const LEADER: u8 = 0x55;
const SYNC: u8 = 0x3C;
const BLOCK_FILENAME: u8 = 0x00;
const BLOCK_EOF: u8 = 0xFF;
/// Largest tape image accepted / recording kept (real .CAS files are a few
/// KiB to a few hundred KiB).
pub(crate) const MAX_TAPE_BYTES: usize = 4 * 1024 * 1024;

/// Recorder: DAC hysteresis around the mid level (32 of 0..63).
const REC_RISE: u8 = 34;
const REC_FALL: u8 = 30;
/// Initial 0/1 split: cells shorter than this are `1` bits (MAME's 1500 Hz).
const REC_SPLIT_S: f64 = 1.0 / 1500.0;
/// Shorter crossings are glitches.
const REC_MIN_CELL_S: f64 = 1.0 / 6000.0;
/// Longer cells are gaps (silence between blocks).
const REC_MAX_CELL_S: f64 = 1.0 / 450.0;
/// Recent cells used to adapt the split.
const REC_RECENT: usize = 16;
/// Long/short cell ratio accepted as "both bit kinds present".
const REC_RATIO: std::ops::RangeInclusive<f64> = 1.5..=2.6;

fn default_split() -> f64 {
    REC_SPLIT_S
}

/// One element of the playback program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapeItem {
    /// Constant level for `units` tape units.
    Silence { units: u32, pos: usize },
    /// One byte, 8 FSK cycles.
    Byte { value: u8, pos: usize },
}

impl TapeItem {
    /// Offset in the tape image this item corresponds to.
    fn pos(&self) -> usize {
        match *self {
            TapeItem::Silence { pos, .. } | TapeItem::Byte { pos, .. } => pos,
        }
    }

    /// Number of constant-level segments.
    fn segments(&self) -> u8 {
        match self {
            TapeItem::Silence { .. } => 1,
            TapeItem::Byte { .. } => 16,
        }
    }

    /// Level and length (in units) of segment `sub`.
    fn segment(&self, sub: u8) -> (bool, u32) {
        match *self {
            TapeItem::Silence { units, .. } => (true, units.max(1)),
            TapeItem::Byte { value, .. } => {
                let bit = (value >> (sub / 2)) & 1 != 0;
                (sub % 2 == 0, if bit { HALF_ONE } else { HALF_ZERO })
            }
        }
    }
}

/// Byte formed by the 8 bits starting at bit offset `bit` (LSB first).
fn byte_at(image: &[u8], bit: usize) -> Option<u8> {
    if bit + 8 > image.len() * 8 {
        return None;
    }
    let shift = bit % 8;
    let lo = image[bit / 8] >> shift;
    if shift == 0 {
        return Some(lo);
    }
    Some(lo | (image[bit / 8 + 1] << (8 - shift)))
}

/// A checksummed block found in the image.
struct FoundBlock {
    /// Bit offset of the `$3C` sync byte.
    sync_bit: usize,
    /// Type, length, data and checksum.
    bytes: Vec<u8>,
    /// Bit offset just past the checksum.
    end_bit: usize,
}

/// Find the next `$3C` sync at any bit offset from `from_bit` and read the
/// block behind it. `None` when there is none or it fails its checksum.
fn find_block(image: &[u8], from_bit: usize) -> Option<FoundBlock> {
    let total = image.len() * 8;
    let mut bit = from_bit;
    while bit + 8 <= total {
        if byte_at(image, bit) == Some(SYNC) {
            let block_type = byte_at(image, bit + 8)?;
            let len = byte_at(image, bit + 16)?;
            let mut bytes = Vec::with_capacity(usize::from(len) + 3);
            bytes.push(block_type);
            bytes.push(len);
            let mut sum = block_type.wrapping_add(len);
            let mut p = bit + 24;
            for _ in 0..len {
                let b = byte_at(image, p)?;
                sum = sum.wrapping_add(b);
                bytes.push(b);
                p += 8;
            }
            let checksum = byte_at(image, p)?;
            if checksum != sum {
                return None;
            }
            bytes.push(checksum);
            return Some(FoundBlock {
                sync_bit: bit,
                bytes,
                end_bit: p + 8,
            });
        }
        bit += 1;
    }
    None
}

/// Build the playback program for a `.CAS` image (see module docs).
fn build_program(image: &[u8]) -> Vec<TapeItem> {
    let mut items = Vec::with_capacity(image.len() + 4 * LEADER_BYTES);
    let mut from = 0usize;
    let mut last_type: Option<u8> = None;
    while let Some(block) = find_block(image, from) {
        let region = from / 8;
        let file_leader = (block.sync_bit - from) / 8;
        let block_type = block.bytes[0];
        let gapped = block_type == BLOCK_FILENAME
            || matches!(last_type, None | Some(BLOCK_FILENAME) | Some(BLOCK_EOF));
        if gapped {
            items.push(TapeItem::Silence {
                units: GAP_UNITS,
                pos: region,
            });
            for i in 0..LEADER_BYTES {
                items.push(TapeItem::Byte {
                    value: LEADER,
                    pos: region + i * file_leader / LEADER_BYTES,
                });
            }
        } else {
            for i in 0..file_leader {
                items.push(TapeItem::Byte {
                    value: LEADER,
                    pos: region + i,
                });
            }
        }
        let sync_pos = block.sync_bit / 8;
        items.push(TapeItem::Byte {
            value: LEADER,
            pos: sync_pos,
        });
        items.push(TapeItem::Byte {
            value: SYNC,
            pos: sync_pos,
        });
        for (i, &b) in block.bytes.iter().enumerate() {
            items.push(TapeItem::Byte {
                value: b,
                pos: (block.sync_bit + 8 * (i + 1)) / 8,
            });
        }
        items.push(TapeItem::Byte {
            value: LEADER,
            pos: (block.end_bit / 8).min(image.len()),
        });
        last_type = Some(block_type);
        from = block.end_bit;
    }
    // Whatever cannot be decoded as blocks is played as-is.
    let rest = from.div_ceil(8).min(image.len());
    if rest < image.len() && items.is_empty() {
        items.push(TapeItem::Silence {
            units: GAP_UNITS,
            pos: rest,
        });
    }
    for (i, &value) in image[rest..].iter().enumerate() {
        items.push(TapeItem::Byte {
            value,
            pos: rest + i,
        });
    }
    items
}

/// CSAVE decoder: DAC waveform → bits → `.CAS` bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Recorder {
    /// Recorded `.CAS` image since the last take.
    #[serde(default, with = "hex_bytes")]
    out: Vec<u8>,
    /// DAC currently above the mid level (hysteresis state).
    #[serde(default)]
    above: bool,
    /// A reference rising crossing exists.
    #[serde(default)]
    have_edge: bool,
    /// Seconds since the reference rising crossing.
    #[serde(default)]
    since_edge: f64,
    /// Current 0/1 split (seconds).
    #[serde(default = "default_split")]
    split: f64,
    /// Recent bit cells (seconds).
    #[serde(default)]
    recent: Vec<f64>,
    /// Byte-aligned inside a block.
    #[serde(default)]
    synced: bool,
    /// Last 8 bits while hunting for the sync byte (newest in bit 7).
    #[serde(default)]
    window: u8,
    /// Bits received while hunting.
    #[serde(default)]
    hunt_bits: u32,
    #[serde(default)]
    byte: u8,
    #[serde(default)]
    nbits: u8,
    /// Bytes received since the sync byte / expected block size (0 = unknown).
    #[serde(default)]
    block_pos: u32,
    #[serde(default)]
    block_len: u32,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            out: Vec::new(),
            above: false,
            have_edge: false,
            since_edge: 0.0,
            split: REC_SPLIT_S,
            recent: Vec::new(),
            synced: false,
            window: 0,
            hunt_bits: 0,
            byte: 0,
            nbits: 0,
            block_pos: 0,
            block_len: 0,
        }
    }
}

impl Recorder {
    /// Watch the DAC for `cycles` time units at `clock_hz` (motor running).
    fn tick(&mut self, cycles: u32, clock_hz: u32, dac: u8) {
        self.since_edge += f64::from(cycles) / f64::from(clock_hz.max(1));
        if self.above {
            if dac <= REC_FALL {
                self.above = false;
            }
            return;
        }
        if dac < REC_RISE {
            return;
        }
        self.above = true;
        if self.have_edge {
            let cell = self.since_edge;
            if cell < REC_MIN_CELL_S {
                return; // glitch: keep the previous reference crossing
            }
            self.on_cell(cell);
        }
        self.have_edge = true;
        self.since_edge = 0.0;
    }

    fn on_cell(&mut self, cell: f64) {
        if cell > REC_MAX_CELL_S {
            self.desync();
            return;
        }
        if self.recent.len() >= REC_RECENT {
            self.recent.remove(0);
        }
        self.recent.push(cell);
        if self.recent.len() >= REC_RECENT / 2 {
            let lo = self.recent.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = self.recent.iter().copied().fold(0.0, f64::max);
            if REC_RATIO.contains(&(hi / lo)) {
                self.split = (lo + hi) / 2.0;
            }
        }
        self.push_bit(cell < self.split);
    }

    fn push_bit(&mut self, bit: bool) {
        let bit = u8::from(bit);
        if !self.synced {
            self.window = (self.window >> 1) | (bit << 7);
            self.hunt_bits += 1;
            if self.hunt_bits >= 8 && self.window == SYNC {
                let leader = ((self.hunt_bits - 8) / 8).max(1);
                for _ in 0..leader {
                    self.emit(LEADER);
                }
                self.emit(SYNC);
                self.synced = true;
                self.byte = 0;
                self.nbits = 0;
                self.block_pos = 0;
                self.block_len = 0;
            }
            return;
        }
        self.byte = (self.byte >> 1) | (bit << 7);
        self.nbits += 1;
        if self.nbits < 8 {
            return;
        }
        let value = self.byte;
        self.emit(value);
        self.byte = 0;
        self.nbits = 0;
        self.block_pos += 1;
        if self.block_pos == 2 {
            // type, length, `length` data bytes, checksum
            self.block_len = 3 + u32::from(value);
        }
        if self.block_len != 0 && self.block_pos >= self.block_len {
            self.hunt();
        }
    }

    /// Append a byte to the recording (bounded).
    fn emit(&mut self, byte: u8) {
        if self.out.len() < MAX_TAPE_BYTES {
            self.out.push(byte);
        }
    }

    /// Back to hunting for a sync byte (after a block).
    fn hunt(&mut self) {
        self.synced = false;
        self.window = 0;
        self.hunt_bits = 0;
    }

    /// Lost the signal: drop any partial byte.
    fn desync(&mut self) {
        self.hunt();
        self.byte = 0;
        self.nbits = 0;
    }

    /// Motor stopped: finish the last bit cell and keep a trailing leader.
    fn flush(&mut self) {
        if self.have_edge
            && self.since_edge >= REC_MIN_CELL_S
            && self.since_edge <= REC_MAX_CELL_S
        {
            // The relay opens right after the last cycle was written.
            let cell = self.since_edge;
            self.on_cell(cell);
        }
        if !self.synced && self.hunt_bits >= 8 && !self.out.is_empty() {
            for _ in 0..self.hunt_bits / 8 {
                self.emit(LEADER);
            }
        }
        self.desync();
        self.have_edge = false;
        self.since_edge = 0.0;
    }
}

/// Cassette deck: tape image, playback position, relay and recorder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Cassette {
    #[serde(default)]
    name: Option<String>,
    /// The inserted `.CAS` image (kept so a restored session can go on playing).
    #[serde(default, with = "hex_bytes")]
    image: Vec<u8>,
    #[serde(default)]
    loaded: bool,
    /// Relay state (PIA1 CA2) as last seen.
    #[serde(default)]
    motor: bool,
    /// Playback cursor: program item, segment within it, units left in it.
    #[serde(default)]
    item: usize,
    #[serde(default)]
    sub: u8,
    #[serde(default)]
    units_left: u32,
    /// Fractional tape-unit accumulator (`time units * UNITS_PER_SECOND`).
    #[serde(default)]
    unit_acc: u64,
    #[serde(default)]
    unit_clock: u32,
    /// Image offset of the playback cursor.
    #[serde(default)]
    position: usize,
    #[serde(default)]
    recorder: Recorder,
    /// Derived from `image`; rebuilt on demand after a restore.
    #[serde(skip)]
    program: Option<Vec<TapeItem>>,
}

impl Cassette {
    /// Insert a tape image. Empty or oversized data leaves the deck empty.
    pub fn insert(&mut self, name: String, data: Vec<u8>) {
        if data.is_empty() || data.len() > MAX_TAPE_BYTES {
            self.eject();
            return;
        }
        self.name = Some(name);
        self.image = data;
        self.loaded = true;
        self.program = None;
        self.rewind();
    }

    pub fn eject(&mut self) {
        self.name = None;
        self.image = Vec::new();
        self.loaded = false;
        self.program = None;
        self.rewind();
    }

    pub fn rewind(&mut self) {
        self.item = 0;
        self.sub = 0;
        self.units_left = 0;
        self.unit_acc = 0;
        self.position = 0;
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }

    pub fn motor(&self) -> bool {
        self.motor
    }

    pub fn position(&self) -> usize {
        self.position.min(self.image.len())
    }

    pub fn length(&self) -> usize {
        self.image.len()
    }

    pub fn recorded_bytes(&self) -> usize {
        self.recorder.out.len()
    }

    pub fn take_recording(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.recorder.out)
    }

    /// Hardware reset: the relay opens.
    pub fn reset(&mut self) {
        self.set_motor(false);
    }

    fn set_motor(&mut self, on: bool) {
        if self.motor && !on {
            self.recorder.flush();
        }
        if !self.motor && on {
            // The first crossing after the relay closes only starts timing.
            self.recorder.have_edge = false;
            self.recorder.since_edge = 0.0;
        }
        self.motor = on;
    }

    /// Advance by one CPU step (`cycles` time units at `clock_hz`): `motor`
    /// is the relay level after the step, `dac` the 6-bit DAC value after it.
    pub fn tick(&mut self, cycles: u32, clock_hz: u32, motor: bool, dac: u8) {
        if self.motor && cycles > 0 && clock_hz > 0 {
            self.advance(cycles, clock_hz);
            self.recorder.tick(cycles, clock_hz, dac);
        }
        if motor != self.motor {
            self.set_motor(motor);
        }
    }

    /// Current segment of the playback program, if playing.
    fn current(&self) -> Option<(TapeItem, u8)> {
        if !self.loaded || !self.motor {
            return None;
        }
        let program = self.program.as_ref()?;
        program.get(self.item).map(|item| (*item, self.sub))
    }

    /// Level on PIA1 PA0 (high when stopped or silent).
    pub fn input_level(&self) -> bool {
        match self.current() {
            Some((item, sub)) => item.segment(sub).0,
            None => true,
        }
    }

    /// Level for the MUX cassette audio input (0.5 = silence).
    pub fn audio_level(&self) -> f32 {
        match self.current() {
            Some((item @ TapeItem::Byte { .. }, sub)) => {
                if item.segment(sub).0 {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.5,
        }
    }

    fn advance(&mut self, cycles: u32, clock_hz: u32) {
        if !self.loaded {
            return;
        }
        if self.program.is_none() {
            self.program = Some(build_program(&self.image));
        }
        let Some(program) = self.program.as_ref() else {
            return;
        };
        if self.unit_clock != clock_hz {
            if self.unit_clock != 0 {
                let scaled = self.unit_acc as f64 * f64::from(clock_hz) / f64::from(self.unit_clock);
                self.unit_acc = (scaled as u64).min(u64::from(clock_hz) - 1);
            }
            self.unit_clock = clock_hz;
        }
        let clock = u64::from(clock_hz);
        self.unit_acc += u64::from(cycles) * UNITS_PER_SECOND;
        while self.unit_acc >= clock {
            self.unit_acc -= clock;
            let Some(item) = program.get(self.item) else {
                // End of tape.
                self.unit_acc = 0;
                self.position = self.image.len();
                break;
            };
            if self.units_left == 0 {
                self.units_left = item.segment(self.sub).1;
            }
            self.units_left -= 1;
            if self.units_left > 0 {
                continue;
            }
            self.sub += 1;
            if self.sub >= item.segments() {
                self.sub = 0;
                self.item += 1;
            }
            match program.get(self.item) {
                Some(next) => {
                    self.units_left = next.segment(self.sub).1;
                    self.position = next.pos();
                }
                None => self.position = self.image.len(),
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const CLOCK: u32 = 894_886;

    /// Build one block: sync, type, length, data, checksum.
    pub(crate) fn block(block_type: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![SYNC, block_type, data.len() as u8];
        out.extend_from_slice(data);
        let sum = data
            .iter()
            .fold(block_type.wrapping_add(data.len() as u8), |s, &b| s.wrapping_add(b));
        out.push(sum);
        out
    }

    /// A Color BASIC tape: leader, filename block, leader, data blocks, EOF.
    pub(crate) fn basic_tape(name: &str, program: &[u8], exec: u16, load: u16) -> Vec<u8> {
        let mut header = [b' '; 8].to_vec();
        for (i, b) in name.bytes().take(8).enumerate() {
            header[i] = b;
        }
        header.push(0x00); // file type: BASIC program
        header.push(0x00); // binary
        header.push(0x00); // no gaps
        header.extend_from_slice(&exec.to_be_bytes());
        header.extend_from_slice(&load.to_be_bytes());
        let mut tape = vec![LEADER; LEADER_BYTES];
        tape.extend(block(BLOCK_FILENAME, &header));
        tape.push(LEADER);
        tape.extend(vec![LEADER; LEADER_BYTES]);
        for chunk in program.chunks(255) {
            tape.push(LEADER);
            tape.extend(block(0x01, chunk));
            tape.push(LEADER);
        }
        tape.push(LEADER);
        tape.extend(block(BLOCK_EOF, &[]));
        tape.push(LEADER);
        tape
    }

    /// Run the deck with the motor on and record PA0 edges (cycle stamps).
    fn capture(cas: &mut Cassette, cycles: u64, step: u32) -> Vec<(u64, bool)> {
        let mut t = 0u64;
        let mut last = cas.input_level();
        let mut edges = vec![(0, last)];
        while t < cycles {
            cas.tick(step, CLOCK, true, 0);
            t += u64::from(step);
            let level = cas.input_level();
            if level != last {
                edges.push((t, level));
                last = level;
            }
        }
        edges
    }

    /// Decode a PA0 edge list the way the ROM does: full cycles measured
    /// between rising edges, short = 1, bits LSB first, byte-aligned on $3C.
    fn decode_edges(edges: &[(u64, bool)]) -> Vec<u8> {
        let rises: Vec<u64> = edges.iter().filter(|e| e.1).map(|e| e.0).collect();
        let split = f64::from(CLOCK) / 1800.0;
        let mut bits = Vec::new();
        for pair in rises.windows(2) {
            let period = (pair[1] - pair[0]) as f64;
            if period > f64::from(CLOCK) / 600.0 {
                bits.push(None); // gap
            } else {
                bits.push(Some(period < split));
            }
        }
        let mut out = Vec::new();
        let mut window = 0u8;
        let mut synced = false;
        let mut byte = 0u8;
        let mut n = 0;
        for bit in bits {
            let Some(bit) = bit else {
                synced = false;
                continue;
            };
            let b = u8::from(bit);
            if !synced {
                window = (window >> 1) | (b << 7);
                if window == SYNC {
                    synced = true;
                    out.push(SYNC);
                    byte = 0;
                    n = 0;
                }
                continue;
            }
            byte = (byte >> 1) | (b << 7);
            n += 1;
            if n == 8 {
                out.push(byte);
                n = 0;
            }
        }
        out
    }

    #[test]
    fn byte_at_reads_unaligned_lsb_first_bytes() {
        let img = [0x3C, 0x00];
        assert_eq!(byte_at(&img, 0), Some(0x3C));
        // 0x3C << 3 spread over two bytes
        let shifted = [0x3C << 3, 0x3C >> 5];
        assert_eq!(byte_at(&shifted, 3), Some(0x3C));
        assert_eq!(byte_at(&img, 9), None);
    }

    #[test]
    fn program_inserts_gaps_and_leaders_like_a_real_recording() {
        // Short leaders only, as found in many .CAS files.
        let mut tape = vec![LEADER];
        tape.extend(block(BLOCK_FILENAME, &[1, 2, 3]));
        tape.push(LEADER);
        tape.extend(block(0x01, &[9, 8, 7]));
        tape.extend(block(0x01, &[6]));
        tape.extend(block(BLOCK_EOF, &[]));
        let program = build_program(&tape);
        // Silence + 128 leader before the filename block ...
        assert!(matches!(program[0], TapeItem::Silence { units: GAP_UNITS, .. }));
        assert!(program[1..=LEADER_BYTES]
            .iter()
            .all(|i| matches!(i, TapeItem::Byte { value: LEADER, .. })));
        let silences = program
            .iter()
            .filter(|i| matches!(i, TapeItem::Silence { .. }))
            .count();
        // ... and before the first data block; none between data blocks.
        assert_eq!(silences, 2);
        let bytes: Vec<u8> = program
            .iter()
            .filter_map(|i| match i {
                TapeItem::Byte { value, .. } => Some(*value),
                _ => None,
            })
            .collect();
        let syncs = bytes.iter().filter(|&&b| b == SYNC).count();
        assert_eq!(syncs, 4);
        // Positions never run backwards and stay inside the image.
        let mut last = 0;
        for item in &program {
            assert!(item.pos() >= last && item.pos() <= tape.len());
            last = item.pos();
        }
    }

    #[test]
    fn program_finds_bit_shifted_blocks_and_keeps_undecodable_tails() {
        let mut aligned = vec![LEADER; 4];
        aligned.extend(block(BLOCK_FILENAME, &[0x41, 0x42]));
        // Shift the whole stream by 3 bits.
        let mut shifted = vec![0u8; aligned.len() + 1];
        for (i, &b) in aligned.iter().enumerate() {
            shifted[i] |= b << 3;
            shifted[i + 1] |= b >> 5;
        }
        let program = build_program(&shifted);
        let bytes: Vec<u8> = program
            .iter()
            .filter_map(|i| match i {
                TapeItem::Byte { value, .. } => Some(*value),
                _ => None,
            })
            .collect();
        let at = bytes.iter().position(|&b| b == SYNC).expect("sync found");
        let expected = block(BLOCK_FILENAME, &[0x41, 0x42]);
        assert_eq!(&bytes[at..at + expected.len()], &expected[..]);

        // A corrupt checksum stops block parsing; the bytes are still played.
        let mut bad = vec![LEADER; 2];
        bad.extend([SYNC, 0x01, 1, 0x10, 0x99]);
        let program = build_program(&bad);
        let bytes: Vec<u8> = program
            .iter()
            .filter_map(|i| match i {
                TapeItem::Byte { value, .. } => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(bytes, bad);
        assert!(matches!(program[0], TapeItem::Silence { .. }));
    }

    #[test]
    fn playback_waveform_has_fsk_timing_and_round_trips() {
        let tape = basic_tape("HELLO", &[0x12, 0x34, 0x56, 0x78, 0x9A], 0, 0);
        let mut cas = Cassette::default();
        cas.insert("hello.cas".into(), tape.clone());
        cas.tick(1, CLOCK, true, 0); // close the relay
        let seconds = 8;
        let edges = capture(&mut cas, u64::from(CLOCK) * seconds, 5);
        // Rising-to-rising periods are 1/2400 s or 1/1200 s (within one step).
        let rises: Vec<u64> = edges.iter().filter(|e| e.1).map(|e| e.0).collect();
        let one = f64::from(CLOCK) / 2400.0;
        let zero = f64::from(CLOCK) / 1200.0;
        let mut ones = 0;
        let mut zeros = 0;
        for pair in rises.windows(2) {
            let p = (pair[1] - pair[0]) as f64;
            if (p - one).abs() <= 6.0 {
                ones += 1;
            } else if (p - zero).abs() <= 6.0 {
                zeros += 1;
            } else {
                assert!(p > f64::from(CLOCK) / 10.0, "unexpected period {p}");
            }
        }
        assert!(ones > 1000 && zeros > 1000, "ones={ones} zeros={zeros}");
        // Decoding the waveform like the ROM gives back every block.
        let decoded = decode_edges(&edges);
        assert_eq!(blocks_of(&decoded), blocks_of(&tape));
        assert_eq!(blocks_of(&tape).len(), 3);
        assert_eq!(cas.position(), tape.len(), "tape ran to the end");
    }

    /// Type/length/data/checksum of every block in an image.
    fn blocks_of(img: &[u8]) -> Vec<Vec<u8>> {
        let mut from = 0;
        let mut v = Vec::new();
        while let Some(b) = find_block(img, from) {
            v.push(b.bytes);
            from = b.end_bit;
        }
        v
    }

    #[test]
    fn tape_only_moves_with_the_motor_and_rewinds() {
        let tape = basic_tape("A", &[1, 2, 3], 0, 0);
        let mut cas = Cassette::default();
        cas.insert("a.cas".into(), tape.clone());
        // Motor off: nothing moves, PA0 idles high.
        for _ in 0..100_000 {
            cas.tick(9, CLOCK, false, 0);
        }
        assert_eq!(cas.position(), 0);
        assert!(cas.input_level());
        // Motor on for 3 s: past the gap into the leader.
        for _ in 0..(3 * CLOCK / 9) {
            cas.tick(9, CLOCK, true, 0);
        }
        let pos = cas.position();
        assert!(pos > 0, "tape advanced");
        // Stop: the position holds.
        cas.tick(9, CLOCK, false, 0);
        for _ in 0..10_000 {
            cas.tick(9, CLOCK, false, 0);
        }
        assert_eq!(cas.position(), pos);
        assert!(!cas.motor());
        cas.rewind();
        assert_eq!(cas.position(), 0);
        cas.eject();
        assert!(!cas.loaded());
        assert_eq!(cas.length(), 0);
        assert!(cas.input_level());
        // Empty or absurdly large images are refused.
        cas.insert("empty.cas".into(), Vec::new());
        assert!(!cas.loaded());
        cas.insert("huge.cas".into(), vec![0x55; MAX_TAPE_BYTES + 1]);
        assert!(!cas.loaded() && cas.name().is_none());
    }

    #[test]
    fn playback_speed_follows_real_time_at_any_cpu_clock() {
        let tape = vec![0x00; 64]; // undecodable: played verbatim after 1 s
        let run = |clock: u32| {
            let mut cas = Cassette::default();
            cas.insert("z.cas".into(), tape.clone());
            cas.tick(1, clock, true, 0);
            // 1.5 s of wall time.
            let total = u64::from(clock) * 3 / 2;
            let mut t = 0;
            while t < total {
                cas.tick(4, clock, true, 0);
                t += 4;
            }
            cas.position()
        };
        let normal = run(CLOCK);
        let fast = run(CLOCK * 2);
        // 0.5 s of zero bits at 1200 bit/s = 75 bytes (capped by the image).
        assert!(normal > 50, "normal={normal}");
        assert!((normal as i64 - fast as i64).abs() <= 1, "normal={normal} fast={fast}");
    }

    /// Synthesize the ROM's CSAVE waveform (36-entry sine table, one entry
    /// every ~22 cycles, every other entry for a `1`).
    pub(crate) fn rom_waveform(bytes: &[u8]) -> Vec<(u32, u8)> {
        const SINE: [u8; 36] = [
            32, 36, 42, 46, 50, 54, 58, 60, 62, 62, 62, 60, 58, 54, 50, 46, 42, 36, 30, 26, 20,
            16, 12, 8, 4, 2, 0, 0, 0, 2, 4, 8, 12, 16, 20, 26,
        ];
        let mut out = Vec::new();
        for &b in bytes {
            for bit in 0..8 {
                let one = (b >> bit) & 1 != 0;
                let step = if one { 2 } else { 1 };
                let mut i = 0;
                while i < SINE.len() {
                    out.push((22 + u32::from(one), SINE[i]));
                    i += step;
                }
            }
        }
        out
    }

    fn feed(rec_cas: &mut Cassette, wave: &[(u32, u8)], clock: u32) {
        for &(cycles, dac) in wave {
            rec_cas.tick(cycles, clock, true, dac);
        }
    }

    #[test]
    fn recorder_turns_csave_waveform_into_cas_blocks() {
        let mut cas = Cassette::default();
        // Header: relay on, silence (flat DAC) for 0.5 s, leader, block.
        cas.tick(10, CLOCK, true, 32);
        for _ in 0..(CLOCK / 2 / 10) {
            cas.tick(10, CLOCK, true, 32);
        }
        let mut header_stream = vec![LEADER; LEADER_BYTES];
        let header = block(BLOCK_FILENAME, b"PROG    \x00\x00\x00\x00\x00\x00\x00");
        header_stream.push(LEADER);
        header_stream.extend(&header);
        header_stream.push(LEADER);
        feed(&mut cas, &rom_waveform(&header_stream), CLOCK);
        cas.tick(20, CLOCK, false, 32); // relay opens
        // Data: relay on again, silence, leader, two blocks back to back, EOF.
        cas.tick(10, CLOCK, true, 32);
        for _ in 0..(CLOCK / 2 / 10) {
            cas.tick(10, CLOCK, true, 32);
        }
        let d1 = block(0x01, &[0x11; 255]);
        let d2 = block(0x01, &[0x00, 0xFF, 0x3C, 0x55]);
        let eof = block(BLOCK_EOF, &[]);
        let mut data_stream = vec![LEADER; LEADER_BYTES];
        for b in [&d1, &d2, &eof] {
            data_stream.push(LEADER);
            data_stream.extend(b.iter());
            data_stream.push(LEADER);
        }
        feed(&mut cas, &rom_waveform(&data_stream), CLOCK);
        cas.tick(20, CLOCK, false, 32);

        let rec = cas.take_recording();
        assert_eq!(cas.recorded_bytes(), 0, "take clears the recording");
        // Every block is there, in order, byte-exact.
        let mut from = 0;
        let mut found = Vec::new();
        while let Some(b) = find_block(&rec, from) {
            assert_eq!(b.sync_bit % 8, 0, "byte aligned");
            let mut full = vec![SYNC];
            full.extend(&b.bytes);
            found.push(full);
            from = b.end_bit;
        }
        assert_eq!(found, vec![header, d1, d2, eof]);
        // Leaders are kept: ~128 bytes before the filename and data blocks.
        let lead = rec.iter().take_while(|&&b| b == LEADER).count();
        assert!((127..=130).contains(&lead), "leader {lead}");
        assert_eq!(rec.last(), Some(&LEADER), "trailing leader byte");
    }

    #[test]
    fn recorder_is_independent_of_the_cpu_clock() {
        // The ROM's writer is cycle-timed: with the SAM speed-up the CPU runs
        // twice as fast, so the waveform is twice as fast in real time
        // (E cycles fed at twice the clock). The adaptive split decodes it.
        let mut cas = Cassette::default();
        let stream = [&vec![LEADER; 16][..], &block(0x01, &[1, 2, 3, 250])[..], &[LEADER]].concat();
        cas.tick(10, CLOCK * 2, true, 32);
        feed(&mut cas, &rom_waveform(&stream), CLOCK * 2);
        cas.tick(20, CLOCK * 2, false, 32);
        // Normal speed in the board's tick units (2 ticks per E cycle).
        let mut slow = Cassette::default();
        let doubled: Vec<(u32, u8)> = rom_waveform(&stream).iter().map(|&(c, d)| (c * 2, d)).collect();
        slow.tick(10, CLOCK * 2, true, 32);
        feed(&mut slow, &doubled, CLOCK * 2);
        slow.tick(20, CLOCK * 2, false, 32);
        assert_eq!(blocks_of(&slow.take_recording()).len(), 1);
        let rec = cas.take_recording();
        // type, length, data, checksum (1 + 4 + 1 + 2 + 3 + 250 = 261 → $05)
        assert_eq!(blocks_of(&rec), vec![vec![0x01, 4, 1, 2, 3, 250, 0x05]]);
    }

    #[test]
    fn recorder_starts_with_the_1500_hz_split() {
        assert_eq!(Recorder::default().split, REC_SPLIT_S);
        let old: Recorder = serde_json::from_str("{}").unwrap();
        assert_eq!(old.split, REC_SPLIT_S);
        // A lone sync byte right after the relay closes (no leader to adapt on).
        let mut cas = Cassette::default();
        cas.tick(10, CLOCK, true, 32);
        feed(&mut cas, &rom_waveform(&block(0x01, &[7])), CLOCK);
        cas.tick(20, CLOCK, false, 32);
        assert_eq!(blocks_of(&cas.take_recording()), vec![vec![0x01, 1, 7, 9]]);
    }

    #[test]
    fn recorded_tape_plays_back_identically() {
        let mut cas = Cassette::default();
        let original = basic_tape("RT", &(0..40u8).collect::<Vec<_>>(), 0, 0);
        cas.tick(10, CLOCK, true, 32);
        feed(&mut cas, &rom_waveform(&original), CLOCK);
        cas.tick(20, CLOCK, false, 32);
        let rec = cas.take_recording();
        assert_eq!(blocks_of(&rec), blocks_of(&original));
        assert_eq!(blocks_of(&rec).len(), 3);

        // And the recording plays back into the same blocks.
        let mut deck = Cassette::default();
        deck.insert("rec.cas".into(), rec);
        deck.tick(1, CLOCK, true, 0);
        let edges = capture(&mut deck, u64::from(CLOCK) * 6, 6);
        assert_eq!(blocks_of(&decode_edges(&edges)), blocks_of(&original));
    }

    #[test]
    fn serde_keeps_tape_and_position_and_rebuilds_the_program() {
        let tape = basic_tape("S", &[5, 6, 7], 0, 0);
        let mut cas = Cassette::default();
        cas.insert("s.cas".into(), tape.clone());
        for _ in 0..(2 * CLOCK / 7) {
            cas.tick(7, CLOCK, true, 0);
        }
        let json = serde_json::to_string(&cas).unwrap();
        assert!(json.contains("\"image\":\""), "image stored as hex text");
        let mut back: Cassette = serde_json::from_str(&json).unwrap();
        assert_eq!(back.position(), cas.position());
        assert_eq!(back.length(), tape.len());
        assert_eq!(back.name(), Some("s.cas".into()));
        // Both decks continue identically.
        for _ in 0..50_000 {
            cas.tick(7, CLOCK, true, 0);
            back.tick(7, CLOCK, true, 0);
            assert_eq!(cas.input_level(), back.input_level());
        }
        let old: Cassette = serde_json::from_str("{}").unwrap();
        assert!(!old.loaded());
    }
}
