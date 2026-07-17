use m6809_core::Emulator;
use serde::{Deserialize, Serialize};

use crate::sam::Sam;
use crate::vdg::{decode_layout, sg4_columns, VdgMode};
use crate::{current_kind, MachineKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrameDto {
    pub cols: u16,
    pub rows: u16,
    pub base_addr: u16,
    pub mode: String,
    pub cells: Vec<u8>,
    /// UTF-8 display glyphs (one string per row); used for SG4 / wide modes.
    pub rows_text: Vec<String>,
}

fn board_pointer<'a>(snapshot: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let nested = format!("/board/state{path}");
    snapshot
        .pointer(&nested)
        .or_else(|| snapshot.pointer(path))
}

pub fn video_frame(emu: &Emulator) -> Option<VideoFrameDto> {
    let kind = current_kind(emu);
    let snapshot = emu.memory.io.as_ref()?.snapshot();

    let (base, layout) = match kind {
        MachineKind::Coco2 => {
            let sam = parse_sam(&snapshot);
            let pia1_b = board_pointer(&snapshot, "/inner/pia1/orb")
                .and_then(|v| v.as_u64())
                .or_else(|| board_pointer(&snapshot, "/pia1/data_b").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u8;
            let layout = decode_layout(pia1_b, &sam);
            (sam.video_base(), layout)
        }
        MachineKind::Dragon32 => {
            let sam = parse_sam(&snapshot);
            let mode_byte = board_pointer(&snapshot, "/inner/vdg_mode")
                .and_then(|v| v.as_u64())
                .or_else(|| board_pointer(&snapshot, "/inner/pia1/orb").and_then(|v| v.as_u64()))
                .or_else(|| board_pointer(&snapshot, "/vdg_mode").and_then(|v| v.as_u64()))
                .unwrap_or(0) as u8;
            let layout = decode_layout(mode_byte, &sam);
            (sam.video_base(), layout)
        }
        MachineKind::Bare => return None,
    };

    let len = layout.bytes.min(0x2000);
    let mut cells = Vec::with_capacity(len);
    for i in 0..len {
        cells.push(emu.memory.read8(base.wrapping_add(i as u16)));
    }

    let rows_text = render_rows(&layout, &cells);

    Some(VideoFrameDto {
        cols: layout.cols,
        rows: layout.rows,
        base_addr: base,
        mode: format!("{:?}", layout.mode),
        cells,
        rows_text,
    })
}

fn parse_sam(snapshot: &serde_json::Value) -> Sam {
    parse_sam_with_default(snapshot, Sam::default())
}

fn parse_sam_with_default(snapshot: &serde_json::Value, default: Sam) -> Sam {
    if let Some(bits) = board_pointer(snapshot, "/inner/sam/bits")
        .or_else(|| board_pointer(snapshot, "/sam/bits"))
        .and_then(|v| v.as_u64())
    {
        return Sam::with_bits_and_base(bits as u16, default.base_addr());
    }
    default
}

fn row_bytes(cells: &[u8], row: usize, cols: usize) -> &[u8] {
    let start = row.saturating_mul(cols);
    if start >= cells.len() {
        return &[];
    }
    let end = (start + cols).min(cells.len());
    &cells[start..end]
}

fn render_rows(layout: &crate::vdg::VdgLayout, cells: &[u8]) -> Vec<String> {
    let cols = layout.cols as usize;
    let rows = layout.rows as usize;

    let rendered = match layout.mode {
        VdgMode::Semigraphics4 | VdgMode::Semigraphics6 => {
            (0..rows)
                .map(|row| {
                    row_bytes(cells, row, cols)
                        .iter()
                        .flat_map(|&byte| sg4_columns(byte))
                        .collect()
                })
                .collect()
        }
        _ => {
            (0..rows)
                .map(|row| {
                    row_bytes(cells, row, cols)
                        .iter()
                        .map(|&b| text_cell_char(b))
                        .collect()
                })
                .collect()
        }
    };

    if layout.mode == VdgMode::Graphics {
        trim_trailing_empty_rows(rendered)
    } else {
        rendered
    }
}

fn trim_trailing_empty_rows(mut rows: Vec<String>) -> Vec<String> {
    while rows.last().is_some_and(|row| row.trim().is_empty()) {
        rows.pop();
    }
    rows
}

/// Decode an MC6847 alphanumeric cell to a host glyph.
///
/// On CoCo/Dragon the VDG uses **bits 0–5 as the character index** and bit 6
/// only as invert/colour. Bit 7 selects semigraphics. Interpreting the byte as
/// ASCII (e.g. `$68` → `h`) is wrong: Dragon BASIC stores `(C) 1982` with bit 6
/// set on punctuation/digits (`$68` = normal `(`), which must render as `(`.
fn text_cell_char(value: u8) -> char {
    // Semigraphics (bit 7) — block approximation
    if value & 0x80 != 0 {
        return '·';
    }
    // Uninitialized RAM / cleared graphics (firmware CLS uses $60 for blank)
    if value == 0x00 || value == 0xff {
        return ' ';
    }
    let code = value & 0x3f;
    if code < 0x20 {
        // 0x00..0x1F → @ A B … Z [ \ ] ↑ ←
        char::from_u32((code + 0x40) as u32).unwrap_or('·')
    } else {
        // 0x20..0x3F → space !"#$%&'()*+,-./0-9:;<=>?
        char::from_u32(code as u32).unwrap_or('·')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vdg::{VdgLayout, VdgMode};

    #[test]
    fn render_rows_survives_layout_byte_mismatch() {
        let layout = VdgLayout {
            mode: VdgMode::Graphics,
            cols: 128,
            rows: 192,
            bytes: 6144,
        };
        let cells = vec![0u8; 6144];
        let rows = render_rows(&layout, &cells);
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn text_mode_keeps_full_grid() {
        let layout = VdgLayout {
            mode: VdgMode::Text32x16,
            cols: 32,
            rows: 16,
            bytes: 512,
        };
        let mut cells = vec![0x60; 512]; // CoCo blank
        cells[0] = b'H'; // $48 → VDG index $08 → 'H'
        cells[1] = b'I';
        let rows = render_rows(&layout, &cells);
        assert_eq!(rows.len(), 16);
        assert!(rows[0].starts_with("HI"));
    }

    #[test]
    fn vdg_bit6_is_invert_not_ascii() {
        // Dragon BASIC style: normal '(' / digits with bit 6 set
        assert_eq!(text_cell_char(0x68), '('); // not 'h'
        assert_eq!(text_cell_char(0x43), 'C');
        assert_eq!(text_cell_char(0x69), ')'); // not 'i'
        assert_eq!(text_cell_char(0x60), ' ');
        assert_eq!(text_cell_char(0x71), '1'); // not 'q'
        assert_eq!(text_cell_char(0x79), '9'); // not 'y'
        assert_eq!(text_cell_char(0x78), '8'); // not 'x'
        assert_eq!(text_cell_char(0x72), '2'); // not 'r'
        // Straight ASCII letters still work ($41 → index $01 → 'A')
        assert_eq!(text_cell_char(b'A'), 'A');
        assert_eq!(text_cell_char(b'6'), '6');
    }

    #[test]
    fn graphics_video_frame_does_not_panic() {
        let mut emu = m6809_core::Emulator::new();
        crate::apply_machine(&mut emu, crate::MachineKind::Coco2);
        // Select data register, then set VDG graphics bit on PIA1 port B.
        emu.memory.write8(0xFF23, 0x04);
        emu.memory.write8(0xFF22, 0x80);
        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "Graphics");
        assert_eq!(frame.cols, 32);
        assert_eq!(frame.rows_text.len(), 0);
    }
}