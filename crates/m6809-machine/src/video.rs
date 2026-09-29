//! Video frame DTO for the frontend: the MC6847 / SAM field rendered by
//! [`crate::vdg`] as a palette-indexed framebuffer.

use m6809_core::Emulator;
use serde::{Deserialize, Serialize};

use crate::vdg::{self, VdgInputs};

/// One rendered VDG field.
///
/// `pixels` holds `width * height` palette indices (one byte per pixel,
/// row-major) encoded as standard base64; `palette[i]` is the colour of index
/// `i`. The 256 × 192 active area sits at (`active_x`, `active_y`); the rest
/// is the VDG border.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct VideoFrameDto {
    /// Framebuffer width in pixels (320).
    pub width: u16,
    /// Framebuffer height in pixels (240).
    pub height: u16,
    /// Left edge of the active area (32).
    pub active_x: u16,
    /// Top edge of the active area (24).
    pub active_y: u16,
    /// Active area width (256).
    pub active_width: u16,
    /// Active area height (192).
    pub active_height: u16,
    /// Base64 of the palette indices, one byte per pixel.
    pub pixels: String,
    /// Palette colours as "#rrggbb".
    pub palette: Vec<String>,
    /// Display mode: "Text32x16" (alphanumerics + SG4), "SG6", "SG8", "SG12",
    /// "SG24", "CG1", "RG1", "CG2", "RG2", "CG3", "RG3", "CG6", "RG6"; a SAM
    /// mode that does not belong to the VDG mode is appended as "/V<n>".
    pub mode: String,
    /// Video RAM base address (SAM F × 512).
    pub base_addr: u16,
    /// Bytes of video RAM shown, counted from `base_addr`.
    pub vram_bytes: u16,
    /// Logical resolution: character cells for "Text32x16" (32 × 16),
    /// elements / pixels for the other modes (e.g. 128 × 96 for RG2).
    pub cols: u16,
    pub rows: u16,
    /// Text decode of an alphanumeric screen (16 rows of 32 characters;
    /// semigraphics bytes as block characters). Empty for graphics modes.
    pub rows_text: Vec<String>,
    /// Hex 64-bit hash over the pixels and every other field: an unchanged
    /// hash means an unchanged frame.
    pub hash: String,
    /// SAM V2..V0.
    pub sam_v: u8,
    /// SAM F6..F0.
    pub sam_f: u8,
    /// PIA1 port B VDG control levels (CSS, GM0-2, A/G).
    pub vdg_ctrl: u8,
}

/// Current video frame of a CoCo 2 / Dragon 32 (`None` for machines without a VDG).
///
/// Reads video RAM straight from `emu.memory.ram`: rendering has no side
/// effects on I/O devices.
pub fn video_frame(emu: &Emulator) -> Option<VideoFrameDto> {
    let inputs = crate::machine_container(emu)?.vdg_inputs()?;
    Some(frame_from_ram(&emu.memory.ram, inputs))
}

/// Render the field that `inputs` select from `ram`.
pub fn frame_from_ram(ram: &[u8; 0x10000], inputs: VdgInputs) -> VideoFrameDto {
    let frame = vdg::render_frame(ram, inputs);
    let mut dto = VideoFrameDto {
        width: vdg::FRAME_WIDTH as u16,
        height: vdg::FRAME_HEIGHT as u16,
        active_x: vdg::BORDER_X as u16,
        active_y: vdg::BORDER_Y as u16,
        active_width: vdg::ACTIVE_WIDTH as u16,
        active_height: vdg::ACTIVE_HEIGHT as u16,
        pixels: base64_encode(&frame.pixels),
        palette: vdg::PALETTE
            .iter()
            .map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
            .collect(),
        mode: frame.mode,
        base_addr: frame.base_addr,
        vram_bytes: frame.vram_bytes,
        cols: frame.cols,
        rows: frame.rows,
        rows_text: vdg::text_rows(ram, inputs),
        hash: String::new(),
        sam_v: inputs.sam_v & 7,
        sam_f: inputs.sam_f & 0x7F,
        vdg_ctrl: inputs.vdg_ctrl,
    };
    dto.hash = frame_hash(&frame.pixels, &dto);
    dto
}

/// Cheap 64-bit hash (FxHash-style rotate / xor / multiply over 8-byte
/// words) of the raw pixels and the other DTO fields. Every step is a
/// bijection of the state, so changing any single word changes the result.
fn frame_hash(pixels: &[u8], dto: &VideoFrameDto) -> String {
    const K: u64 = 0x517C_C1B7_2722_0A95;
    struct Hasher(u64);
    impl Hasher {
        fn word(&mut self, w: u64) {
            self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(K);
        }
        fn bytes(&mut self, bytes: &[u8]) {
            self.word(bytes.len() as u64);
            let mut words = bytes.chunks_exact(8);
            for w in &mut words {
                self.word(u64::from_le_bytes([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]));
            }
            for &b in words.remainder() {
                self.word(u64::from(b));
            }
        }
    }
    let mut h = Hasher(0);
    for w in [
        dto.width,
        dto.height,
        dto.active_x,
        dto.active_y,
        dto.active_width,
        dto.active_height,
        dto.base_addr,
        dto.vram_bytes,
        dto.cols,
        dto.rows,
        u16::from(dto.sam_v),
        u16::from(dto.sam_f),
        u16::from(dto.vdg_ctrl),
    ] {
        h.word(u64::from(w));
    }
    h.bytes(pixels);
    h.bytes(dto.mode.as_bytes());
    for text in dto.palette.iter().chain(&dto.rows_text) {
        h.bytes(text.as_bytes());
    }
    format!("{:016x}", h.0)
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Two base64 characters for every 12-bit value.
static BASE64_PAIRS: [[u8; 2]; 4096] = {
    let mut pairs = [[0u8; 2]; 4096];
    let mut i = 0;
    while i < 4096 {
        pairs[i] = [BASE64_ALPHABET[i >> 6], BASE64_ALPHABET[i & 63]];
        i += 1;
    }
    pairs
};

/// Standard base64 (RFC 4648 alphabet, `=` padding).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = vec![b'='; data.len().div_ceil(3) * 4];
    let whole = data.len() / 3 * 3;
    let (mut i, mut o) = (0, 0);
    while i < whole {
        let n = (usize::from(data[i]) << 16) | (usize::from(data[i + 1]) << 8) | usize::from(data[i + 2]);
        let hi = &BASE64_PAIRS[n >> 12];
        let lo = &BASE64_PAIRS[n & 0xFFF];
        out[o] = hi[0];
        out[o + 1] = hi[1];
        out[o + 2] = lo[0];
        out[o + 3] = lo[1];
        i += 3;
        o += 4;
    }
    let rest = &data[whole..];
    if !rest.is_empty() {
        let n = (usize::from(rest[0]) << 16) | (usize::from(rest.get(1).copied().unwrap_or(0)) << 8);
        out[o] = BASE64_ALPHABET[n >> 18];
        out[o + 1] = BASE64_ALPHABET[(n >> 12) & 63];
        if rest.len() == 2 {
            out[o + 2] = BASE64_ALPHABET[(n >> 6) & 63];
        }
    }
    String::from_utf8(out).expect("base64 output is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vdg::colour::*;
    use crate::{apply_machine, MachineKind};

    fn base64_decode(text: &str) -> Vec<u8> {
        let value = |c: u8| -> u32 {
            match c {
                b'A'..=b'Z' => u32::from(c - b'A'),
                b'a'..=b'z' => u32::from(c - b'a') + 26,
                b'0'..=b'9' => u32::from(c - b'0') + 52,
                b'+' => 62,
                b'/' => 63,
                _ => panic!("invalid base64 {c}"),
            }
        };
        let mut out = Vec::new();
        for quad in text.as_bytes().chunks(4) {
            let pad = quad.iter().filter(|&&c| c == b'=').count();
            let mut n = 0u32;
            for &c in quad {
                n = (n << 6) | if c == b'=' { 0 } else { value(c) };
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..3 - pad]);
        }
        out
    }

    fn ram() -> Box<[u8; 0x10000]> {
        Box::new([0u8; 0x10000])
    }

    fn text_inputs() -> VdgInputs {
        VdgInputs {
            sam_v: 0,
            sam_f: 2,
            vdg_ctrl: 0,
        }
    }

    /// Program PIA1 port B like BASIC does: PB3-PB7 outputs, then `mode`.
    fn set_vdg_mode(emu: &mut Emulator, mode: u8) {
        emu.memory.write8(0xFF23, 0x00); // select DDRB
        emu.memory.write8(0xFF22, 0xF8);
        emu.memory.write8(0xFF23, 0x04); // select data register
        emu.memory.write8(0xFF22, mode);
    }

    /// Set the SAM V and F registers through their bit addresses.
    fn set_sam(emu: &mut Emulator, v: u8, f: u8) {
        for bit in 0..3u16 {
            let set = (v >> bit) & 1;
            emu.memory.write8(0xFFC0 + bit * 2 + u16::from(set), 0);
        }
        for bit in 0..7u16 {
            let set = (f >> bit) & 1;
            emu.memory.write8(0xFFC6 + bit * 2 + u16::from(set), 0);
        }
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in vectors {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)), all);
    }

    #[test]
    fn dto_carries_the_rendered_framebuffer() {
        let mut mem = ram();
        mem[0x0400] = b'H';
        mem[0x0401] = b'I';
        let frame = frame_from_ram(&mem, text_inputs());
        assert_eq!((frame.width, frame.height), (320, 240));
        assert_eq!((frame.active_x, frame.active_y), (32, 24));
        assert_eq!((frame.active_width, frame.active_height), (256, 192));
        assert_eq!((frame.cols, frame.rows), (32, 16));
        assert_eq!(frame.mode, "Text32x16");
        assert_eq!(frame.base_addr, 0x0400);
        assert_eq!(frame.vram_bytes, 512);
        let pixels = base64_decode(&frame.pixels);
        assert_eq!(pixels.len(), 320 * 240);
        assert_eq!(pixels, vdg::render_frame(&mem, text_inputs()).pixels);
        assert!(pixels.iter().all(|&p| (p as usize) < frame.palette.len()));
        assert_eq!(frame.palette.len(), vdg::PALETTE.len());
        assert_eq!(frame.palette[BLACK as usize], "#263016");
        assert_eq!(frame.palette[GREEN as usize], "#30d200");
        assert_eq!(frame.palette[BUFF as usize], "#bfc8ad");
        assert!(frame.rows_text[0].starts_with("HI"));
        assert_eq!(pixels[0], BLACK, "alphanumeric border");
        // 'H' is normal text (D6 set): its first line is bright background.
        assert_eq!(pixels[24 * 320 + 32], ALPHA_BRIGHT_GREEN);
    }

    #[test]
    fn dto_serializes_snake_case() {
        let frame = frame_from_ram(&ram(), text_inputs());
        let json = serde_json::to_value(&frame).expect("json");
        for key in [
            "width",
            "height",
            "active_x",
            "active_y",
            "active_width",
            "active_height",
            "pixels",
            "palette",
            "mode",
            "base_addr",
            "vram_bytes",
            "cols",
            "rows",
            "rows_text",
            "hash",
            "sam_v",
            "sam_f",
            "vdg_ctrl",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn hash_is_stable_and_tracks_the_picture() {
        let mut mem = ram();
        let a = frame_from_ram(&mem, text_inputs());
        let b = frame_from_ram(&mem, text_inputs());
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.hash.len(), 16);
        mem[0x0400] = 0x41;
        let c = frame_from_ram(&mem, text_inputs());
        assert_ne!(a.hash, c.hash, "VRAM change");
        let d = frame_from_ram(
            &mem,
            VdgInputs {
                vdg_ctrl: 0x08,
                ..text_inputs()
            },
        );
        assert_ne!(c.hash, d.hash, "CSS change");
        // Bytes outside the displayed area do not change the picture.
        mem[0x0600] = 0x55;
        assert_eq!(frame_from_ram(&mem, text_inputs()).hash, c.hash);
        // The same picture from another address is still a different frame.
        let mut copy = ram();
        copy[0x0400] = 0x41;
        copy[0x0600] = 0x41;
        let moved = frame_from_ram(
            &copy,
            VdgInputs {
                sam_f: 3,
                ..text_inputs()
            },
        );
        assert_eq!(moved.pixels, c.pixels);
        assert_ne!(moved.hash, c.hash, "base address change");
    }

    #[test]
    fn graphics_frames_have_no_text() {
        let mem = ram();
        let frame = frame_from_ram(
            &mem,
            VdgInputs {
                sam_v: 3,
                sam_f: 2,
                vdg_ctrl: 0xB0, // A/G, GM = 3 (RG2 = PMODE 0)
            },
        );
        assert_eq!(frame.mode, "RG2");
        assert_eq!((frame.cols, frame.rows), (128, 96));
        assert_eq!(frame.vram_bytes, 1536);
        assert!(frame.rows_text.is_empty());
        assert_eq!(base64_decode(&frame.pixels)[0], GREEN, "graphics border");
    }

    #[test]
    fn coco2_frame_follows_pia_and_sam() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        set_vdg_mode(&mut emu, 0x00);
        set_sam(&mut emu, 0, 2);
        emu.memory.write8(0x0400, b'H');
        emu.memory.write8(0x0401, b'I');
        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "Text32x16");
        assert_eq!(frame.base_addr, 0x0400);
        assert!(frame.rows_text[0].starts_with("HI"));
        assert_eq!(frame.vdg_ctrl & 0xF8, 0x00);

        // PMODE 4: RG6 with SAM V = 6, screen at $0E00, CSS 1.
        set_vdg_mode(&mut emu, 0xF8);
        set_sam(&mut emu, 6, 7);
        emu.memory.write8(0x0E00, 0x80);
        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "RG6");
        assert_eq!(frame.base_addr, 0x0E00);
        assert_eq!((frame.sam_v, frame.sam_f), (6, 7));
        assert!(frame.rows_text.is_empty());
        let pixels = base64_decode(&frame.pixels);
        assert_eq!(pixels[0], BUFF, "CSS 1 graphics border");
        assert_eq!(pixels[24 * 320 + 32], BUFF);
        assert_eq!(pixels[24 * 320 + 33], BLACK);
    }

    #[test]
    fn dragon32_frame_follows_pia_and_sam() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Dragon32);
        set_vdg_mode(&mut emu, 0x00);
        set_sam(&mut emu, 0, 2);
        emu.memory.write8(0x0400, 0x04); // inverse 'D'
        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "Text32x16");
        assert!(frame.rows_text[0].starts_with('D'));

        // PMODE 1: CG3 with SAM V = 4.
        set_vdg_mode(&mut emu, 0xC0);
        set_sam(&mut emu, 4, 2);
        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.mode, "CG3");
        assert_eq!(frame.vram_bytes, 3072);
    }

    #[test]
    fn machines_without_vdg_have_no_frame() {
        for kind in [MachineKind::Bare, MachineKind::MsBasic] {
            let mut emu = Emulator::new();
            apply_machine(&mut emu, kind);
            assert!(video_frame(&emu).is_none());
        }
    }

    #[test]
    fn rendering_has_no_side_effects_on_io() {
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        // Screen at $FE00: the field spans $FE00-$FFFF, including the PIAs.
        set_vdg_mode(&mut emu, 0x00);
        set_sam(&mut emu, 0, 0x7F);
        // Let field sync latch the VSYNC flag in PIA0 CRB.
        let mut latched = false;
        for _ in 0..8 {
            emu.memory.io.as_mut().expect("io").tick(15_000);
            if emu.memory.peek8(0xFF03) & 0x80 != 0 {
                latched = true;
                break;
            }
        }
        assert!(latched, "field sync should set the PIA0 CB1 flag");
        let before = emu.memory.io.as_ref().expect("io").snapshot();

        let frame = video_frame(&emu).expect("frame");
        assert_eq!(frame.base_addr, 0xFE00);

        assert_ne!(emu.memory.peek8(0xFF03) & 0x80, 0, "flag still pending");
        assert_eq!(emu.memory.io.as_ref().expect("io").snapshot(), before);
    }
}
