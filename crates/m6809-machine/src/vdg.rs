//! Motorola MC6847 VDG driven by an MC6883 SAM (TRS-80 CoCo 2 / Dragon 32).
//!
//! [`render_frame`] renders one complete field into a palette-indexed
//! framebuffer of [`FRAME_WIDTH`] × [`FRAME_HEIGHT`] pixels: the 256 × 192
//! active area with a [`BORDER_X`] px left/right and [`BORDER_Y`] px top/bottom
//! border (320 × 240, the usual 4:3 "visible TV area" crop). One framebuffer
//! pixel is one VDG pixel of the finest mode (RG6), so 32-byte modes map one
//! data bit to one pixel and 16-byte modes two.
//!
//! Video data is fetched the way the hardware does it: every scan line the
//! VDG reads 32 bytes (alphanumeric / semigraphics, CG2, CG3, CG6, RG6) or
//! 16 bytes (CG1, RG1, RG2, RG3) through the SAM video address counter
//! ([`SamVideoCounter`]), whose X/Y dividers and HSYNC clear behaviour are set
//! by the SAM V mode. A mismatched VDG / SAM pair is therefore rendered
//! faithfully, including the semigraphics 8/12/24 modes (alphanumeric VDG
//! with SAM V = 2/4/6): the VDG's own 12-line character row counter keeps
//! running, so each fetched row shows the matching slice of the character
//! cells. The VDG only ever sees RAM, never I/O or ROM overlays.

use serde::{Deserialize, Serialize};

/// Signals that feed the MC6847 VDG and the SAM video address generator,
/// provided by the board (`Coco2Machine::vdg_inputs` / `Dragon32Machine::vdg_inputs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VdgInputs {
    /// SAM V2..V0 video mode (0-7).
    pub sam_v: u8,
    /// SAM F6..F0 display offset; video base address = `sam_f as u16 * 512`.
    pub sam_f: u8,
    /// PIA1 port B levels seen by the VDG: bit 3 CSS, bit 4 GM0 (also INT/EXT),
    /// bit 5 GM1, bit 6 GM2, bit 7 A/G.
    pub vdg_ctrl: u8,
}

/// Width of the VDG active display area in pixels.
pub const ACTIVE_WIDTH: usize = 256;
/// Height of the VDG active display area in scan lines.
pub const ACTIVE_HEIGHT: usize = 192;
/// Border width left and right of the active area.
pub const BORDER_X: usize = 32;
/// Border height above and below the active area.
pub const BORDER_Y: usize = 24;
/// Framebuffer width (active area + left/right border).
pub const FRAME_WIDTH: usize = ACTIVE_WIDTH + 2 * BORDER_X;
/// Framebuffer height (active area + top/bottom border).
pub const FRAME_HEIGHT: usize = ACTIVE_HEIGHT + 2 * BORDER_Y;

/// Palette indices. 0-7 are the eight MC6847 colours in hardware order: CG
/// pixel values, SG4 colour bits and SG6 colour bits index them directly.
#[allow(dead_code)]
pub mod colour {
    pub const GREEN: u8 = 0;
    pub const YELLOW: u8 = 1;
    pub const BLUE: u8 = 2;
    pub const RED: u8 = 3;
    pub const BUFF: u8 = 4;
    pub const CYAN: u8 = 5;
    pub const MAGENTA: u8 = 6;
    pub const ORANGE: u8 = 7;
    pub const BLACK: u8 = 8;
    pub const ALPHA_DARK_GREEN: u8 = 9;
    pub const ALPHA_BRIGHT_GREEN: u8 = 10;
    pub const ALPHA_DARK_ORANGE: u8 = 11;
    pub const ALPHA_BRIGHT_ORANGE: u8 = 12;
}

use colour::*;

/// RGB value of every palette index (values as in MAME's `mc6847.cpp`).
pub const PALETTE: [[u8; 3]; 13] = [
    [0x30, 0xD2, 0x00], // green
    [0xC1, 0xE5, 0x00], // yellow
    [0x4C, 0x3A, 0xB4], // blue
    [0x9A, 0x32, 0x36], // red
    [0xBF, 0xC8, 0xAD], // buff
    [0x41, 0xAF, 0x71], // cyan
    [0xC8, 0x4E, 0xF0], // magenta
    [0xD4, 0x7F, 0x00], // orange
    [0x26, 0x30, 0x16], // black
    [0x00, 0x7C, 0x00], // alphanumeric dark green
    [0x30, 0xD2, 0x00], // alphanumeric bright green
    [0x6B, 0x27, 0x00], // alphanumeric dark orange
    [0xFF, 0xB7, 0x00], // alphanumeric bright orange
];

/// MC6847 internal character generator: 64 characters, 5 × 7 dots each
/// (`#` = dot). Index = VRAM byte & $3F.
const GLYPHS: [[&str; 7]; 64] = [
    [".###.", "#...#", "....#", ".##.#", "#.#.#", "#.#.#", ".###."], // $00 @
    ["..#..", ".#.#.", "#...#", "#...#", "#####", "#...#", "#...#"], // $01 A
    ["####.", ".#..#", ".#..#", ".###.", ".#..#", ".#..#", "####."], // $02 B
    [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."], // $03 C
    ["####.", ".#..#", ".#..#", ".#..#", ".#..#", ".#..#", "####."], // $04 D
    ["#####", "#....", "#....", "####.", "#....", "#....", "#####"], // $05 E
    ["#####", "#....", "#....", "####.", "#....", "#....", "#...."], // $06 F
    [".####", "#....", "#....", "#..##", "#...#", "#...#", ".####"], // $07 G
    ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"], // $08 H
    [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."], // $09 I
    ["....#", "....#", "....#", "....#", "#...#", "#...#", ".###."], // $0A J
    ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"], // $0B K
    ["#....", "#....", "#....", "#....", "#....", "#....", "#####"], // $0C L
    ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"], // $0D M
    ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"], // $0E N
    ["#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#####"], // $0F O
    ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."], // $10 P
    [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"], // $11 Q
    ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"], // $12 R
    [".###.", "#...#", ".#...", "..#..", "...#.", "#...#", ".###."], // $13 S
    ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."], // $14 T
    ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."], // $15 U
    ["#...#", "#...#", "#...#", ".#.#.", ".#.#.", "..#..", "..#.."], // $16 V
    ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"], // $17 W
    ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"], // $18 X
    ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."], // $19 Y
    ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"], // $1A Z
    ["###..", "#....", "#....", "#....", "#....", "#....", "###.."], // $1B [
    ["#....", "#....", ".#...", "..#..", "...#.", "....#", "....#"], // $1C backslash
    ["..###", "....#", "....#", "....#", "....#", "....#", "..###"], // $1D ]
    ["..#..", ".###.", "#.#.#", "..#..", "..#..", "..#..", "..#.."], // $1E up arrow
    [".....", "..#..", ".#...", "#####", ".#...", "..#..", "....."], // $1F left arrow
    [".....", ".....", ".....", ".....", ".....", ".....", "....."], // $20 space
    ["..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#.."], // $21 !
    [".#.#.", ".#.#.", ".#.#.", ".....", ".....", ".....", "....."], // $22 "
    [".#.#.", ".#.#.", "##.##", ".....", "##.##", ".#.#.", ".#.#."], // $23 #
    ["..#..", ".####", "#....", ".###.", "....#", "####.", "..#.."], // $24 $
    ["##..#", "##..#", "...#.", "..#..", ".#...", "#..##", "#..##"], // $25 %
    [".#...", "#.#..", "#.#..", ".#...", "#.#.#", "#..#.", ".##.#"], // $26 &
    [".##..", ".##..", ".##..", ".....", ".....", ".....", "....."], // $27 '
    ["..#..", ".#...", "#....", "#....", "#....", ".#...", "..#.."], // $28 (
    ["..#..", "...#.", "....#", "....#", "....#", "...#.", "..#.."], // $29 )
    [".....", "..#..", ".###.", "#####", ".###.", "..#..", "....."], // $2A *
    [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."], // $2B +
    [".....", ".....", ".....", "##...", "##...", ".#...", "#...."], // $2C ,
    [".....", ".....", ".....", "#####", ".....", ".....", "....."], // $2D -
    [".....", ".....", ".....", ".....", ".....", "##...", "##..."], // $2E .
    ["....#", "....#", "...#.", "..#..", ".#...", "#....", "#...."], // $2F /
    [".##..", "#..#.", "#..#.", "#..#.", "#..#.", "#..#.", ".##.."], // $30 0
    ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."], // $31 1
    [".###.", "#...#", "....#", ".###.", "#....", "#....", "#####"], // $32 2
    [".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."], // $33 3
    ["...#.", "..##.", ".#.#.", "#####", "...#.", "...#.", "...#."], // $34 4
    ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."], // $35 5
    [".###.", "#....", "#....", "####.", "#...#", "#...#", ".###."], // $36 6
    ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#...."], // $37 7
    [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."], // $38 8
    [".###.", "#...#", "#...#", ".####", "....#", "....#", ".###."], // $39 9
    [".....", ".##..", ".##..", ".....", ".##..", ".##..", "....."], // $3A :
    [".##..", ".##..", ".....", ".##..", ".##..", "..#..", ".#..."], // $3B ;
    ["...#.", "..#..", ".#...", "#....", ".#...", "..#..", "...#."], // $3C <
    [".....", ".....", "#####", ".....", "#####", ".....", "....."], // $3D =
    [".#...", "..#..", "...#.", "....#", "...#.", "..#..", ".#..."], // $3E >
    [".##..", "#..#.", "...#.", "..#..", "..#..", ".....", "..#.."], // $3F ?
];

/// First line of the 12-line character cell that holds glyph dots
/// (three blank lines above, two below).
const GLYPH_TOP: usize = 3;
/// Leftmost of the eight cell columns that holds glyph dots (two blank
/// columns on the left, one on the right).
const GLYPH_LEFT: u32 = 2;

/// Character generator expanded to the 8 × 12 cell: `FONT[code][line]` is the
/// 8-pixel pattern of that cell line, bit 7 = leftmost pixel.
static FONT: [[u8; 12]; 64] = build_font();

const fn build_font() -> [[u8; 12]; 64] {
    let mut font = [[0u8; 12]; 64];
    let mut code = 0;
    while code < 64 {
        let mut row = 0;
        while row < 7 {
            let dots = GLYPHS[code][row].as_bytes();
            let mut pattern = 0u8;
            let mut col = 0;
            while col < 5 {
                if dots[col] == b'#' {
                    pattern |= 0x80 >> (GLYPH_LEFT + col as u32);
                }
                col += 1;
            }
            font[code][GLYPH_TOP + row] = pattern;
            row += 1;
        }
        code += 1;
    }
    font
}

/// 8-pixel pattern of one line (0-11) of an internal character cell.
pub fn glyph_line(code: u8, line: usize) -> u8 {
    FONT[(code & 0x3F) as usize][line % 12]
}

/// Host character for each internal character code ($00-$3F).
static ALPHA_CHARS: [char; 64] = [
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '↑', '←', //
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
];

/// Unicode quadrant blocks indexed by the SG4 element bits
/// (bit 3 top-left, bit 2 top-right, bit 1 bottom-left, bit 0 bottom-right).
static SG4_CHARS: [char; 16] = [
    ' ', '▗', '▖', '▄', '▝', '▐', '▞', '▟', '▘', '▚', '▌', '▙', '▀', '▜', '▛', '█',
];

/// Host glyph for one VRAM byte of an alphanumeric screen: the internal
/// character (bits 0-5; bit 6 only inverts it), or a block graphic for
/// semigraphics bytes (bit 7 set): SG4 with `ext == false`, SG6 with
/// `ext == true` (INT/EXT = GM0 on the CoCo / Dragon).
pub fn cell_char(byte: u8, ext: bool) -> char {
    if byte & 0x80 == 0 {
        return ALPHA_CHARS[(byte & 0x3F) as usize];
    }
    if !ext {
        return SG4_CHARS[(byte & 0x0F) as usize];
    }
    // SG6: bit 5 top-left, 4 top-right, 3 middle-left, 2 middle-right,
    // 1 bottom-left, 0 bottom-right → Unicode sextant cells 1..6.
    let mut cells = 0u32;
    for cell in 0..6 {
        if byte & (0x20 >> cell) != 0 {
            cells |= 1 << cell;
        }
    }
    match cells {
        0 => ' ',
        21 => '▌',
        42 => '▐',
        63 => '█',
        _ => {
            // U+1FB00.. lists the sextants in binary order, minus the two
            // half blocks that already exist.
            let skipped = u32::from(cells > 21) + u32::from(cells > 42);
            char::from_u32(0x1FB00 + cells - 1 - skipped).unwrap_or('▒')
        }
    }
}

/// MC6847 control inputs as wired on the CoCo / Dragon (PIA1 PB3-PB7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdgControl {
    /// A/G: graphics mode.
    pub ag: bool,
    /// GM2..GM0 graphics mode (GM0 doubles as INT/EXT).
    pub gm: u8,
    /// CSS colour set select.
    pub css: bool,
}

/// Graphics mode names indexed by GM2..GM0.
const GRAPHICS_NAMES: [&str; 8] = ["CG1", "RG1", "CG2", "RG2", "CG3", "RG3", "CG6", "RG6"];
/// SAM V mode that belongs to each graphics mode (MC6883 data sheet).
const NATURAL_SAM_V: [u8; 8] = [1, 1, 2, 3, 4, 5, 6, 6];

impl VdgControl {
    pub fn from_pia(vdg_ctrl: u8) -> Self {
        Self {
            ag: vdg_ctrl & 0x80 != 0,
            gm: (vdg_ctrl >> 4) & 0x07,
            css: vdg_ctrl & 0x08 != 0,
        }
    }

    /// INT/EXT input; A/S comes from D7, INV from D6 of each VRAM byte.
    pub fn int_ext(self) -> bool {
        self.gm & 1 != 0
    }

    /// Bytes the VDG fetches per scan line.
    pub fn bytes_per_line(self) -> usize {
        if self.ag && matches!(self.gm, 0 | 1 | 3 | 5) {
            16
        } else {
            32
        }
    }

    /// Border colour: black around alphanumerics / semigraphics, green or
    /// buff (by CSS) around graphics.
    pub fn border(self) -> u8 {
        match (self.ag, self.css) {
            (false, _) => BLACK,
            (true, false) => GREEN,
            (true, true) => BUFF,
        }
    }

    /// Colour of each of the eight bit positions of one VRAM byte on the given
    /// line (0-11) of the VDG character row counter.
    fn decode(self, data: u8, line: usize) -> [u8; 8] {
        if self.ag {
            if self.gm & 1 != 0 {
                // Resolution graphics: 1 bpp.
                let fg = if self.css { BUFF } else { GREEN };
                return bits(data, fg, BLACK);
            }
            // Colour graphics: 2 bpp.
            let set = if self.css { BUFF } else { GREEN };
            let mut out = [0u8; 8];
            for pair in 0..4 {
                let colour = set + ((data >> (6 - 2 * pair)) & 3);
                out[2 * pair] = colour;
                out[2 * pair + 1] = colour;
            }
            return out;
        }
        if data & 0x80 != 0 {
            if self.int_ext() {
                // Semigraphics 6: 2 × 3 elements, colour from D7 D6 and CSS.
                let colour = (if self.css { BUFF } else { GREEN }) + ((data >> 6) & 3);
                let pair = match line {
                    0..=3 => data >> 4,
                    4..=7 => data >> 2,
                    _ => data,
                };
                return halves(pair, colour);
            }
            // Semigraphics 4: 2 × 2 elements, colour from bits 6-4.
            let colour = (data >> 4) & 7;
            let pair = if line < 6 { data >> 2 } else { data };
            return halves(pair, colour);
        }
        // Alphanumerics. Without an external character ROM the EXT pattern is
        // the byte itself (as XRoar renders it). D6 drives INV.
        let mut pattern = if self.int_ext() { data } else { glyph_line(data, line) };
        if data & 0x40 != 0 {
            pattern = !pattern;
        }
        let (fg, bg) = if self.css {
            (ALPHA_BRIGHT_ORANGE, ALPHA_DARK_ORANGE)
        } else {
            (ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN)
        };
        bits(pattern, fg, bg)
    }
}

fn bits(pattern: u8, fg: u8, bg: u8) -> [u8; 8] {
    let mut out = [bg; 8];
    for (i, px) in out.iter_mut().enumerate() {
        if pattern & (0x80 >> i) != 0 {
            *px = fg;
        }
    }
    out
}

/// Semigraphics element pair: bit 1 lights the left half, bit 0 the right
/// half; unlit elements are black.
fn halves(pair: u8, colour: u8) -> [u8; 8] {
    let left = if pair & 2 != 0 { colour } else { BLACK };
    let right = if pair & 1 != 0 { colour } else { BLACK };
    [left, left, left, left, right, right, right, right]
}

/// SAM X divider per V mode (B4 toggles every n carries out of B3).
const SAM_X_DIV: [u8; 8] = [1, 3, 1, 2, 1, 1, 1, 1];
/// SAM Y divider per V mode (B15-B5 advance every n carries out of B4).
const SAM_Y_DIV: [u8; 8] = [12, 1, 3, 1, 2, 1, 1, 1];

/// MC6883 video address counter (B15-B0) with its X / Y dividers.
///
/// The VDG clocks B0-B3 once per byte; the carry out of B3 goes through the
/// X divider into B4, the carry out of B4 through the Y divider into B5-B15.
/// On HSYNC the SAM clears B1-B4 (V = 0, 2, 4, 6) or B1-B3 (V = 1, 3, 5);
/// clearing a set bit propagates like a carry. V = 7 (DMA) never clears.
/// The counter is reloaded from F at field sync.
#[derive(Debug, Clone)]
pub struct SamVideoCounter {
    v: usize,
    addr: u16,
    xdiv: u8,
    ydiv: u8,
}

impl SamVideoCounter {
    /// Counter state at the start of a field.
    pub fn new(sam_v: u8, sam_f: u8) -> Self {
        Self {
            v: usize::from(sam_v & 7),
            addr: u16::from(sam_f & 0x7F) << 9,
            xdiv: 0,
            ydiv: 0,
        }
    }

    /// Current video address.
    pub fn addr(&self) -> u16 {
        self.addr
    }

    /// Address of the byte the VDG reads now; advances the counter past it.
    pub fn fetch(&mut self) -> u16 {
        let addr = self.addr;
        let low = (self.addr & 0x000F) + 1;
        self.addr = (self.addr & !0x000F) | (low & 0x000F);
        if low > 0x000F {
            self.carry_b3();
        }
        addr
    }

    /// Falling edge of HSYNC at the end of a scan line.
    pub fn hsync(&mut self) {
        match self.v {
            1 | 3 | 5 => {
                let carry = self.addr & 0x0008 != 0;
                self.addr &= !0x000F;
                if carry {
                    self.carry_b3();
                }
            }
            0 | 2 | 4 | 6 => {
                let carry = self.addr & 0x0010 != 0;
                self.addr &= !0x001F;
                if carry {
                    self.carry_b4();
                }
            }
            _ => {}
        }
    }

    fn carry_b3(&mut self) {
        self.xdiv += 1;
        if self.xdiv >= SAM_X_DIV[self.v] {
            self.xdiv = 0;
            self.addr ^= 0x0010;
            if self.addr & 0x0010 == 0 {
                self.carry_b4();
            }
        }
    }

    fn carry_b4(&mut self) {
        self.ydiv += 1;
        if self.ydiv >= SAM_Y_DIV[self.v] {
            self.ydiv = 0;
            self.addr = self.addr.wrapping_add(0x0020);
        }
    }
}

/// One rendered field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    /// `FRAME_WIDTH * FRAME_HEIGHT` palette indices, row-major.
    pub pixels: Vec<u8>,
    /// Display mode ("Text32x16", "SG6", "SG8", "SG12", "SG24", "CG1".."RG6";
    /// a SAM mode that does not belong to the VDG mode is appended as "/V<n>").
    pub mode: String,
    /// Video RAM base address (SAM F × 512).
    pub base_addr: u16,
    /// Bytes of video RAM the field spans, counted from `base_addr`.
    pub vram_bytes: u16,
    /// Logical resolution of the mode: character cells for "Text32x16",
    /// elements / pixels otherwise.
    pub cols: u16,
    pub rows: u16,
}

/// Render one field from RAM.
pub fn render_frame(ram: &[u8; 0x10000], inputs: VdgInputs) -> RenderedFrame {
    let ctrl = VdgControl::from_pia(inputs.vdg_ctrl);
    let sam_v = inputs.sam_v & 7;
    let mut counter = SamVideoCounter::new(sam_v, inputs.sam_f);
    let base_addr = counter.addr();
    let bytes_per_line = ctrl.bytes_per_line();
    let px_per_byte = ACTIVE_WIDTH / bytes_per_line;

    let mut pixels = vec![ctrl.border(); FRAME_WIDTH * FRAME_HEIGHT];
    let mut span: u16 = 0;
    let mut distinct_rows: u16 = 0;
    let mut prev_row_start: Option<u16> = None;

    for line in 0..ACTIVE_HEIGHT {
        // The VDG row counter restarts at the top of the active area and
        // counts 0-11 regardless of the SAM mode.
        let char_line = line % 12;
        let start = (BORDER_Y + line) * FRAME_WIDTH + BORDER_X;
        let out = &mut pixels[start..start + ACTIVE_WIDTH];
        for (i, chunk) in out.chunks_exact_mut(px_per_byte).enumerate() {
            let addr = counter.fetch();
            if i == 0 && prev_row_start != Some(addr) {
                prev_row_start = Some(addr);
                distinct_rows += 1;
            }
            span = span.max(addr.wrapping_sub(base_addr));
            let colours = ctrl.decode(ram[usize::from(addr)], char_line);
            if px_per_byte == 8 {
                chunk.copy_from_slice(&colours);
            } else {
                for (pair, &colour) in chunk.chunks_exact_mut(2).zip(colours.iter()) {
                    pair[0] = colour;
                    pair[1] = colour;
                }
            }
        }
        counter.hsync();
    }

    let (mode, cols, rows) = describe_mode(ctrl, sam_v, distinct_rows);
    RenderedFrame {
        pixels,
        mode,
        base_addr,
        vram_bytes: span.saturating_add(1),
        cols,
        rows,
    }
}

/// Mode name and logical resolution. `distinct_rows` is the number of
/// different VRAM rows the field displayed.
fn describe_mode(ctrl: VdgControl, sam_v: u8, distinct_rows: u16) -> (String, u16, u16) {
    if !ctrl.ag {
        return match (sam_v, ctrl.int_ext()) {
            (0, false) => ("Text32x16".into(), 32, 16),
            (0, true) => ("SG6".into(), 64, 48),
            (2, _) => ("SG8".into(), 64, 64),
            (4, _) => ("SG12".into(), 64, 96),
            (6, _) => ("SG24".into(), 64, 192),
            (v, _) => (format!("Alpha/V{v}"), 32, distinct_rows),
        };
    }
    let gm = usize::from(ctrl.gm);
    let bits_per_pixel = if ctrl.gm & 1 != 0 { 1 } else { 2 };
    let cols = (ctrl.bytes_per_line() * 8 / bits_per_pixel) as u16;
    let name = if sam_v == NATURAL_SAM_V[gm] {
        GRAPHICS_NAMES[gm].to_string()
    } else {
        format!("{}/V{sam_v}", GRAPHICS_NAMES[gm])
    };
    (name, cols, distinct_rows)
}

/// Text decode of an alphanumeric screen (A/G = 0, SAM V = 0): 16 rows of 32
/// characters starting at the video base. Empty for every other mode.
pub fn text_rows(ram: &[u8; 0x10000], inputs: VdgInputs) -> Vec<String> {
    let ctrl = VdgControl::from_pia(inputs.vdg_ctrl);
    if ctrl.ag || inputs.sam_v & 7 != 0 {
        return Vec::new();
    }
    let base = u16::from(inputs.sam_f & 0x7F) << 9;
    (0..16u16)
        .map(|row| {
            (0..32u16)
                .map(|col| {
                    let addr = base.wrapping_add(row * 32 + col);
                    cell_char(ram[usize::from(addr)], ctrl.int_ext())
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_G: u8 = 0x80;
    const GM0: u8 = 0x10;
    const CSS: u8 = 0x08;

    fn ram() -> Box<[u8; 0x10000]> {
        Box::new([0u8; 0x10000])
    }

    fn inputs(sam_v: u8, sam_f: u8, vdg_ctrl: u8) -> VdgInputs {
        VdgInputs {
            sam_v,
            sam_f,
            vdg_ctrl,
        }
    }

    /// Graphics mode control byte for GM2..GM0.
    fn gfx(gm: u8) -> u8 {
        A_G | (gm << 4)
    }

    /// Pixel at active-area coordinates.
    fn px(frame: &RenderedFrame, x: usize, y: usize) -> u8 {
        frame.pixels[(BORDER_Y + y) * FRAME_WIDTH + BORDER_X + x]
    }

    /// The eight pixels of one text cell line.
    fn cell_line(frame: &RenderedFrame, col: usize, y: usize) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, p) in out.iter_mut().enumerate() {
            *p = px(frame, col * 8 + i, y);
        }
        out
    }

    fn expect_pattern(pattern: u8, fg: u8, bg: u8) -> [u8; 8] {
        let mut out = [bg; 8];
        for (i, p) in out.iter_mut().enumerate() {
            if pattern & (0x80 >> i) != 0 {
                *p = fg;
            }
        }
        out
    }

    #[test]
    fn font_rows_are_centred_in_the_cell() {
        // '@' as drawn by the MC6847.
        let at = [0x00, 0x00, 0x00, 0x1C, 0x22, 0x02, 0x1A, 0x2A, 0x2A, 0x1C, 0x00, 0x00];
        for (line, &row) in at.iter().enumerate() {
            assert_eq!(glyph_line(0x00, line), row, "@ line {line}");
        }
        for code in 0..64u8 {
            for line in [0, 1, 2, 10, 11] {
                assert_eq!(glyph_line(code, line), 0, "code {code:02X} line {line} blank");
            }
            for line in 0..12 {
                assert_eq!(glyph_line(code, line) & 0xC1, 0, "dots stay in columns 2-6");
            }
        }
        assert!((0..12).all(|l| glyph_line(0x20, l) == 0), "space is empty");
    }

    #[test]
    fn up_and_left_arrows() {
        let up: Vec<u8> = (3..10).map(|l| glyph_line(0x1E, l)).collect();
        assert_eq!(up, [0x08, 0x1C, 0x2A, 0x08, 0x08, 0x08, 0x08]);
        let left: Vec<u8> = (3..10).map(|l| glyph_line(0x1F, l)).collect();
        assert_eq!(left, [0x00, 0x08, 0x10, 0x3E, 0x10, 0x08, 0x00]);
        assert_eq!(cell_char(0x1E, false), '↑');
        assert_eq!(cell_char(0x5F, false), '←');
    }

    #[test]
    fn alpha_normal_text_is_dark_on_bright() {
        // BASIC's normal text has D6 set: INV → dark character on bright background.
        let mut mem = ram();
        mem[0x0400] = 0x41; // 'A'
        let frame = render_frame(&mem, inputs(0, 2, 0x00));
        assert_eq!(frame.mode, "Text32x16");
        for line in 0..12 {
            let expect = expect_pattern(
                !glyph_line(0x01, line),
                ALPHA_BRIGHT_GREEN,
                ALPHA_DARK_GREEN,
            );
            assert_eq!(cell_line(&frame, 0, line), expect, "line {line}");
        }
        assert_eq!(px(&frame, 0, 0), ALPHA_BRIGHT_GREEN, "cell background is bright");
        assert_eq!(px(&frame, 4, 3), ALPHA_DARK_GREEN, "apex of the A is dark");
    }

    #[test]
    fn alpha_inverse_text_is_bright_on_dark() {
        // D6 clear (BASIC's lower case): bright character on dark background.
        let mut mem = ram();
        mem[0x0400] = 0x01; // 'A', inverse
        mem[0x0401] = 0x00; // '@' — a character, not blank
        let frame = render_frame(&mem, inputs(0, 2, 0x00));
        for line in 0..12 {
            let a = expect_pattern(glyph_line(0x01, line), ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN);
            assert_eq!(cell_line(&frame, 0, line), a, "A line {line}");
            let at = expect_pattern(glyph_line(0x00, line), ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN);
            assert_eq!(cell_line(&frame, 1, line), at, "@ line {line}");
        }
        assert_eq!(px(&frame, 0, 0), ALPHA_DARK_GREEN);
        assert_eq!(px(&frame, 4, 3), ALPHA_BRIGHT_GREEN);
    }

    #[test]
    fn css_selects_orange_text() {
        let mut mem = ram();
        mem[0x0400] = 0x41;
        mem[0x0401] = 0x01;
        let frame = render_frame(&mem, inputs(0, 2, CSS));
        assert_eq!(frame.mode, "Text32x16");
        assert_eq!(px(&frame, 0, 0), ALPHA_BRIGHT_ORANGE);
        assert_eq!(px(&frame, 4, 3), ALPHA_DARK_ORANGE);
        assert_eq!(px(&frame, 8, 0), ALPHA_DARK_ORANGE);
        assert_eq!(px(&frame, 12, 3), ALPHA_BRIGHT_ORANGE);
        assert_eq!(frame.pixels[0], BLACK, "text border stays black with CSS");
    }

    #[test]
    fn gm1_gm2_are_ignored_in_alpha_mode() {
        let mut mem = ram();
        for (i, b) in mem[0x0400..0x0600].iter_mut().enumerate() {
            *b = i as u8;
        }
        let plain = render_frame(&mem, inputs(0, 2, 0x00));
        let gm21 = render_frame(&mem, inputs(0, 2, 0x60));
        assert_eq!(plain, gm21);
    }

    #[test]
    fn sg4_colours_and_quadrants() {
        let mut mem = ram();
        // Colour 1 (yellow), top-left + bottom-right lit.
        mem[0x0400] = 0x80 | (1 << 4) | 0b1001;
        // Colour 7 (orange), top-right + bottom-left lit.
        mem[0x0401] = 0x80 | (7 << 4) | 0b0110;
        // CLS 0 block: nothing lit.
        mem[0x0402] = 0x80;
        let frame = render_frame(&mem, inputs(0, 2, 0x00));
        assert_eq!(frame.mode, "Text32x16");
        for y in 0..12 {
            let top = y < 6;
            let a = cell_line(&frame, 0, y);
            let b = cell_line(&frame, 1, y);
            let (al, ar) = if top { (YELLOW, BLACK) } else { (BLACK, YELLOW) };
            let (bl, br) = if top { (BLACK, ORANGE) } else { (ORANGE, BLACK) };
            assert_eq!(a, [al, al, al, al, ar, ar, ar, ar], "cell 0 line {y}");
            assert_eq!(b, [bl, bl, bl, bl, br, br, br, br], "cell 1 line {y}");
            assert_eq!(cell_line(&frame, 2, y), [BLACK; 8]);
        }
        // All eight colours, independent of CSS.
        for colour in 0..8u8 {
            mem[0x0400] = 0x8F | (colour << 4);
            for ctrl in [0x00, CSS] {
                let frame = render_frame(&mem, inputs(0, 2, ctrl));
                assert_eq!(px(&frame, 0, 0), colour);
            }
        }
    }

    #[test]
    fn sg6_elements_and_colours() {
        let mut mem = ram();
        // D7 D6 = 11 → colour 3 (red, CSS 0) / 7 (orange, CSS 1).
        // Elements: top-left, middle-right, bottom-left.
        mem[0x0400] = 0xC0 | 0b10_01_10;
        // D7 D6 = 10 → colour 2 (blue) / 6 (magenta): all elements.
        mem[0x0401] = 0xBF;
        for (ctrl, c0, c1) in [(GM0, RED, BLUE), (GM0 | CSS, ORANGE, MAGENTA)] {
            let frame = render_frame(&mem, inputs(0, 2, ctrl));
            assert_eq!(frame.mode, "SG6");
            assert_eq!((frame.cols, frame.rows), (64, 48));
            for y in 0..12 {
                let (l, r) = match y {
                    0..=3 => (c0, BLACK),
                    4..=7 => (BLACK, c0),
                    _ => (c0, BLACK),
                };
                assert_eq!(cell_line(&frame, 0, y), [l, l, l, l, r, r, r, r], "line {y}");
                assert_eq!(cell_line(&frame, 1, y), [c1; 8]);
            }
        }
    }

    #[test]
    fn external_characters_show_the_byte_pattern() {
        // INT/EXT = 1 with no external character ROM fitted.
        let mut mem = ram();
        mem[0x0400] = 0x25; // D6 = 0: pattern as is
        mem[0x0401] = 0x65; // D6 = 1: inverted
        let frame = render_frame(&mem, inputs(0, 2, GM0));
        for y in 0..12 {
            assert_eq!(
                cell_line(&frame, 0, y),
                expect_pattern(0x25, ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN)
            );
            assert_eq!(
                cell_line(&frame, 1, y),
                expect_pattern(!0x65, ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN)
            );
        }
    }

    #[test]
    fn alpha_border_black_graphics_border_green_or_buff() {
        let mem = ram();
        let cases = [
            (0x00, BLACK),
            (CSS, BLACK),
            (gfx(7), GREEN),
            (gfx(7) | CSS, BUFF),
            (gfx(0), GREEN),
            (gfx(0) | CSS, BUFF),
        ];
        for (ctrl, border) in cases {
            let frame = render_frame(&mem, inputs(6, 2, ctrl));
            assert_eq!(frame.pixels.len(), FRAME_WIDTH * FRAME_HEIGHT);
            let corners = [
                0,
                FRAME_WIDTH - 1,
                (FRAME_HEIGHT - 1) * FRAME_WIDTH,
                FRAME_WIDTH * FRAME_HEIGHT - 1,
                BORDER_Y * FRAME_WIDTH + BORDER_X - 1,
                (BORDER_Y + ACTIVE_HEIGHT) * FRAME_WIDTH - 1,
            ];
            for i in corners {
                assert_eq!(frame.pixels[i], border, "ctrl {ctrl:02X} pixel {i}");
            }
        }
    }

    #[test]
    fn base_address_comes_from_sam_f() {
        let mut mem = ram();
        mem[0x0E00] = 0x58; // 'X' at F = 7
        let frame = render_frame(&mem, inputs(0, 7, 0x00));
        assert_eq!(frame.base_addr, 0x0E00);
        assert_eq!(frame.vram_bytes, 512);
        assert_eq!(
            cell_line(&frame, 0, 3),
            expect_pattern(!glyph_line(0x18, 3), ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN)
        );
        assert_eq!(text_rows(&mem, inputs(0, 7, 0x00))[0].chars().next(), Some('X'));
        assert_eq!(render_frame(&mem, inputs(0, 0x7F, 0x00)).base_addr, 0xFE00);
    }

    /// First VRAM address fetched on each of the 192 lines.
    fn line_starts(sam_v: u8, bytes_per_line: usize) -> Vec<u16> {
        let mut counter = SamVideoCounter::new(sam_v, 2);
        (0..ACTIVE_HEIGHT)
            .map(|_| {
                let start = counter.fetch();
                for _ in 1..bytes_per_line {
                    counter.fetch();
                }
                counter.hsync();
                start
            })
            .collect()
    }

    #[test]
    fn sam_rows_repeat_per_v_mode() {
        // (V, VDG bytes per line, lines per row, row stride)
        let cases = [
            (0, 32, 12, 32),
            (1, 16, 3, 16),
            (2, 32, 3, 32),
            (3, 16, 2, 16),
            (4, 32, 2, 32),
            (5, 16, 1, 16),
            (6, 32, 1, 32),
            (7, 32, 1, 32),
            (7, 16, 1, 16),
        ];
        for (v, bpl, lines, stride) in cases {
            let starts = line_starts(v, bpl);
            for (line, &start) in starts.iter().enumerate() {
                let row = (line / lines) as u16;
                assert_eq!(start, 0x0400 + row * stride, "V={v} line {line}");
            }
        }
    }

    #[test]
    fn sam_mismatched_modes_follow_the_dividers() {
        // 16-byte VDG mode with a B1-B4 clearing SAM mode: clearing B4 on
        // HSYNC carries into the Y divider, so rows still advance by 32 bytes.
        let starts = line_starts(0, 16);
        assert_eq!(starts[11], 0x0400);
        assert_eq!(starts[12], 0x0420);
        // 32-byte VDG mode with the X÷3 SAM mode: each line reads its 16-byte
        // half twice, B4 toggles every third half-line.
        let mut counter = SamVideoCounter::new(1, 2);
        let line0: Vec<u16> = (0..32).map(|_| counter.fetch()).collect();
        assert_eq!(line0[0], 0x0400);
        assert_eq!(line0[16], 0x0400, "second half repeats the first");
        counter.hsync();
        let line1: Vec<u16> = (0..32).map(|_| counter.fetch()).collect();
        assert_eq!(line1[0], 0x0400);
        assert_eq!(line1[16], 0x0410);
    }

    #[test]
    fn semigraphics_8_12_24_slice_the_character_cell() {
        for (v, name, lines_per_row, rows) in [(2, "SG8", 3, 64), (4, "SG12", 2, 96), (6, "SG24", 1, 192)] {
            let mut mem = ram();
            // Every VRAM row holds an SG4 byte with all elements lit, colour
            // = row number mod 8.
            for row in 0..rows {
                mem[0x0400 + row * 32] = 0x8F | (((row % 8) as u8) << 4);
            }
            let frame = render_frame(&mem, inputs(v, 2, 0x00));
            assert_eq!(frame.mode, name);
            assert_eq!((frame.cols, frame.rows as usize), (64, rows));
            assert_eq!(frame.vram_bytes as usize, rows * 32);
            for line in 0..ACTIVE_HEIGHT {
                let row = line / lines_per_row;
                assert_eq!(px(&frame, 0, line), (row % 8) as u8, "{name} line {line}");
            }
            // Elements follow the VDG's 12-line counter, not the SAM row:
            // top pair on cell lines 0-5, bottom pair on 6-11.
            let mut mem = ram();
            for row in 0..rows {
                mem[0x0400 + row * 32] = 0x80 | (3 << 4) | 0b1000; // top-left only
            }
            let frame = render_frame(&mem, inputs(v, 2, 0x00));
            for line in 0..ACTIVE_HEIGHT {
                let lit = line % 12 < 6;
                assert_eq!(px(&frame, 0, line), if lit { RED } else { BLACK }, "{name} line {line}");
            }
        }
    }

    #[test]
    fn sg8_text_shows_a_slice_of_each_glyph() {
        let mut mem = ram();
        for row in 0..64 {
            mem[0x0400 + row * 32] = 0x01; // inverse 'A' in every row
        }
        let frame = render_frame(&mem, inputs(2, 2, 0x00));
        for line in 0..ACTIVE_HEIGHT {
            let expect = expect_pattern(glyph_line(0x01, line % 12), ALPHA_BRIGHT_GREEN, ALPHA_DARK_GREEN);
            assert_eq!(cell_line(&frame, 0, line), expect, "line {line}");
        }
    }

    #[test]
    fn graphics_modes_sizes_and_names() {
        // (GM, name, cols, rows, bytes)
        let cases = [
            (0u8, "CG1", 64u16, 64u16, 1024u16),
            (1, "RG1", 128, 64, 1024),
            (2, "CG2", 128, 64, 2048),
            (3, "RG2", 128, 96, 1536),
            (4, "CG3", 128, 96, 3072),
            (5, "RG3", 128, 192, 3072),
            (6, "CG6", 128, 192, 6144),
            (7, "RG6", 256, 192, 6144),
        ];
        let mem = ram();
        for (gm, name, cols, rows, bytes) in cases {
            let v = NATURAL_SAM_V[gm as usize];
            let frame = render_frame(&mem, inputs(v, 2, gfx(gm)));
            assert_eq!(frame.mode, name);
            assert_eq!((frame.cols, frame.rows), (cols, rows), "{name}");
            assert_eq!(frame.vram_bytes, bytes, "{name}");
            assert!(text_rows(&mem, inputs(v, 2, gfx(gm))).is_empty());
        }
        let odd = render_frame(&mem, inputs(0, 2, gfx(7)));
        assert_eq!(odd.mode, "RG6/V0");
        assert_eq!(odd.rows, 16);
        // PMODE 0 is RG2 (128×96, 16 bytes per row, 1536 bytes), not text.
        let pmode0 = render_frame(&mem, inputs(3, 2, gfx(3)));
        assert_eq!(pmode0.mode, "RG2");
        assert_eq!(pmode0.vram_bytes, 1536);
    }

    #[test]
    fn rg6_one_bit_per_pixel() {
        let mut mem = ram();
        mem[0x0400] = 0xA5;
        mem[0x0400 + 32] = 0xFF; // second line
        for (ctrl, fg) in [(gfx(7), GREEN), (gfx(7) | CSS, BUFF)] {
            let frame = render_frame(&mem, inputs(6, 2, ctrl));
            let line0: Vec<u8> = (0..8).map(|x| px(&frame, x, 0)).collect();
            assert_eq!(line0, expect_pattern(0xA5, fg, BLACK));
            assert_eq!(px(&frame, 8, 0), BLACK);
            assert!((0..8).all(|x| px(&frame, x, 1) == fg));
        }
    }

    #[test]
    fn cg6_two_bits_per_pixel() {
        let mut mem = ram();
        mem[0x0400] = 0b00_01_10_11;
        for (ctrl, set) in [(gfx(6), GREEN), (gfx(6) | CSS, BUFF)] {
            let frame = render_frame(&mem, inputs(6, 2, ctrl));
            let line0: Vec<u8> = (0..8).map(|x| px(&frame, x, 0)).collect();
            assert_eq!(line0, [set, set, set + 1, set + 1, set + 2, set + 2, set + 3, set + 3]);
        }
        // CSS 0: green yellow blue red; CSS 1: buff cyan magenta orange.
        assert_eq!([GREEN + 1, GREEN + 2, GREEN + 3], [YELLOW, BLUE, RED]);
        assert_eq!([BUFF + 1, BUFF + 2, BUFF + 3], [CYAN, MAGENTA, ORANGE]);
    }

    #[test]
    fn cg1_four_pixel_wide_colours_three_lines_per_row() {
        let mut mem = ram();
        mem[0x0400] = 0b11_10_01_00;
        mem[0x0410] = 0b01_01_01_01; // next VRAM row
        let frame = render_frame(&mem, inputs(1, 2, gfx(0)));
        assert_eq!(frame.mode, "CG1");
        for y in 0..3 {
            let line: Vec<u8> = (0..16).map(|x| px(&frame, x, y)).collect();
            let mut expect = vec![RED; 4];
            expect.extend([BLUE; 4]);
            expect.extend([YELLOW; 4]);
            expect.extend([GREEN; 4]);
            assert_eq!(line, expect, "line {y}");
        }
        assert!((0..16).all(|x| px(&frame, x, 3) == YELLOW));
    }

    #[test]
    fn rg1_rg2_rg3_two_pixels_per_bit() {
        // (GM, V, lines per row)
        for (gm, v, lines) in [(1u8, 1u8, 3usize), (3, 3, 2), (5, 5, 1)] {
            let mut mem = ram();
            mem[0x0400] = 0x81;
            mem[0x0410] = 0xFF;
            let frame = render_frame(&mem, inputs(v, 2, gfx(gm)));
            for y in 0..lines {
                let line: Vec<u8> = (0..16).map(|x| px(&frame, x, y)).collect();
                let mut expect = vec![GREEN, GREEN];
                expect.extend([BLACK; 12]);
                expect.extend([GREEN, GREEN]);
                assert_eq!(line, expect, "GM {gm} line {y}");
            }
            assert!((0..16).all(|x| px(&frame, x, lines) == GREEN), "GM {gm} next row");
            // 16 bytes span the full 256-pixel line.
            assert_eq!(px(&frame, 255, 0), BLACK);
        }
    }

    #[test]
    fn text_rows_decode() {
        let mut mem = ram();
        mem[0x0400] = b'H';
        mem[0x0401] = b'I';
        mem[0x0402] = 0x00; // '@', not blank
        mem[0x0403] = 0x5E; // ↑
        mem[0x0404] = 0x1F; // ← (inverse)
        mem[0x0405] = 0x68; // '(' with D6 set, not 'h'
        mem[0x0406] = 0x8F; // SG4 all lit
        mem[0x0407] = 0x89; // SG4 top-left + bottom-right
        mem[0x0408] = 0x80; // SG4 blank
        mem[0x0409] = 0x60; // BASIC's blank
        let rows = text_rows(&mem, inputs(0, 2, 0x00));
        assert_eq!(rows.len(), 16);
        assert!(rows.iter().all(|r| r.chars().count() == 32));
        assert!(rows[0].starts_with("HI@↑←(█▚  "), "{:?}", rows[0]);
        // SG6 screens decode graphics bytes as sextants.
        mem[0x0400] = 0xFF;
        mem[0x0401] = 0x80 | 0b10_10_10;
        mem[0x0402] = 0x80 | 0b10_00_00; // top-left only → U+1FB00
        let rows = text_rows(&mem, inputs(0, 2, GM0));
        assert!(rows[0].starts_with("█▌\u{1FB00}"), "{:?}", rows[0]);
        assert!(text_rows(&mem, inputs(2, 2, 0x00)).is_empty(), "SG8 is not text");
    }

    #[test]
    fn sextant_mapping_covers_all_patterns() {
        let mut seen = std::collections::HashSet::new();
        for b in 0x80..=0xBFu8 {
            assert!(seen.insert(cell_char(b, true)), "duplicate for {b:02X}");
        }
        assert_eq!(cell_char(0x80 | 0b01_11_11, true), '\u{1FB3B}');
        assert_eq!(cell_char(0x80 | 0b01_00_00, true), '\u{1FB01}');
    }
}
