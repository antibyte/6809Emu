//! CoCo / Dragon keyboard matrix.
//!
//! Hardware scan (Color BASIC POLCAT):
//! - Write active-low column select to PIA0 port B (`$FF02`)
//! - Read active-low rows from PIA0 port A (`$FF00`)

use serde::{Deserialize, Serialize};

/// 7 rows × 8 columns of key state (true = key down).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardMatrix {
    /// `keys[row][col]` — row 0..6, col 0..7
    keys: [[bool; 8]; 7],
}

impl Default for KeyboardMatrix {
    fn default() -> Self {
        Self {
            keys: [[false; 8]; 7],
        }
    }
}

impl KeyboardMatrix {
    pub fn clear(&mut self) {
        self.keys = [[false; 8]; 7];
    }

    pub fn set_key(&mut self, row: usize, col: usize, down: bool) {
        if row < 7 && col < 8 {
            self.keys[row][col] = down;
        }
    }

    /// Read row inputs for a given column drive mask (active low on columns).
    /// Returns active-low row byte (bit N low if any selected column has that row key).
    pub fn read_rows(&self, column_drive: u8) -> u8 {
        let mut rows = 0xFFu8;
        for col in 0..8u8 {
            // Column selected when driven low.
            if column_drive & (1 << col) != 0 {
                continue;
            }
            for row in 0..7u8 {
                if self.keys[row as usize][col as usize] {
                    rows &= !(1 << row);
                }
            }
        }
        rows
    }

    /// Map a host browser/OS key code to CoCo matrix (row, col).
    /// Returns `None` for unmapped keys.
    pub fn map_host_key(code: &str) -> Option<(u8, u8)> {
        // Matrix from MAME / XRoar CoCo layout:
        //   col →  0    1    2    3    4    5    6    7
        // row 0   @    A    B    C    D    E    F    G
        // row 1   H    I    J    K    L    M    N    O
        // row 2   P    Q    R    S    T    U    V    W
        // row 3   X    Y    Z   UP  DOWN LEFT RIGHT SPACE
        // row 4   0    1    2    3    4    5    6    7
        // row 5   8    9    :    ;    ,    -    .    /
        // row 6 ENTER CLEAR BREAK  ..  .. SHIFT  .. ALT
        Some(match code {
            // Letters
            "KeyA" => (0, 1),
            "KeyB" => (0, 2),
            "KeyC" => (0, 3),
            "KeyD" => (0, 4),
            "KeyE" => (0, 5),
            "KeyF" => (0, 6),
            "KeyG" => (0, 7),
            "KeyH" => (1, 0),
            "KeyI" => (1, 1),
            "KeyJ" => (1, 2),
            "KeyK" => (1, 3),
            "KeyL" => (1, 4),
            "KeyM" => (1, 5),
            "KeyN" => (1, 6),
            "KeyO" => (1, 7),
            "KeyP" => (2, 0),
            "KeyQ" => (2, 1),
            "KeyR" => (2, 2),
            "KeyS" => (2, 3),
            "KeyT" => (2, 4),
            "KeyU" => (2, 5),
            "KeyV" => (2, 6),
            "KeyW" => (2, 7),
            "KeyX" => (3, 0),
            "KeyY" => (3, 1),
            "KeyZ" => (3, 2),
            // @ on CoCo is its own key; map common host keys
            "BracketLeft" => (0, 0),
            // Arrows / space
            // BASIC line editor uses LEFT ARROW as backspace (CHR$ 8).
            "ArrowUp" => (3, 3),
            "ArrowDown" => (3, 4),
            "ArrowLeft" | "Backspace" => (3, 5),
            "ArrowRight" => (3, 6),
            "Space" => (3, 7),
            // Delete → CLEAR (wipes input line on many BASIC prompts)
            "Delete" => (6, 1),
            // Digits
            "Digit0" | "Numpad0" => (4, 0),
            "Digit1" | "Numpad1" => (4, 1),
            "Digit2" | "Numpad2" => (4, 2),
            "Digit3" | "Numpad3" => (4, 3),
            "Digit4" | "Numpad4" => (4, 4),
            "Digit5" | "Numpad5" => (4, 5),
            "Digit6" | "Numpad6" => (4, 6),
            "Digit7" | "Numpad7" => (4, 7),
            "Digit8" | "Numpad8" => (5, 0),
            "Digit9" | "Numpad9" => (5, 1),
            // Punctuation
            "Semicolon" => (5, 3),
            "Comma" => (5, 4),
            "Minus" | "NumpadSubtract" => (5, 5),
            "Period" | "NumpadDecimal" => (5, 6),
            "Slash" | "NumpadDivide" => (5, 7),
            "Quote" => (5, 2),
            // Control
            "Enter" | "NumpadEnter" => (6, 0),
            "Home" | "Escape" => (6, 1), // CLEAR
            "End" | "Pause" | "F1" => (6, 2), // BREAK
            "ShiftLeft" | "ShiftRight" => (6, 5),
            "AltLeft" | "AltRight" => (6, 7),
            _ => return None,
        })
    }

    /// Apply a host key event (`code` from KeyboardEvent.code).
    pub fn host_key(&mut self, code: &str, down: bool) {
        if let Some((row, col)) = Self::map_host_key(code) {
            self.set_key(row as usize, col as usize, down);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn backspace_maps_to_left_arrow() {
        assert_eq!(
            KeyboardMatrix::map_host_key("Backspace"),
            KeyboardMatrix::map_host_key("ArrowLeft")
        );
        assert_eq!(KeyboardMatrix::map_host_key("Backspace"), Some((3, 5)));
    }
}
