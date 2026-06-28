use serde::{Deserialize, Serialize};

/// Simplified CoCo/Dragon keyboard matrix stub (8 rows × 7 column bits, active low).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyboardMatrix {
    /// Column lines per row (bit set = key down, active low on read).
    rows: [u8; 8],
    selected_row: u8,
}

impl KeyboardMatrix {
    pub fn select_row(&mut self, row_mask: u8) {
        self.selected_row = row_mask;
    }

    pub fn read_columns(&self) -> u8 {
        let mut cols = 0xFFu8;
        for i in 0..8u8 {
            let row_mask = 1u8 << i;
            if self.selected_row & row_mask != 0 {
                cols &= !self.rows[i as usize];
            }
        }
        cols
    }

    #[allow(dead_code)]
    pub fn set_row_keys(&mut self, row: usize, keys: u8) {
        if row < 8 {
            self.rows[row] = keys;
        }
    }

    #[allow(dead_code)]
    pub fn selected_row_mask(&self) -> u8 {
        self.selected_row
    }
}