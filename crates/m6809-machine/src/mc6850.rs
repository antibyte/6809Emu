//! Motorola MC6850 ACIA plus the host side of the serial line (terminal).
//!
//! Chip model (MC6850 datasheet, cross-checked with MAME `6850acia.cpp`):
//! * Control: CR1:CR0 = counter divide (/1, /16, /64, 11 = master reset);
//!   CR4:CR2 = word select (7E2 7O2 7E1 7O1 8N2 8N1 8E1 8O1, i.e. frames of
//!   10 or 11 bits including the start bit); CR6:CR5 = RTS / TX interrupt
//!   enable / break; CR7 = RX interrupt enable. RTS and break are latched but
//!   have no effect on the modelled line (host pacing: see `strict_rx`).
//! * Master reset (also the power-on state) clears the status register (TDRE
//!   reads 0), idles transmitter and receiver and releases /IRQ; CR2-CR7 are
//!   still latched. Writing a non-reset divide releases it and TDRE reads 1.
//! * /IRQ is a level: `RIE & (RDRF | OVRN) | TIE & TDRE` (no DCD/CTS inputs
//!   are modelled: carrier present, clear to send). Reading RDR releases it.
//! * Transmitter: TDR + shift register. Data written to the idle transmitter
//!   moves into the shift register one bit time later (TDRE returns to 1);
//!   a write while TDRE = 0 overwrites TDR (the earlier byte is lost). A
//!   character is complete after its frame time and then goes to the
//!   terminal (7-bit modes send `byte & $7F`).
//! * Receiver: a single RDR. Reading RDR while RDRF = 0 returns the last
//!   received byte; in 7-bit modes bit 7 reads 0. Overrun (strict mode): a
//!   character completing while RDRF is set is lost; OVRN appears once the
//!   valid character has been read (RDRF stays set) and is reset by the next
//!   RDR read, i.e. the usual "read status, then read data" sequence.
//!   Parity and framing errors are never raised for host input.
//! * Reads of the status register and [`Acia6850::peek`] have no side effects.

use std::cell::RefCell;
use std::collections::VecDeque;

use m6809_core::IoRegisterView;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_ADDR: u16 = 0xFFA0;
pub const DEFAULT_BAUD: u32 = 9600;
pub const DEFAULT_E_CLOCK_HZ: u32 = 1_000_000;
/// Transmitted bytes kept for the terminal (ring buffer: the oldest byte is
/// dropped in O(1) once full).
pub const TX_HISTORY_CAP: usize = 8192;
/// Host input (typed / pasted) waiting to be sent; excess input is dropped.
pub const RX_FIFO_CAP: usize = 1 << 20;

const SR_RDRF: u8 = 0x01;
const SR_TDRE: u8 = 0x02;
const SR_OVRN: u8 = 0x20;
const SR_IRQ: u8 = 0x80;

const CR_DIVIDE_MASK: u8 = 0x03;
/// CR1:CR0 = 11 is master reset (not CR7:CR6).
const CR_MASTER_RESET: u8 = 0x03;
/// CR7 = receive interrupt enable.
const CR_RIE: u8 = 0x80;
/// CR6:CR5 = 01 -> RTS low, transmit interrupt enable.
const CR_TX_CTRL_MASK: u8 = 0x60;
const CR_TX_IRQ: u8 = 0x20;
/// Frame length (start + data + parity + stop bits) per CR4:CR2 word select.
const FRAME_BITS: [u64; 8] = [11, 11, 10, 10, 11, 10, 11, 11];

/// MC6850 ACIA configuration.
///
/// Timing: [`Acia6850::tick`] is fed CPU E cycles running at `e_clock_hz`.
/// The TX/RX clock inputs are modelled as `16 * baud` Hz, so `baud` is the
/// line rate with the usual /16 counter divide (CR1:CR0 = 01); /1 runs 16x
/// faster and /64 4x slower. One bit lasts `divide * e_clock_hz / (16 * baud)`
/// E cycles; frame times are kept in an exact integer time base (units of
/// `1 / (16 * baud)` E cycle), so fractional bit times never drift.
/// Grant Searle SBC (MsBasic): ACIA clock = E = 1.8432 MHz, `baud` 115200,
/// control $15 -> /16 -> 16 E cycles per bit, 160 per 8N1 character.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AciaConfig {
    pub enabled: bool,
    pub base_addr: u16,
    /// Line rate at /16; defines the TX/RX clock (16 x baud).
    pub baud: u32,
    /// Rate of the cycles passed to `tick` (CPU E clock).
    pub e_clock_hz: u32,
    /// Register order. `true`: Motorola RS = A0 map (status/control at
    /// `base`, data at `base+1`, e.g. Grant Searle SBC). `false` (default,
    /// the emulator's legacy map): data at `base`, status/control at `base+1`.
    #[serde(default)]
    pub motorola: bool,
    /// Receive pacing.
    /// * `false` (default, lenient): host input waits in a FIFO and the next
    ///   character starts arriving (one frame time) only after the previous
    ///   one was read from RDR, like hardware flow control (Grant Searle's
    ///   /IRQ -> /RTS modification). Pasted text is never lost.
    /// * `true` (strict): the host sends back-to-back, one character per
    ///   frame time. A character that completes while RDRF is still set is
    ///   lost and raises OVRN (datasheet overrun semantics).
    ///
    /// In both modes the host holds its input while the ACIA is in master reset.
    #[serde(default)]
    pub strict_rx: bool,
}

impl Default for AciaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_addr: DEFAULT_BASE_ADDR,
            baud: DEFAULT_BAUD,
            e_clock_hz: DEFAULT_E_CLOCK_HZ,
            motorola: false,
            strict_rx: false,
        }
    }
}

impl AciaConfig {
    fn units_per_cycle(&self) -> u64 {
        16 * u64::from(self.baud.max(1))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AciaTerminalDto {
    /// Terminal screen text: BS erases, CR returns to column 0, LF starts a
    /// new line, TAB advances to the next multiple of 8, FF clears; other
    /// control codes are ignored; bytes >= $80 decode as Latin-1.
    pub tx_text: String,
    pub rdrf: bool,
    pub tdre: bool,
    pub irq: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TxPhase {
    Idle,
    /// Data was written to the idle transmitter; TDR moves into the shift
    /// register when `remaining` (one bit time) has elapsed.
    Loading { remaining: u64 },
    /// A character (already masked to the word length) is on the line.
    Shifting { byte: u8, remaining: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct RxFrame {
    byte: u8,
    remaining: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct AciaState {
    /// Last value written to the control register (CR1:CR0 = 11: in reset).
    control: u8,
    rdr: u8,
    rdrf: bool,
    ovrn: bool,
    /// A character was lost; OVRN shows after the next RDR read.
    overrun_pending: bool,
    tdr: u8,
    /// Internal TDRE; reads as 0 while in master reset.
    tdre: bool,
    tx: TxPhase,
    /// Character currently arriving on the RX line.
    rx: Option<RxFrame>,
    /// Host input not yet sent.
    host_fifo: VecDeque<u8>,
    /// Raw transmitted bytes (terminal history).
    tx_history: VecDeque<u8>,
    #[serde(skip)]
    text_cache: Option<String>,
}

impl Default for AciaState {
    /// Power-on: the chip is held in master reset until the first control
    /// write with a non-reset divide.
    fn default() -> Self {
        Self {
            control: CR_MASTER_RESET,
            rdr: 0,
            rdrf: false,
            ovrn: false,
            overrun_pending: false,
            tdr: 0,
            tdre: true,
            tx: TxPhase::Idle,
            rx: None,
            host_fifo: VecDeque::new(),
            tx_history: VecDeque::new(),
            text_cache: None,
        }
    }
}

#[derive(Debug)]
pub struct Acia6850 {
    config: AciaConfig,
    state: RefCell<AciaState>,
}

#[derive(Serialize, Deserialize)]
struct AciaSnapshot {
    config: AciaConfig,
    state: AciaState,
}

impl Serialize for Acia6850 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AciaSnapshot {
            config: self.config,
            state: self.state.borrow().clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Acia6850 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let snap = AciaSnapshot::deserialize(deserializer)?;
        Ok(Self {
            config: snap.config,
            state: RefCell::new(snap.state),
        })
    }
}

impl Clone for Acia6850 {
    fn clone(&self) -> Self {
        Self {
            config: self.config,
            state: RefCell::new(self.state.borrow().clone()),
        }
    }
}

impl Default for Acia6850 {
    fn default() -> Self {
        Self::new(AciaConfig::default())
    }
}

impl Acia6850 {
    /// Power-on state: held in master reset (status reads $00) until the
    /// program writes a control word with a non-reset divide.
    pub fn new(config: AciaConfig) -> Self {
        Self {
            config,
            state: RefCell::new(AciaState::default()),
        }
    }

    pub fn config(&self) -> AciaConfig {
        self.config
    }

    pub fn set_config(&mut self, config: AciaConfig) {
        let old = self.config;
        self.config = config;
        if config.enabled && !old.enabled {
            // Power-on of a newly enabled chip (fresh terminal).
            *self.state.get_mut() = AciaState::default();
            return;
        }
        let (old_upc, new_upc) = (old.units_per_cycle(), config.units_per_cycle());
        if old_upc != new_upc {
            // Keep the time left on characters in flight (in E cycles).
            let rescale = |units: u64| {
                (u128::from(units) * u128::from(new_upc) / u128::from(old_upc)).max(1) as u64
            };
            let state = self.state.get_mut();
            state.tx = match state.tx {
                TxPhase::Idle => TxPhase::Idle,
                TxPhase::Loading { remaining } => TxPhase::Loading {
                    remaining: rescale(remaining),
                },
                TxPhase::Shifting { byte, remaining } => TxPhase::Shifting {
                    byte,
                    remaining: rescale(remaining),
                },
            };
            if let Some(frame) = state.rx.as_mut() {
                frame.remaining = rescale(frame.remaining);
            }
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn handles(&self, addr: u16) -> bool {
        self.config.enabled
            && (addr == self.config.base_addr || addr == self.config.base_addr.wrapping_add(1))
    }

    fn data_addr(&self) -> u16 {
        if self.config.motorola {
            self.config.base_addr.wrapping_add(1)
        } else {
            self.config.base_addr
        }
    }

    fn status_addr(&self) -> u16 {
        if self.config.motorola {
            self.config.base_addr
        } else {
            self.config.base_addr.wrapping_add(1)
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        if !self.handles(addr) {
            return 0xFF;
        }
        if addr == self.data_addr() {
            return self.read_data();
        }
        status_byte(&self.state.borrow())
    }

    /// Side-effect-free register view for the debugger: status, or the
    /// current RDR contents (nothing is consumed, no flag changes).
    pub fn peek(&self, addr: u16) -> u8 {
        if !self.handles(addr) {
            return 0xFF;
        }
        let state = self.state.borrow();
        if addr == self.data_addr() {
            state.rdr
        } else {
            status_byte(&state)
        }
    }

    /// Machine RESET. The MC6850 has no reset pin: a CPU reset leaves the
    /// ACIA (and the terminal and host input) untouched; the firmware puts
    /// it into a known state with a master reset / control write.
    pub fn reset(&mut self) {}

    pub fn write(&self, addr: u16, value: u8) {
        if !self.handles(addr) {
            return;
        }
        if addr == self.data_addr() {
            self.write_data(value);
        } else {
            self.write_control(value);
        }
    }

    /// Advance transmitter and receiver by `cycles` E cycles.
    pub fn tick(&self, cycles: u32) {
        if !self.config.enabled || cycles == 0 {
            return;
        }
        let mut state = self.state.borrow_mut();
        if in_reset(&state) {
            return;
        }
        let budget = u64::from(cycles) * self.config.units_per_cycle();
        self.advance_tx(&mut state, budget);
        self.advance_rx(&mut state, budget);
    }

    /// Current level of the /IRQ output (no latching, polling has no effect).
    pub fn poll_irq(&self) -> bool {
        self.config.enabled && irq_level(&self.state.borrow())
    }

    /// Queue raw bytes typed / pasted on the host terminal.
    pub fn enqueue_rx(&self, bytes: &[u8]) {
        if !self.config.enabled {
            return;
        }
        let mut state = self.state.borrow_mut();
        let room = RX_FIFO_CAP.saturating_sub(state.host_fifo.len());
        state.host_fifo.extend(bytes.iter().take(room));
        self.start_rx(&mut state);
    }

    /// Queue host text encoded as Latin-1 (the terminal's character set);
    /// characters above U+00FF are sent as `?`.
    pub fn enqueue_rx_text(&self, text: &str) {
        let bytes: Vec<u8> = text
            .chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect();
        self.enqueue_rx(&bytes);
    }

    /// Host characters not yet delivered (queued plus the one on the line).
    pub fn rx_backlog(&self) -> usize {
        let state = self.state.borrow();
        state.host_fifo.len() + usize::from(state.rx.is_some())
    }

    pub fn terminal_state(&self) -> AciaTerminalDto {
        let mut state = self.state.borrow_mut();
        if state.text_cache.is_none() {
            let text = render_terminal(state.tx_history.iter().copied());
            state.text_cache = Some(text);
        }
        AciaTerminalDto {
            tx_text: state.text_cache.clone().unwrap_or_default(),
            rdrf: state.rdrf,
            tdre: visible_tdre(&state),
            irq: self.config.enabled && irq_level(&state),
        }
    }

    pub fn clear_terminal(&self) {
        let mut state = self.state.borrow_mut();
        state.tx_history.clear();
        state.text_cache = None;
    }

    pub fn io_registers(&self) -> Vec<IoRegisterView> {
        if !self.config.enabled {
            return Vec::new();
        }
        let state = self.state.borrow();
        vec![
            IoRegisterView {
                address: self.data_addr(),
                name: "ACIA Data (RDR)".into(),
                value: state.rdr,
            },
            IoRegisterView {
                address: self.status_addr(),
                name: format!("ACIA Status (CR ${:02X})", state.control),
                value: status_byte(&state),
            },
        ]
    }

    fn read_data(&self) -> u8 {
        let mut state = self.state.borrow_mut();
        if state.overrun_pending {
            // The valid character is read now; OVRN becomes visible and
            // RDRF stays set until the overrun is reset by the next read.
            state.ovrn = true;
            state.overrun_pending = false;
        } else {
            state.ovrn = false;
            state.rdrf = false;
        }
        let value = state.rdr;
        // Lenient pacing: the host sends its next character now.
        self.start_rx(&mut state);
        value
    }

    fn write_data(&self, value: u8) {
        let mut state = self.state.borrow_mut();
        if in_reset(&state) {
            return; // ignored while held in master reset
        }
        state.tdr = value;
        state.tdre = false;
        if state.tx == TxPhase::Idle {
            state.tx = TxPhase::Loading {
                remaining: units_per_bit(&self.config, state.control),
            };
        }
    }

    fn write_control(&self, value: u8) {
        let mut state = self.state.borrow_mut();
        let was_reset = in_reset(&state);
        state.control = value;
        if value & CR_DIVIDE_MASK == CR_MASTER_RESET {
            state.rdrf = false;
            state.ovrn = false;
            state.overrun_pending = false;
            state.tdre = true; // masked while in reset, reads 1 once released
            state.tx = TxPhase::Idle;
            if let Some(frame) = state.rx.take() {
                if !self.config.strict_rx {
                    state.host_fifo.push_front(frame.byte); // not lost
                }
            }
            return;
        }
        if was_reset {
            self.start_rx(&mut state);
        }
    }

    /// Put the next host character on the line if the receiver may take it.
    fn start_rx(&self, state: &mut AciaState) {
        if state.rx.is_some() || in_reset(state) {
            return;
        }
        if !self.config.strict_rx && (state.rdrf || state.overrun_pending) {
            return; // lenient: wait until RDR has been read
        }
        if let Some(byte) = state.host_fifo.pop_front() {
            state.rx = Some(RxFrame {
                byte,
                remaining: frame_units(&self.config, state.control),
            });
        }
    }

    fn advance_rx(&self, state: &mut AciaState, mut budget: u64) {
        loop {
            self.start_rx(state);
            let Some(mut frame) = state.rx else {
                return;
            };
            if budget < frame.remaining {
                frame.remaining -= budget;
                state.rx = Some(frame);
                return;
            }
            budget -= frame.remaining;
            state.rx = None;
            if state.rdrf {
                state.overrun_pending = true; // character lost
            } else {
                state.rdr = frame.byte & data_mask(state.control);
                state.rdrf = true;
            }
        }
    }

    fn advance_tx(&self, state: &mut AciaState, mut budget: u64) {
        loop {
            match state.tx {
                TxPhase::Idle => return,
                TxPhase::Loading { remaining } => {
                    if budget < remaining {
                        state.tx = TxPhase::Loading {
                            remaining: remaining - budget,
                        };
                        return;
                    }
                    budget -= remaining;
                    self.load_shift_register(state);
                }
                TxPhase::Shifting { byte, remaining } => {
                    if budget < remaining {
                        state.tx = TxPhase::Shifting {
                            byte,
                            remaining: remaining - budget,
                        };
                        return;
                    }
                    budget -= remaining;
                    push_tx_history(state, byte);
                    if state.tdre {
                        state.tx = TxPhase::Idle;
                    } else {
                        self.load_shift_register(state);
                    }
                }
            }
        }
    }

    /// TDR -> shift register: TDRE returns to 1, the frame starts.
    fn load_shift_register(&self, state: &mut AciaState) {
        state.tx = TxPhase::Shifting {
            byte: state.tdr & data_mask(state.control),
            remaining: frame_units(&self.config, state.control),
        };
        state.tdre = true;
    }
}

fn in_reset(state: &AciaState) -> bool {
    state.control & CR_DIVIDE_MASK == CR_MASTER_RESET
}

fn visible_tdre(state: &AciaState) -> bool {
    state.tdre && !in_reset(state)
}

fn irq_level(state: &AciaState) -> bool {
    if in_reset(state) {
        return false;
    }
    let rx = state.control & CR_RIE != 0 && (state.rdrf || state.ovrn);
    let tx = state.control & CR_TX_CTRL_MASK == CR_TX_IRQ && state.tdre;
    rx || tx
}

fn status_byte(state: &AciaState) -> u8 {
    let mut status = 0u8;
    if state.rdrf {
        status |= SR_RDRF;
    }
    if visible_tdre(state) {
        status |= SR_TDRE;
    }
    if state.ovrn {
        status |= SR_OVRN;
    }
    if irq_level(state) {
        status |= SR_IRQ;
    }
    status
}

fn divide(control: u8) -> u64 {
    match control & CR_DIVIDE_MASK {
        0 => 1,
        1 => 16,
        2 => 64,
        _ => 0,
    }
}

fn units_per_bit(config: &AciaConfig, control: u8) -> u64 {
    (divide(control) * u64::from(config.e_clock_hz.max(1))).max(1)
}

fn frame_units(config: &AciaConfig, control: u8) -> u64 {
    FRAME_BITS[usize::from((control >> 2) & 7)] * units_per_bit(config, control)
}

/// 7-bit word formats (CR4 = 0) carry 7 data bits.
fn data_mask(control: u8) -> u8 {
    if control & 0x10 == 0 {
        0x7F
    } else {
        0xFF
    }
}

fn push_tx_history(state: &mut AciaState, byte: u8) {
    if state.tx_history.len() >= TX_HISTORY_CAP {
        state.tx_history.pop_front();
    }
    state.tx_history.push_back(byte);
    state.text_cache = None;
}

/// Render transmitted bytes as terminal text (see [`AciaTerminalDto::tx_text`]).
fn render_terminal(bytes: impl IntoIterator<Item = u8>) -> String {
    let mut lines: Vec<Vec<char>> = vec![Vec::new()];
    let mut col = 0usize;
    for byte in bytes {
        match byte {
            b'\n' => {
                lines.push(Vec::new());
                col = 0;
            }
            b'\r' => col = 0,
            0x08 => {
                if col > 0 {
                    col -= 1;
                    let line = lines.last_mut().expect("at least one line");
                    if col + 1 == line.len() {
                        line.pop();
                    } else if col < line.len() {
                        line[col] = ' ';
                    }
                }
            }
            b'\t' => {
                let line = lines.last_mut().expect("at least one line");
                col = (col / 8 + 1) * 8;
                while line.len() < col {
                    line.push(' ');
                }
            }
            0x0C => {
                lines.clear();
                lines.push(Vec::new());
                col = 0;
            }
            0x00..=0x1F | 0x7F => {}
            _ => {
                let line = lines.last_mut().expect("at least one line");
                let ch = char::from(byte); // Latin-1
                if col < line.len() {
                    line[col] = ch;
                } else {
                    while line.len() < col {
                        line.push(' ');
                    }
                    line.push(ch);
                }
                col += 1;
            }
        }
    }
    let mut text = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.extend(line.iter());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u16 = 0xFFA0;
    // Legacy map: data at base, status/control at base+1.
    const DATA: u16 = 0xFFA0;
    const CTRL: u16 = 0xFFA1;

    fn acia_with(baud: u32, e_clock_hz: u32, strict_rx: bool) -> Acia6850 {
        Acia6850::new(AciaConfig {
            enabled: true,
            base_addr: BASE,
            baud,
            e_clock_hz,
            motorola: false,
            strict_rx,
        })
    }

    /// 8N1 at /16 with one E cycle per bit: 10-cycle frames.
    fn fast(strict_rx: bool) -> Acia6850 {
        let acia = acia_with(1_000_000, 1_000_000, strict_rx);
        acia.write(CTRL, 0x03);
        acia.write(CTRL, 0x15);
        acia
    }

    fn status(acia: &Acia6850) -> u8 {
        acia.read(CTRL)
    }

    fn text(acia: &Acia6850) -> String {
        acia.terminal_state().tx_text
    }

    /// Cycles until RDRF rises (ticking one cycle at a time).
    fn cycles_until_rdrf(acia: &Acia6850, limit: u32) -> Option<u32> {
        (1..=limit).find(|_| {
            acia.tick(1);
            status(acia) & SR_RDRF != 0
        })
    }

    #[test]
    fn power_on_holds_chip_in_master_reset_until_configured() {
        let acia = acia_with(1_000_000, 1_000_000, false);
        assert_eq!(status(&acia), 0x00, "TDRE reads 0 while held in reset");
        acia.write(DATA, b'X');
        acia.tick(100);
        assert_eq!(text(&acia), "", "data writes are ignored in reset");
        acia.enqueue_rx(b"A");
        acia.tick(100);
        assert_eq!(status(&acia) & SR_RDRF, 0, "receiver idle in reset");
        acia.write(CTRL, 0x15);
        assert_eq!(status(&acia), SR_TDRE);
        assert_eq!(cycles_until_rdrf(&acia, 100), Some(10), "host input was held");
        assert_eq!(acia.read(DATA), b'A');
    }

    #[test]
    fn master_reset_clears_status_latches_control_and_releases() {
        let acia = fast(false);
        acia.write(CTRL, 0x95); // RIE
        acia.enqueue_rx(b"QR");
        acia.tick(10);
        assert!(acia.poll_irq());
        acia.write(DATA, b'T');
        acia.write(CTRL, 0xE3); // master reset with CR2-CR7 = RIE | TX ctrl 11
        assert_eq!(status(&acia), 0x00, "status cleared, TDRE masked");
        assert!(!acia.poll_irq(), "no /IRQ while in reset");
        assert_eq!(acia.state.borrow().control, 0xE3, "CR2-CR7 still latched");
        acia.tick(1000);
        assert_eq!(text(&acia), "", "transmitter idle: the pending byte is gone");
        acia.write(CTRL, 0x15);
        assert_eq!(status(&acia), SR_TDRE, "release sets TDRE, RDRF stays clear");
        // Lenient mode keeps host input across the reset.
        assert_eq!(cycles_until_rdrf(&acia, 100), Some(10));
        assert_eq!(acia.read(DATA), b'R');
    }

    #[test]
    fn control_word_sets_frame_length_and_divide() {
        // baud = E: /16 -> 1 cycle per bit, /64 -> 4, /1 -> 1/16.
        for (control, cycles) in [
            (0x15u8, 10u32), // 8N1 /16
            (0x11, 11),      // 8N2 /16
            (0x19, 11),      // 8E1 /16
            (0x1D, 11),      // 8O1 /16
            (0x09, 10),      // 7E1 /16
            (0x0D, 10),      // 7O1 /16
            (0x01, 11),      // 7E2 /16
            (0x05, 11),      // 7O2 /16
            (0x16, 40),      // 8N1 /64
            (0x14, 1),       // 8N1 /1: 10/16 cycle
        ] {
            let acia = acia_with(1_000_000, 1_000_000, false);
            acia.write(CTRL, control);
            acia.enqueue_rx(b"U");
            assert_eq!(
                cycles_until_rdrf(&acia, 1000),
                Some(cycles),
                "control ${control:02X}"
            );
        }
    }

    #[test]
    fn ms_basic_profile_is_115200_baud_at_1_8432_mhz() {
        let acia = Acia6850::new(AciaConfig {
            enabled: true,
            base_addr: 0xA000,
            baud: 115_200,
            e_clock_hz: 1_843_200,
            motorola: true,
            strict_rx: false,
        });
        acia.write(0xA000, 0x15);
        acia.enqueue_rx(b"P");
        let mut cycles = 0;
        while acia.read(0xA000) & SR_RDRF == 0 {
            acia.tick(1);
            cycles += 1;
        }
        assert_eq!(cycles, 160, "16 E cycles per bit, 10-bit frame");
        assert_eq!(acia.read(0xA001), b'P');
    }

    #[test]
    fn fractional_bit_time_does_not_drift() {
        // 9600 baud at 1 MHz: 104.1667 cycles per bit, 1041.667 per frame.
        let acia = acia_with(9_600, 1_000_000, true);
        acia.write(CTRL, 0x15);
        acia.enqueue_rx(&[b'x'; 96]);
        let mut arrivals = Vec::new();
        for cycle in 1..=100_000u32 {
            acia.tick(1);
            if status(&acia) & SR_RDRF != 0 {
                arrivals.push(cycle);
                acia.read(DATA);
            }
        }
        assert_eq!(arrivals.len(), 96);
        assert_eq!(arrivals[0], 1042);
        assert_eq!(arrivals[2], 3125);
        assert_eq!(arrivals[95], 100_000, "96 frames = exactly 100,000 cycles");
    }

    #[test]
    fn rx_irq_is_a_level_released_by_reading_rdr() {
        let acia = fast(false);
        acia.write(CTRL, 0x95); // RIE, 8N1, /16
        acia.enqueue_rx(b"A");
        acia.tick(9);
        assert!(!acia.poll_irq());
        acia.tick(1);
        for _ in 0..3 {
            assert!(acia.poll_irq(), "polling must not clear the level");
        }
        assert_eq!(status(&acia), SR_IRQ | SR_TDRE | SR_RDRF);
        assert!(acia.poll_irq(), "status read does not release /IRQ");
        assert_eq!(acia.read(DATA), b'A');
        assert!(!acia.poll_irq());
        assert_eq!(status(&acia) & SR_IRQ, 0);
    }

    #[test]
    fn tx_irq_follows_tdre() {
        let acia = fast(false);
        acia.write(CTRL, 0x35); // TIE (CR6:CR5 = 01), 8N1, /16
        assert!(acia.poll_irq(), "TDRE with TIE asserts /IRQ");
        acia.write(DATA, b'Z');
        assert!(!acia.poll_irq(), "writing TDR releases /IRQ");
        acia.tick(1); // one bit time: TDR -> shift register
        assert!(acia.poll_irq());
        acia.write(CTRL, 0x15);
        assert!(!acia.poll_irq(), "TIE off");
    }

    #[test]
    fn control_15_does_not_enable_interrupts() {
        let acia = fast(false);
        acia.tick(1);
        assert!(!acia.poll_irq(), "Grant Searle $15 is polled 8N1, no IRQ");
        acia.enqueue_rx(b"A");
        acia.tick(10);
        assert_eq!(status(&acia) & SR_RDRF, SR_RDRF);
        assert_eq!(status(&acia) & SR_IRQ, 0);
        assert!(!acia.poll_irq());
    }

    #[test]
    fn peek_does_not_consume_or_change_flags() {
        let acia = fast(false);
        acia.write(CTRL, 0x95);
        acia.enqueue_rx(b"KL");
        acia.tick(10);
        for _ in 0..3 {
            assert_eq!(acia.peek(DATA), b'K');
            assert_eq!(acia.peek(CTRL), SR_IRQ | SR_TDRE | SR_RDRF);
        }
        acia.tick(100);
        assert!(acia.poll_irq());
        assert_eq!(acia.rx_backlog(), 1, "L still waits on the host side");
        assert_eq!(acia.read(DATA), b'K');
        assert_eq!(acia.peek(DATA), b'K', "peek shows the last RDR value");
        assert_eq!(acia.peek(0x1234), 0xFF);
    }

    #[test]
    fn lenient_rx_paces_by_reads_and_never_loses_input() {
        let acia = fast(false);
        acia.enqueue_rx(b"HELLO");
        assert_eq!(cycles_until_rdrf(&acia, 100), Some(10));
        acia.tick(5_000); // a slow program: no overrun, input waits
        assert_eq!(status(&acia), SR_TDRE | SR_RDRF);
        assert_eq!(acia.rx_backlog(), 4);
        let mut got = vec![acia.read(DATA)];
        while got.len() < 5 {
            // Next character: one frame time after the previous read.
            assert_eq!(cycles_until_rdrf(&acia, 100), Some(10));
            got.push(acia.read(DATA));
        }
        assert_eq!(got, b"HELLO");
        assert_eq!(status(&acia) & (SR_RDRF | SR_OVRN), 0);
    }

    #[test]
    fn strict_rx_overrun_follows_datasheet() {
        let acia = fast(true);
        acia.write(CTRL, 0x95); // RIE
        acia.enqueue_rx(b"ABC");
        acia.tick(30); // A lands; B and C complete while RDRF is set: lost
        assert_eq!(
            status(&acia),
            SR_IRQ | SR_TDRE | SR_RDRF,
            "OVRN not visible before the valid character is read"
        );
        assert_eq!(acia.read(DATA), b'A');
        assert_eq!(
            status(&acia),
            SR_IRQ | SR_OVRN | SR_TDRE | SR_RDRF,
            "OVRN shows, RDRF stays set, /IRQ stays asserted"
        );
        assert!(acia.poll_irq());
        assert_eq!(acia.read(DATA), b'A', "RDR unchanged: B and C were lost");
        assert_eq!(status(&acia), SR_TDRE, "status + data read reset OVRN and RDRF");
        assert!(!acia.poll_irq());
        assert_eq!(acia.rx_backlog(), 0);
    }

    #[test]
    fn strict_rx_fast_reader_receives_everything() {
        let acia = fast(true);
        acia.enqueue_rx(b"FAST");
        let mut got = Vec::new();
        for _ in 0..100 {
            acia.tick(1);
            if status(&acia) & SR_RDRF != 0 {
                got.push(acia.read(DATA));
            }
        }
        assert_eq!(got, b"FAST");
        assert_eq!(status(&acia) & SR_OVRN, 0);
    }

    #[test]
    fn rdr_keeps_last_received_byte() {
        let acia = fast(false);
        acia.enqueue_rx(b"Q");
        acia.tick(10);
        assert_eq!(acia.read(DATA), b'Q');
        assert_eq!(status(&acia) & SR_RDRF, 0);
        assert_eq!(acia.read(DATA), b'Q', "RDR read with RDRF = 0 is not $00");
    }

    #[test]
    fn transmitter_is_double_buffered() {
        let acia = fast(false);
        acia.write(DATA, b'X');
        assert_eq!(status(&acia) & SR_TDRE, 0);
        acia.tick(1); // one bit: X moves into the shift register
        assert_eq!(status(&acia) & SR_TDRE, SR_TDRE);
        acia.write(DATA, b'Y'); // TDR full while X is on the line
        assert_eq!(status(&acia) & SR_TDRE, 0);
        acia.tick(9);
        assert_eq!(text(&acia), "", "X needs a full 10-bit frame");
        acia.tick(1);
        assert_eq!(text(&acia), "X");
        assert_eq!(status(&acia) & SR_TDRE, SR_TDRE, "Y moved on at once");
        acia.tick(10);
        assert_eq!(text(&acia), "XY");
    }

    #[test]
    fn write_while_tdre_clear_overwrites_tdr() {
        let acia = fast(false);
        acia.write(DATA, b'X');
        acia.write(DATA, b'Y'); // before the transfer: X is lost
        acia.tick(50);
        assert_eq!(text(&acia), "Y");
    }

    #[test]
    fn seven_bit_modes_mask_bit_7() {
        let acia = acia_with(1_000_000, 1_000_000, false);
        acia.write(CTRL, 0x09); // 7E1 /16
        acia.enqueue_rx(&[0xC1]);
        acia.tick(10);
        assert_eq!(acia.read(DATA), 0x41, "RDR bit 7 reads 0");
        acia.write(DATA, 0xC2);
        acia.tick(11);
        assert_eq!(text(&acia), "B", "bit 7 is not transmitted");

        let eight = fast(false);
        eight.enqueue_rx(&[0xC1]);
        eight.tick(10);
        assert_eq!(eight.read(DATA), 0xC1);
    }

    #[test]
    fn motorola_map_puts_status_at_base() {
        let acia = Acia6850::new(AciaConfig {
            enabled: true,
            base_addr: 0xA000,
            baud: 1_000_000,
            e_clock_hz: 1_000_000,
            motorola: true,
            strict_rx: false,
        });
        acia.write(0xA000, 0x15);
        assert_eq!(acia.read(0xA000), SR_TDRE);
        acia.enqueue_rx(b"M");
        acia.tick(10);
        assert_eq!(acia.peek(0xA001), b'M');
        assert_eq!(acia.read(0xA001), b'M');
        let regs = acia.io_registers();
        assert_eq!(regs[0].address, 0xA001);
        assert_eq!(regs[0].value, b'M');
        assert_eq!(regs[1].address, 0xA000);
        assert_eq!(regs[1].name, "ACIA Status (CR $15)");
    }

    #[test]
    fn legacy_map_is_the_default() {
        assert!(!AciaConfig::default().motorola);
        assert!(!AciaConfig::default().strict_rx);
        let acia = fast(false);
        let regs = acia.io_registers();
        assert_eq!((regs[0].address, regs[1].address), (DATA, CTRL));
    }

    #[test]
    fn config_json_without_new_fields_uses_defaults() {
        let cfg: AciaConfig = serde_json::from_value(serde_json::json!({
            "enabled": true, "base_addr": 65440, "baud": 9600, "e_clock_hz": 1000000
        }))
        .unwrap();
        assert!(!cfg.motorola);
        assert!(!cfg.strict_rx);
    }

    #[test]
    fn old_snapshot_restores_running_chip() {
        let old = serde_json::json!({
            "config": { "enabled": true, "base_addr": 40960, "baud": 115200,
                        "e_clock_hz": 1843200, "motorola": true },
            "state": { "control": 21, "rx_queue": [], "rx_pending": null,
                       "tx_pending": null, "tx_queue": [], "tx_history": [79, 75],
                       "irq_latched": false }
        });
        let acia: Acia6850 = serde_json::from_value(old).unwrap();
        assert_eq!(acia.read(0xA000), SR_TDRE, "control $15: not in reset");
        assert_eq!(acia.terminal_state().tx_text, "OK");
        let json = serde_json::to_value(&acia).unwrap();
        let again: Acia6850 = serde_json::from_value(json).unwrap();
        assert_eq!(again.terminal_state().tx_text, "OK");
    }

    #[test]
    fn baud_change_keeps_time_left_on_frames() {
        let mut acia = fast(false);
        acia.enqueue_rx(b"B");
        acia.tick(5); // half of a 10-cycle frame
        let mut cfg = acia.config();
        cfg.baud = 2_000_000; // unit scale doubles
        acia.set_config(cfg);
        assert_eq!(cycles_until_rdrf(&acia, 100), Some(5));
    }

    #[test]
    fn disabled_acia_is_invisible_and_quiet() {
        let mut acia = fast(false);
        acia.write(CTRL, 0x35); // TIE: /IRQ asserted
        assert!(acia.poll_irq());
        let mut cfg = acia.config();
        cfg.enabled = false;
        acia.set_config(cfg);
        assert!(!acia.poll_irq());
        assert!(!acia.handles(CTRL));
        assert_eq!(acia.read(CTRL), 0xFF);
        assert!(acia.io_registers().is_empty());
        cfg.enabled = true;
        acia.set_config(cfg);
        assert_eq!(status(&acia), 0x00, "re-enabling powers the chip on");
    }

    #[test]
    fn reset_line_leaves_acia_untouched() {
        let mut acia = fast(false);
        acia.enqueue_rx(b"R");
        acia.tick(10);
        acia.reset();
        assert_eq!(status(&acia), SR_TDRE | SR_RDRF);
        assert_eq!(acia.read(DATA), b'R');
    }

    #[test]
    fn enqueue_rx_text_encodes_latin1() {
        let acia = fast(false);
        acia.enqueue_rx_text("\u{e4}\u{20ac}");
        acia.tick(10);
        assert_eq!(acia.read(DATA), 0xE4);
        acia.tick(10);
        assert_eq!(acia.read(DATA), b'?');
    }

    #[test]
    fn terminal_handles_backspace_carriage_return_and_linefeed() {
        let render = |bytes: &[u8]| render_terminal(bytes.iter().copied());
        assert_eq!(render(b"AB\x08C"), "AC", "BS erases the last character");
        assert_eq!(render(b"PRIMT\x08\x08NT"), "PRINT");
        assert_eq!(render(b"AB\x08 \x08"), "A", "BS SPACE BS erase idiom");
        assert_eq!(render(b"\x08\x08X"), "X", "BS at column 0 stays put");
        assert_eq!(render(b"HELLO\rJ"), "JELLO", "CR returns to column 0");
        assert_eq!(render(b"ABCDE\r\x08\x08XY"), "XYCDE");
        assert_eq!(render(b"ABC\r\x1b"), "ABC", "other controls are ignored");
        assert_eq!(render(b"OK\r\nREADY\r\n"), "OK\nREADY\n");
        assert_eq!(render(b"ONE\nTWO"), "ONE\nTWO", "LF starts a new line at column 0");
        assert_eq!(render(b"A\tB"), "A       B");
        assert_eq!(render(b"OLD\x0cNEW"), "NEW", "FF clears the screen");
        assert_eq!(
            render(b"ABCD\r\x08XY\x08\x08Z"),
            "Z CD",
            "mid-line BS blanks the cell, no shift"
        );
        assert_eq!(render(&[0x00, 0x07, b'A', 0x7F]), "A");
    }

    #[test]
    fn terminal_decodes_latin1() {
        let text = render_terminal([b'G', 0xFC, b'n', b't', b'e', b'r', 0xA9, 0xFF]);
        assert_eq!(text, "G\u{fc}nter\u{a9}\u{ff}");
        assert!(!text.contains('\u{fffd}'));
    }

    #[test]
    fn terminal_state_renders_transmitted_bytes() {
        let acia = fast(false);
        for &byte in b"10 PRIMT\x08\x08NT\r\n\xe9" {
            while status(&acia) & SR_TDRE == 0 {
                acia.tick(1);
            }
            acia.write(DATA, byte);
        }
        acia.tick(100);
        assert_eq!(text(&acia), "10 PRINT\n\u{e9}");
        acia.clear_terminal();
        assert_eq!(text(&acia), "");
    }

    #[test]
    fn tx_history_ring_is_capped_in_constant_time() {
        let mut state = AciaState::default();
        for i in 0..(3 * TX_HISTORY_CAP) {
            push_tx_history(&mut state, b'a' + (i % 26) as u8);
        }
        assert_eq!(state.tx_history.len(), TX_HISTORY_CAP);
        let first = (2 * TX_HISTORY_CAP) % 26;
        assert_eq!(state.tx_history.front(), Some(&(b'a' + first as u8)));
    }

    #[test]
    fn host_fifo_is_bounded() {
        let acia = fast(false);
        acia.enqueue_rx(&vec![b'x'; RX_FIFO_CAP + 10]);
        assert_eq!(acia.rx_backlog(), RX_FIFO_CAP);
    }
}
