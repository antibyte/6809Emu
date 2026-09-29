//! CoCo 2 / Dragon 32 keyboard matrix and host keyboard mapping.
//!
//! The switches connect row r (PIA0 PA r) with column c (PIA0 PB c). BASIC
//! drives one column low at a time and reads the rows (POLCAT), but the
//! matrix is passive, so it also works in reverse (rows driven, columns read).
//!
//! CoCo matrix (row = PA bit, column = PB bit):
//!
//! ```text
//!   col →   0      1      2      3      4      5      6      7
//! row 0     @      A      B      C      D      E      F      G
//! row 1     H      I      J      K      L      M      N      O
//! row 2     P      Q      R      S      T      U      V      W
//! row 3     X      Y      Z      ↑      ↓      ←      →    SPACE
//! row 4     0      1      2      3      4      5      6      7
//! row 5     8      9      :      ;      ,      -      .      /
//! row 6   ENTER  CLEAR  BREAK    -      -      -      -    SHIFT
//! ```
//!
//! The Dragon 32 has the same keycaps, but rows 0-5 are wired rotated:
//! `dragon_row = (coco_row + 2) % 6` (row 0 = `01234567`, row 1 = `89:;,-./`,
//! row 2 = `@ABCDEFG`, ..., row 5 = `XYZ↑↓←→ SPACE`); row 6 is identical.
//! The CoCo 3 keys (ALT, CTRL, F1, F2) do not exist on these machines.
//!
//! Host events: [`KeyboardMatrix::host_event`] maps the typed character
//! (`KeyboardEvent.key`) to the key combination that produces it on the
//! emulated machine (`"` = SHIFT+2, `=` = SHIFT+-, ...), so any host layout
//! types correctly and the host Shift state does not matter for characters.
//! Keys without a printable character (Enter, arrows, Shift alone, ...) fall
//! back to a positional map of `KeyboardEvent.code`. Every press is
//! remembered per `code`, so the matching release frees exactly those keys.
//!
//! Timing: changes go through a FIFO so that a tap stays down for at least
//! `min_hold` of emulated time (BASIC scans the keyboard in software, a key
//! pressed and released between two scans would be lost) and a released key
//! stays up for `min_hold` before it can go down again (so double letters are
//! seen twice). The board advances the clock with [`KeyboardMatrix::advance`].

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Key wiring of the emulated machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeyboardLayout {
    #[default]
    Coco,
    Dragon,
}

/// Key position in CoCo numbering: (row = PA bit, column = PB bit).
pub type MatrixKey = (u8, u8);

pub const KEY_AT: MatrixKey = (0, 0);
pub const KEY_UP: MatrixKey = (3, 3);
pub const KEY_DOWN: MatrixKey = (3, 4);
pub const KEY_LEFT: MatrixKey = (3, 5);
pub const KEY_RIGHT: MatrixKey = (3, 6);
pub const KEY_SPACE: MatrixKey = (3, 7);
pub const KEY_ENTER: MatrixKey = (6, 0);
pub const KEY_CLEAR: MatrixKey = (6, 1);
pub const KEY_BREAK: MatrixKey = (6, 2);
pub const KEY_SHIFT: MatrixKey = (6, 7);

impl KeyboardLayout {
    /// Physical (row, column) of a key given in CoCo numbering.
    pub fn physical(self, key: MatrixKey) -> MatrixKey {
        let (row, col) = key;
        match self {
            KeyboardLayout::Dragon if row < 6 => ((row + 2) % 6, col),
            _ => (row, col),
        }
    }
}

/// Letter key (CoCo numbering): A = (0,1) ... G = (0,7), H = (1,0) ... Z = (3,2).
fn letter_key(upper: u8) -> MatrixKey {
    let idx = upper - b'A' + 1;
    (idx / 8, idx % 8)
}

/// Digit key (CoCo numbering): 0-7 = row 4, 8-9 = row 5.
fn digit_key(digit: u8) -> MatrixKey {
    if digit < 8 {
        (4, digit)
    } else {
        (5, digit - 8)
    }
}

/// The key (CoCo numbering) and SHIFT state that type `ch` on a CoCo 2 or
/// Dragon 32 (both have the same keycap legends). Letters of either case
/// map to the unshifted letter key (BASIC wants upper case; SHIFT+letter
/// would give inverse-video lower case). `None` = not typeable.
pub fn char_chord(ch: char) -> Option<(MatrixKey, bool)> {
    let unshifted = |key| Some((key, false));
    let shifted = |key| Some((key, true));
    match ch {
        'A'..='Z' => unshifted(letter_key(ch as u8)),
        'a'..='z' => unshifted(letter_key(ch.to_ascii_uppercase() as u8)),
        '0'..='9' => unshifted(digit_key(ch as u8 - b'0')),
        '@' => unshifted(KEY_AT),
        ' ' => unshifted(KEY_SPACE),
        ':' => unshifted((5, 2)),
        ';' => unshifted((5, 3)),
        ',' => unshifted((5, 4)),
        '-' => unshifted((5, 5)),
        '.' => unshifted((5, 6)),
        '/' => unshifted((5, 7)),
        '!' => shifted(digit_key(1)),
        '"' => shifted(digit_key(2)),
        '#' => shifted(digit_key(3)),
        '$' => shifted(digit_key(4)),
        '%' => shifted(digit_key(5)),
        '&' => shifted(digit_key(6)),
        '\'' => shifted(digit_key(7)),
        '(' => shifted(digit_key(8)),
        ')' => shifted(digit_key(9)),
        '*' => shifted((5, 2)),
        '+' => shifted((5, 3)),
        '<' => shifted((5, 4)),
        '=' => shifted((5, 5)),
        '>' => shifted((5, 6)),
        '?' => shifted((5, 7)),
        // Arrow-key characters of Color / Dragon BASIC.
        '^' | '↑' => unshifted(KEY_UP),
        '_' => shifted(KEY_UP),
        '[' => shifted(KEY_DOWN),
        ']' => shifted(KEY_RIGHT),
        '\\' => shifted(KEY_CLEAR),
        _ => None,
    }
}

/// How a host key affects the emulated SHIFT key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftUse {
    /// Leaves SHIFT alone (ENTER, arrows, ...): the host Shift key applies.
    Ignore,
    /// The host Shift key itself.
    Press,
    /// A typed character: SHIFT exactly as the character needs it,
    /// whatever the host Shift key is doing.
    Force(bool),
}

/// One host key held down: the matrix keys it closes (physical numbering,
/// SHIFT handled separately through `shift`).
#[derive(Debug, Clone)]
struct Holder {
    code: String,
    keys: Vec<MatrixKey>,
    shift: ShiftUse,
    since: u64,
}

#[derive(Debug, Clone)]
enum Pending {
    Down(Holder),
    Up(String),
}

/// 7 × 8 keyboard matrix plus the host key bookkeeping. Only transient input
/// state lives here, so nothing is serialized (a restored session starts
/// with all keys up).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyboardMatrix {
    #[serde(skip)]
    active: Vec<Holder>,
    #[serde(skip)]
    queue: VecDeque<Pending>,
    /// Keys set directly with [`KeyboardMatrix::set_key`] (row bit masks).
    #[serde(skip)]
    direct: [u8; 7],
    /// Effective matrix: bit c of `rows[r]` = switch (r, c) closed.
    #[serde(skip)]
    rows: [u8; 7],
    /// Emulated time (board ticks).
    #[serde(skip)]
    now: u64,
    /// Minimum down / up time in board ticks (0 = immediate).
    #[serde(skip)]
    min_hold: u64,
    /// Release time + 1 of every physical key (0 = never released).
    #[serde(skip)]
    released_at: [[u64; 8]; 7],
}

impl KeyboardMatrix {
    /// Release everything immediately (focus loss).
    pub fn clear(&mut self) {
        self.active.clear();
        self.queue.clear();
        self.direct = [0; 7];
        self.released_at = [[0; 8]; 7];
        self.recompute();
    }

    /// Minimum time (board ticks) a key stays down, and stays up before it
    /// can be pressed again.
    pub fn set_min_hold(&mut self, ticks: u64) {
        self.min_hold = ticks;
    }

    /// Advance emulated time and apply queued key changes that are due.
    pub fn advance(&mut self, ticks: u64) {
        self.now = self.now.saturating_add(ticks);
        if !self.queue.is_empty() {
            self.process();
        }
    }

    /// Key changes still waiting for their minimum hold time.
    #[allow(dead_code)]
    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Close or open one switch directly (physical row/column), bypassing
    /// the host mapping and the timing.
    #[allow(dead_code)]
    pub fn set_key(&mut self, row: usize, col: usize, down: bool) {
        if row < 7 && col < 8 {
            if down {
                self.direct[row] |= 1 << col;
            } else {
                self.direct[row] &= !(1 << col);
            }
            self.recompute();
        }
    }

    /// Whether switch (row, col) (physical numbering) is closed.
    #[allow(dead_code)]
    pub fn is_pressed(&self, row: usize, col: usize) -> bool {
        row < 7 && col < 8 && self.rows[row] & (1 << col) != 0
    }

    /// Rows (bits 0-6) pulled low through closed switches by the columns
    /// driven low in `cols_low`.
    pub fn rows_pulled_low(&self, cols_low: u8) -> u8 {
        let mut out = 0u8;
        for (r, &mask) in self.rows.iter().enumerate() {
            if mask & cols_low != 0 {
                out |= 1 << r;
            }
        }
        out
    }

    /// Columns (bits 0-7) pulled low through closed switches by the rows
    /// driven low in `rows_low` (reverse scan).
    pub fn cols_pulled_low(&self, rows_low: u8) -> u8 {
        let mut out = 0u8;
        for (r, &mask) in self.rows.iter().enumerate() {
            if rows_low & (1 << r) != 0 {
                out |= mask;
            }
        }
        out
    }

    /// Active-low row byte for an active-low column drive (bit 7 stays high).
    #[allow(dead_code)]
    pub fn read_rows(&self, column_drive: u8) -> u8 {
        !self.rows_pulled_low(!column_drive)
    }

    /// Positional map of a host `KeyboardEvent.code` to a key in CoCo
    /// numbering (used when no typed character is available).
    pub fn map_host_key(code: &str) -> Option<MatrixKey> {
        if let Some(letter) = code.strip_prefix("Key") {
            let b = letter.as_bytes();
            if b.len() == 1 && b[0].is_ascii_uppercase() {
                return Some(letter_key(b[0]));
            }
        }
        for prefix in ["Digit", "Numpad"] {
            if let Some(d) = code.strip_prefix(prefix) {
                let b = d.as_bytes();
                if b.len() == 1 && b[0].is_ascii_digit() {
                    return Some(digit_key(b[0] - b'0'));
                }
            }
        }
        Some(match code {
            // @ on the CoCo is its own key, next to P on a US keyboard.
            "BracketLeft" => KEY_AT,
            "Quote" => (5, 2),
            "Semicolon" => (5, 3),
            "Comma" => (5, 4),
            "Minus" | "NumpadSubtract" => (5, 5),
            "Period" | "NumpadDecimal" => (5, 6),
            "Slash" | "NumpadDivide" => (5, 7),
            "ArrowUp" => KEY_UP,
            "ArrowDown" => KEY_DOWN,
            // BASIC's line editor uses LEFT ARROW as backspace (CHR$ 8).
            "ArrowLeft" | "Backspace" => KEY_LEFT,
            "ArrowRight" => KEY_RIGHT,
            "Space" => KEY_SPACE,
            "Enter" | "NumpadEnter" => KEY_ENTER,
            "Home" | "Escape" | "Delete" => KEY_CLEAR,
            "End" | "Pause" | "F1" => KEY_BREAK,
            "ShiftLeft" | "ShiftRight" => KEY_SHIFT,
            _ => return None,
        })
    }

    /// Host key event: `code` = KeyboardEvent.code, `key` = KeyboardEvent.key.
    pub fn host_event(&mut self, layout: KeyboardLayout, code: &str, key: Option<&str>, down: bool) {
        if down {
            if self.is_down_or_pending(code) {
                return; // auto-repeat or duplicate
            }
            let Some((keys, shift)) = map_event(code, key) else {
                return;
            };
            let keys = keys.into_iter().map(|k| layout.physical(k)).collect();
            self.queue.push_back(Pending::Down(Holder {
                code: code.to_string(),
                keys,
                shift,
                since: 0,
            }));
        } else {
            if !self.is_down_or_pending(code) {
                return;
            }
            self.queue.push_back(Pending::Up(code.to_string()));
        }
        self.process();
    }

    /// Whether `code` is down once all queued changes are applied.
    fn is_down_or_pending(&self, code: &str) -> bool {
        let mut down = self.active.iter().any(|h| h.code == code);
        for pending in &self.queue {
            match pending {
                Pending::Down(h) if h.code == code => down = true,
                Pending::Up(c) if c == code => down = false,
                _ => {}
            }
        }
        down
    }

    fn process(&mut self) {
        let mut changed = false;
        while let Some(front) = self.queue.front() {
            match front {
                Pending::Down(holder) => {
                    let blocked = holder.keys.iter().any(|&(r, c)| {
                        let released = self.released_at[r as usize][c as usize];
                        released != 0 && self.now + 1 < released + self.min_hold
                    });
                    if blocked {
                        break;
                    }
                    let mut holder = holder.clone();
                    holder.since = self.now;
                    self.active.push(holder);
                    changed = true;
                }
                Pending::Up(code) => {
                    if let Some(idx) = self.active.iter().rposition(|h| &h.code == code) {
                        if self.now < self.active[idx].since + self.min_hold {
                            break;
                        }
                        let holder = self.active.remove(idx);
                        for &(r, c) in &holder.keys {
                            if !self.active.iter().any(|h| h.keys.contains(&(r, c))) {
                                self.released_at[r as usize][c as usize] = self.now + 1;
                            }
                        }
                        changed = true;
                    }
                }
            }
            self.queue.pop_front();
        }
        if changed {
            self.recompute();
        }
    }

    fn recompute(&mut self) {
        let mut rows = self.direct;
        let mut shift = false;
        for holder in &self.active {
            for &(r, c) in &holder.keys {
                rows[r as usize] |= 1 << c;
            }
            // The most recently pressed key with an opinion decides SHIFT:
            // a typed character forces what it needs, the host Shift key
            // presses it (so SHIFT pressed after a held letter still counts).
            match holder.shift {
                ShiftUse::Force(needed) => shift = needed,
                ShiftUse::Press => shift = true,
                ShiftUse::Ignore => {}
            }
        }
        if shift {
            let (sr, sc) = KEY_SHIFT;
            rows[sr as usize] |= 1 << sc;
        }
        self.rows = rows;
    }
}

/// Map a host event to matrix keys (CoCo numbering) and its SHIFT use.
fn map_event(code: &str, key: Option<&str>) -> Option<(Vec<MatrixKey>, ShiftUse)> {
    if let Some(k) = key {
        let mut chars = k.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            // A printable character: type it, or nothing if the machine lacks it.
            return char_chord(ch).map(|(key, shift)| (vec![key], ShiftUse::Force(shift)));
        }
        match k {
            // Dead keys / IME: the composed character arrives separately.
            "Dead" | "Unidentified" | "Process" | "Compose" => return None,
            "Shift" => return Some((Vec::new(), ShiftUse::Press)),
            _ => {}
        }
        let named = match k {
            "Enter" => Some(KEY_ENTER),
            "ArrowUp" => Some(KEY_UP),
            "ArrowDown" => Some(KEY_DOWN),
            "ArrowLeft" | "Backspace" => Some(KEY_LEFT),
            "ArrowRight" => Some(KEY_RIGHT),
            "Escape" | "Home" | "Delete" | "Clear" => Some(KEY_CLEAR),
            "End" | "Pause" | "F1" | "Cancel" => Some(KEY_BREAK),
            _ => None,
        };
        if let Some(key) = named {
            return Some((vec![key], ShiftUse::Ignore));
        }
    }
    match KeyboardMatrix::map_host_key(code)? {
        KEY_SHIFT => Some((Vec::new(), ShiftUse::Press)),
        key => Some((vec![key], ShiftUse::Ignore)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COCO: KeyboardLayout = KeyboardLayout::Coco;
    const DRAGON: KeyboardLayout = KeyboardLayout::Dragon;

    fn pressed(kbd: &KeyboardMatrix) -> Vec<MatrixKey> {
        let mut out = Vec::new();
        for r in 0..7 {
            for c in 0..8 {
                if kbd.is_pressed(r, c) {
                    out.push((r as u8, c as u8));
                }
            }
        }
        out
    }

    #[test]
    fn no_keys_all_rows_high() {
        let kbd = KeyboardMatrix::default();
        assert_eq!(kbd.read_rows(0x00), 0xFF); // all columns selected, no keys
        assert_eq!(kbd.read_rows(0xFF), 0xFF); // no columns selected
    }

    #[test]
    fn key_a_pulls_row0_when_col1_selected() {
        let mut kbd = KeyboardMatrix::default();
        kbd.set_key(0, 1, true); // A
        // Select only column 1 (bit1 low)
        assert_eq!(kbd.read_rows(0b1111_1101) & 0x01, 0x00);
        // Other columns not selected → row stays high for that key
        assert_eq!(kbd.read_rows(0b1111_1110) & 0x01, 0x01);
        // Reverse scan: row 0 driven low pulls column 1 low.
        assert_eq!(kbd.cols_pulled_low(0x01), 0x02);
        assert_eq!(kbd.cols_pulled_low(0x02), 0x00);
    }

    #[test]
    fn backspace_maps_to_left_arrow() {
        assert_eq!(
            KeyboardMatrix::map_host_key("Backspace"),
            KeyboardMatrix::map_host_key("ArrowLeft")
        );
        assert_eq!(KeyboardMatrix::map_host_key("Backspace"), Some((3, 5)));
    }

    #[test]
    fn host_shift_is_the_real_shift_key_and_alt_is_unmapped() {
        assert_eq!(KeyboardMatrix::map_host_key("ShiftLeft"), Some(KEY_SHIFT));
        assert_eq!(KeyboardMatrix::map_host_key("ShiftRight"), Some((6, 7)));
        assert_eq!(KeyboardMatrix::map_host_key("AltLeft"), None);
        assert_eq!(KeyboardMatrix::map_host_key("AltRight"), None);
        assert_eq!(KeyboardMatrix::map_host_key("ControlLeft"), None);
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), true);
        assert_eq!(pressed(&kbd), vec![(6, 7)]);
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), false);
        assert!(pressed(&kbd).is_empty());
    }

    #[test]
    fn positional_letters_digits_and_controls() {
        assert_eq!(KeyboardMatrix::map_host_key("KeyA"), Some((0, 1)));
        assert_eq!(KeyboardMatrix::map_host_key("KeyH"), Some((1, 0)));
        assert_eq!(KeyboardMatrix::map_host_key("KeyZ"), Some((3, 2)));
        assert_eq!(KeyboardMatrix::map_host_key("Digit0"), Some((4, 0)));
        assert_eq!(KeyboardMatrix::map_host_key("Numpad9"), Some((5, 1)));
        assert_eq!(KeyboardMatrix::map_host_key("Enter"), Some(KEY_ENTER));
        assert_eq!(KeyboardMatrix::map_host_key("Escape"), Some(KEY_CLEAR));
        assert_eq!(KeyboardMatrix::map_host_key("Home"), Some(KEY_CLEAR));
        assert_eq!(KeyboardMatrix::map_host_key("End"), Some(KEY_BREAK));
        assert_eq!(KeyboardMatrix::map_host_key("Pause"), Some(KEY_BREAK));
        assert_eq!(KeyboardMatrix::map_host_key("F1"), Some(KEY_BREAK));
        assert_eq!(KeyboardMatrix::map_host_key("Space"), Some(KEY_SPACE));
    }

    #[test]
    fn characters_map_to_coco_keycaps() {
        let cases: &[(char, MatrixKey, bool)] = &[
            ('"', (4, 2), true),
            ('!', (4, 1), true),
            ('#', (4, 3), true),
            ('$', (4, 4), true),
            ('%', (4, 5), true),
            ('&', (4, 6), true),
            ('\'', (4, 7), true),
            ('(', (5, 0), true),
            (')', (5, 1), true),
            ('*', (5, 2), true),
            ('+', (5, 3), true),
            ('<', (5, 4), true),
            ('=', (5, 5), true),
            ('>', (5, 6), true),
            ('?', (5, 7), true),
            (':', (5, 2), false),
            ('-', (5, 5), false),
            ('a', (0, 1), false),
            ('A', (0, 1), false),
            ('z', (3, 2), false),
            ('@', (0, 0), false),
            ('0', (4, 0), false),
            ('9', (5, 1), false),
            (' ', (3, 7), false),
        ];
        for &(ch, key, shift) in cases {
            assert_eq!(char_chord(ch), Some((key, shift)), "char {ch:?}");
        }
        assert_eq!(char_chord('ä'), None);
        assert_eq!(char_chord('€'), None);
    }

    #[test]
    fn german_layout_equals_is_shift_minus_not_digit0() {
        // German QWERTZ: Shift+0 types '='.
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), true);
        kbd.host_event(COCO, "Digit0", Some("="), true);
        assert_eq!(pressed(&kbd), vec![(5, 5), (6, 7)]);
        kbd.host_event(COCO, "Digit0", Some("="), false);
        // German Shift+7 = '/': SHIFT must be released although host Shift is held.
        kbd.host_event(COCO, "Digit7", Some("/"), true);
        assert_eq!(pressed(&kbd), vec![(5, 7)]);
        kbd.host_event(COCO, "Digit7", Some("/"), false);
        // Only host Shift left: SHIFT down again.
        assert_eq!(pressed(&kbd), vec![(6, 7)]);
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), false);
        assert!(pressed(&kbd).is_empty());
    }

    #[test]
    fn release_frees_exactly_what_the_code_pressed() {
        let mut kbd = KeyboardMatrix::default();
        // US layout: Shift+2 = '"', user lets go of Shift first.
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), true);
        kbd.host_event(COCO, "Digit2", Some("\""), true);
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), false);
        assert_eq!(pressed(&kbd), vec![(4, 2), (6, 7)], "'\"' keeps its SHIFT");
        // The key-up event reports the unshifted character; the code decides.
        kbd.host_event(COCO, "Digit2", Some("2"), false);
        assert!(pressed(&kbd).is_empty());
    }

    #[test]
    fn dead_keys_and_unknown_characters_are_ignored() {
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "Backquote", Some("Dead"), true);
        kbd.host_event(COCO, "Quote", Some("ä"), true);
        assert!(pressed(&kbd).is_empty());
        kbd.host_event(COCO, "AltLeft", Some("Alt"), true);
        assert!(pressed(&kbd).is_empty());
    }

    #[test]
    fn named_keys_keep_host_shift() {
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), true);
        kbd.host_event(COCO, "ArrowLeft", Some("ArrowLeft"), true);
        assert_eq!(pressed(&kbd), vec![(3, 5), (6, 7)], "SHIFT+LEFT erases the line");
        kbd.clear();
        assert!(pressed(&kbd).is_empty());
    }

    #[test]
    fn host_shift_pressed_after_a_held_letter_counts() {
        // Games: hold a letter to move, press SHIFT as a button.
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "KeyA", Some("a"), true);
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), true);
        assert_eq!(pressed(&kbd), vec![(0, 1), (6, 7)]);
        kbd.host_event(COCO, "ShiftLeft", Some("Shift"), false);
        assert_eq!(pressed(&kbd), vec![(0, 1)]);
    }

    #[test]
    fn dragon_rows_are_rotated() {
        assert_eq!(DRAGON.physical((0, 1)), (2, 1)); // A
        assert_eq!(DRAGON.physical((4, 1)), (0, 1)); // 1
        assert_eq!(DRAGON.physical((5, 1)), (1, 1)); // 9
        assert_eq!(DRAGON.physical((2, 0)), (4, 0)); // P
        assert_eq!(DRAGON.physical((3, 7)), (5, 7)); // SPACE
        assert_eq!(DRAGON.physical(KEY_SHIFT), KEY_SHIFT);
        assert_eq!(DRAGON.physical(KEY_ENTER), KEY_ENTER);
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(DRAGON, "KeyA", Some("a"), true);
        assert_eq!(pressed(&kbd), vec![(2, 1)]);
        kbd.host_event(DRAGON, "Digit1", Some("1"), true);
        assert_eq!(pressed(&kbd), vec![(0, 1), (2, 1)]);
    }

    #[test]
    fn short_tap_is_held_for_min_time() {
        let mut kbd = KeyboardMatrix::default();
        kbd.set_min_hold(1000);
        kbd.host_event(COCO, "KeyA", Some("a"), true);
        kbd.host_event(COCO, "KeyA", Some("a"), false);
        assert!(kbd.is_pressed(0, 1), "released too early");
        kbd.advance(999);
        assert!(kbd.is_pressed(0, 1));
        kbd.advance(1);
        assert!(!kbd.is_pressed(0, 1));
        assert!(!kbd.has_pending());
    }

    #[test]
    fn repeated_key_stays_up_before_next_press_and_order_is_kept() {
        let mut kbd = KeyboardMatrix::default();
        kbd.set_min_hold(100);
        // "LL" typed faster than the minimum times.
        kbd.host_event(COCO, "KeyL", Some("l"), true);
        kbd.host_event(COCO, "KeyL", Some("l"), false);
        kbd.host_event(COCO, "KeyL", Some("l"), true);
        kbd.host_event(COCO, "KeyL", Some("l"), false);
        let l = (1, 4);
        let mut timeline = Vec::new();
        for _ in 0..40 {
            timeline.push(kbd.is_pressed(l.0, l.1));
            kbd.advance(10);
        }
        // down 100, up 100, down 100, then up.
        let expect: Vec<bool> = (0..40).map(|i| i < 10 || (20..30).contains(&i)).collect();
        assert_eq!(timeline, expect);

        // A key typed after a shifted one must not see the old SHIFT.
        kbd.host_event(COCO, "Digit2", Some("\""), true);
        kbd.host_event(COCO, "Digit2", Some("\""), false);
        kbd.host_event(COCO, "KeyB", Some("b"), true);
        assert!(kbd.is_pressed(6, 7) && !kbd.is_pressed(0, 2));
        kbd.advance(100);
        assert!(!kbd.is_pressed(6, 7) && kbd.is_pressed(0, 2) && !kbd.is_pressed(4, 2));
    }

    #[test]
    fn auto_repeat_downs_are_ignored() {
        let mut kbd = KeyboardMatrix::default();
        kbd.host_event(COCO, "KeyA", Some("a"), true);
        kbd.host_event(COCO, "KeyA", Some("a"), true);
        kbd.host_event(COCO, "KeyA", Some("a"), false);
        assert!(pressed(&kbd).is_empty());
        assert!(!kbd.has_pending());
    }
}
