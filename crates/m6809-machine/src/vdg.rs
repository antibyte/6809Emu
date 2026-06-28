use serde::{Deserialize, Serialize};

use crate::sam::Sam;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VdgMode {
    Text32x16,
    Semigraphics4,
    Semigraphics6,
    Graphics,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VdgLayout {
    pub mode: VdgMode,
    pub cols: u16,
    pub rows: u16,
    pub bytes: usize,
}

/// Decode VDG screen layout from PIA1 port B ($FF22) and SAM V0–V2.
pub fn decode_layout(pia1_data_b: u8, sam: &Sam) -> VdgLayout {
    let ga = (pia1_data_b >> 7) & 1;
    if ga != 0 {
        return VdgLayout {
            mode: VdgMode::Graphics,
            cols: 32,
            rows: 192,
            bytes: 6144,
        };
    }

    let gm2 = (pia1_data_b >> 6) & 1;
    let gm1 = (pia1_data_b >> 5) & 1;
    let gm0 = (pia1_data_b >> 4) & 1;
    let v = sam.v_mode_bits();

    if v == 0 {
        if gm2 != 0 && gm0 == 0 {
            return VdgLayout {
                mode: VdgMode::Semigraphics4,
                cols: 64,
                rows: 32,
                bytes: 512,
            };
        }
        if gm2 != 0 && gm0 != 0 {
            return VdgLayout {
                mode: VdgMode::Semigraphics6,
                cols: 64,
                rows: 48,
                bytes: 512,
            };
        }
        if gm2 == 0 && gm1 == 0 && gm0 == 0 {
            return VdgLayout {
                mode: VdgMode::Text32x16,
                cols: 32,
                rows: 16,
                bytes: 512,
            };
        }
    }

    VdgLayout {
        mode: VdgMode::Unknown,
        cols: 32,
        rows: 16,
        bytes: 512,
    }
}

/// Expand one SG4 byte into two display columns (2×4 fine pixels per byte).
pub fn sg4_columns(byte: u8) -> [char; 2] {
    [
        sg4_half(byte & 0x0F),
        sg4_half((byte >> 4) & 0x0F),
    ]
}

fn sg4_half(nibble: u8) -> char {
    const TABLE: [char; 16] = [
        ' ', '▄', '▀', '█', '▌', '▐', '░', '▓', '▖', '▗', '▘', '▝', '▞', '▚', '▙', '█',
    ];
    TABLE[(nibble & 0x0F) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_text_32x16() {
        let sam = Sam::default();
        let layout = decode_layout(0x00, &sam);
        assert_eq!(layout.mode, VdgMode::Text32x16);
        assert_eq!(layout.cols, 32);
        assert_eq!(layout.rows, 16);
    }

    #[test]
    fn sg4_mode_from_ff22() {
        let sam = Sam::default();
        let layout = decode_layout(0x40, &sam);
        assert_eq!(layout.mode, VdgMode::Semigraphics4);
        assert_eq!(layout.cols, 64);
    }

    #[test]
    fn graphics_layout_matches_vram_size() {
        let sam = Sam::default();
        let layout = decode_layout(0x80, &sam);
        assert_eq!(layout.mode, VdgMode::Graphics);
        assert_eq!(layout.cols, 32);
        assert_eq!(layout.rows, 192);
        assert_eq!(layout.cols as usize * layout.rows as usize, layout.bytes);
    }
}