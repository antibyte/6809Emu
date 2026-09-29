//! CoCo 2 / Dragon 32 peripherals behind the PIAs: 6-bit DAC and single-bit
//! sound, joystick comparator, cassette, printer and ROM cartridge.
//!
//! The board (`coco2.rs` / `dragon32.rs`) owns the PIAs and calls into this
//! module: `tick` after every CPU step with the current PIA pin levels, and
//! `inputs` whenever it needs the levels the peripherals drive back.
//!
//! Signal map (Color Computer Technical Reference, MAME `coco.cpp`, XRoar):
//! - PIA0 CA2 / CB2 = analog MUX SEL1 (LSB) / SEL2 (MSB). Joystick axis:
//!   0 = right X, 1 = right Y, 2 = left X, 3 = left Y. Sound source:
//!   0 = DAC, 1 = cassette, 2 = cartridge, 3 = none.
//! - PIA0 PA7 = comparator: high while the selected pot ≥ the DAC value (the
//!   ROM's JOYSTK successive approximation adds the step when PA7 is high).
//! - PIA0 PA0 / PA1 = right / left fire button, active low.
//! - PIA1 PA2-PA7 = 6-bit DAC (sound, joystick reference, cassette out);
//!   PA0 = cassette in; PA1 = RS-232 out (CoCo) / printer STROBE (Dragon).
//! - PIA1 PB0 = RS-232 in = printer ready (CoCo, 0 = ready) / printer BUSY
//!   (Dragon); PB1 = single-bit sound.
//! - PIA1 CA1 = RS-232 CD (CoCo) / printer ACK (Dragon); CA2 = cassette relay;
//!   CB1 = CART* (FIRQ); CB2 = sound enable.
//!
//! Serialization: everything is part of the board state. The tape image,
//! the recording and the cartridge ROM are stored as hex strings; the audio
//! queue and the derived tape program are not stored (rebuilt on demand).

use serde::{Deserialize, Serialize};

use crate::board_sound::{self, AnalogInputs, BoardSound};
use crate::cassette::Cassette;
use crate::printer::Printer;

/// Board flavour: the signal routing differs between CoCo and Dragon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardKind {
    Coco2,
    Dragon32,
}

impl BoardKind {
    /// Tick rate the board uses (2 x the nominal E clock); only a fallback
    /// for a caller passing `clock_hz == 0`.
    fn default_tick_hz(self) -> u32 {
        match self {
            BoardKind::Coco2 => 2 * 894_886,
            BoardKind::Dragon32 => 2 * 888_625,
        }
    }
}

/// Effective PIA pin levels seen by the peripherals (outputs where DDR=1,
/// pulled high where DDR=0), sampled by the board.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PiaPins {
    pub pia0_a: u8,
    pub pia0_b: u8,
    pub pia0_ca2: bool,
    pub pia0_cb2: bool,
    pub pia1_a: u8,
    pub pia1_b: u8,
    pub pia1_ca2: bool,
    pub pia1_cb2: bool,
}

impl PiaPins {
    /// Analog MUX select: SEL1 = PIA0 CA2 (bit 0), SEL2 = PIA0 CB2 (bit 1).
    pub fn mux(&self) -> u8 {
        u8::from(self.pia0_ca2) | (u8::from(self.pia0_cb2) << 1)
    }

    /// 6-bit DAC value on PIA1 PA2-PA7.
    pub fn dac(&self) -> u8 {
        self.pia1_a >> 2
    }
}

/// Levels the peripherals drive into the PIAs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeripheralInputs {
    /// PIA0 PA7: joystick comparator output.
    pub pia0_pa7: bool,
    /// PIA0 PA0/PA1: joystick fire buttons, active low (bit set = released).
    /// The board ANDs these with the keyboard rows.
    pub pia0_pa_buttons: u8,
    /// PIA1 PA0: cassette data in.
    pub pia1_pa0: bool,
    /// PIA1 PB0: RS-232 in (CoCo) / printer busy (Dragon).
    pub pia1_pb0: bool,
    /// PIA1 CA1: RS-232 carrier detect (CoCo) / printer acknowledge (Dragon).
    pub pia1_ca1: bool,
    /// PIA1 CB1: CART* (pulsed by Q while an autostart cartridge is inserted).
    pub pia1_cb1: bool,
}

impl Default for PeripheralInputs {
    fn default() -> Self {
        Self {
            pia0_pa7: true,
            pia0_pa_buttons: 0x03,
            pia1_pa0: true,
            pia1_pb0: false,
            pia1_ca1: true,
            pia1_cb1: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CassetteStateDto {
    pub loaded: bool,
    pub name: Option<String>,
    /// Byte position within the tape image.
    pub position: usize,
    pub length: usize,
    /// Cassette relay (PIA1 CA2).
    pub motor: bool,
    /// Bytes captured from CSAVE since the last take.
    pub recorded_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartridgeStateDto {
    pub loaded: bool,
    pub name: Option<String>,
    pub size: usize,
    /// CART* toggled by Q so BASIC autostarts the ROM at $C000.
    pub autostart: bool,
}

/// Largest cartridge ROM window: $C000-$FEFF (16 KiB minus the I/O page).
pub const CARTRIDGE_MAX_BYTES: usize = 0x4000;
const CART_BASE: u16 = 0xC000;
const CART_END: u16 = 0xFEFF;

/// Pot positions of one joystick (0..=63) and its fire button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Joystick {
    x: u8,
    y: u8,
    button: bool,
}

impl Default for Joystick {
    fn default() -> Self {
        // Centred sticks, like a joystick at rest.
        Self {
            x: 32,
            y: 32,
            button: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cartridge {
    name: String,
    #[serde(with = "hex_bytes")]
    rom: Vec<u8>,
    autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peripherals {
    kind: BoardKind,
    /// Time units seen so far (time base for the printer).
    #[serde(default)]
    cycles: u64,
    #[serde(default)]
    sound: BoardSound,
    /// 0 = right, 1 = left.
    #[serde(default)]
    joysticks: [Joystick; 2],
    #[serde(default)]
    cassette: Cassette,
    #[serde(default)]
    printer: Printer,
    #[serde(default)]
    cartridge: Option<Cartridge>,
    /// CART* level while an autostart cartridge toggles it with Q.
    #[serde(default)]
    cart_q: bool,
}

impl Peripherals {
    pub fn new(kind: BoardKind) -> Self {
        Self {
            kind,
            cycles: 0,
            sound: BoardSound::default(),
            joysticks: [Joystick::default(); 2],
            cassette: Cassette::default(),
            printer: Printer::default(),
            cartridge: None,
            cart_q: false,
        }
    }

    pub fn kind(&self) -> BoardKind {
        self.kind
    }

    /// Advance by `cycles` E cycles at `clock_hz` with the given PIA pin levels.
    ///
    /// Only the ratio `cycles / clock_hz` (elapsed real time) is used, so any
    /// time unit works: the board passes SAM-speed-aware ticks at twice the
    /// E clock. `pins` are the levels after the CPU step; the step itself ran
    /// with the previous levels, so audio integrates the old level first.
    pub fn tick(&mut self, cycles: u32, clock_hz: u32, pins: &PiaPins) {
        let clock = if clock_hz == 0 {
            self.kind.default_tick_hz()
        } else {
            clock_hz
        };
        self.sound.advance(cycles, clock);
        self.cycles += u64::from(cycles);

        self.cassette.tick(cycles, clock, pins.pia1_ca2, pins.dac());

        let strobe_or_tx = pins.pia1_a & 0x02 != 0;
        match self.kind {
            BoardKind::Coco2 => self.printer.serial_tick(self.cycles, clock, strobe_or_tx),
            BoardKind::Dragon32 => {
                self.printer
                    .parallel_tick(self.cycles, clock, strobe_or_tx, pins.pia0_b)
            }
        }

        if cycles > 0 && self.cartridge.as_ref().is_some_and(|c| c.autostart) {
            // Q runs at the E rate; one edge per CPU step is plenty for the
            // PIA's edge detector.
            self.cart_q = !self.cart_q;
        }

        self.sound.set_level(board_sound::output_level(&AnalogInputs {
            dac: pins.dac(),
            tape: self.cassette.audio_level(),
            sbs_high: pins.pia1_b & 0x02 != 0,
            mux: pins.mux(),
            sound_enabled: pins.pia1_cb2,
        }));
    }

    /// Current levels the peripherals drive into the PIAs.
    pub fn inputs(&self, pins: &PiaPins) -> PeripheralInputs {
        let pia1_ca1 = match self.kind {
            // RS-232 carrier detect: nothing connected drives it; idle high.
            BoardKind::Coco2 => true,
            BoardKind::Dragon32 => self.printer.ack_line(self.cycles),
        };
        PeripheralInputs {
            pia0_pa7: self.comparator(pins),
            pia0_pa_buttons: self.buttons(),
            pia1_pa0: self.cassette.input_level(),
            // CoCo: the printer drives RS-232 in low when ready (a real Radio
            // Shack printer's busy line); Dragon: BUSY low. Never busy here.
            pia1_pb0: false,
            pia1_ca1,
            pia1_cb1: match &self.cartridge {
                Some(c) if c.autostart => self.cart_q,
                _ => true,
            },
        }
    }

    /// PIA0 PA7: selected pot ≥ DAC.
    fn comparator(&self, pins: &PiaPins) -> bool {
        let mux = pins.mux();
        let stick = &self.joysticks[usize::from(mux >> 1)];
        let pot = if mux & 1 == 0 { stick.x } else { stick.y };
        pot >= pins.dac()
    }

    /// PIA0 PA0 (right) / PA1 (left) fire buttons, active low.
    fn buttons(&self) -> u8 {
        let mut bits = 0x03;
        if self.joysticks[0].button {
            bits &= !0x01;
        }
        if self.joysticks[1].button {
            bits &= !0x02;
        }
        bits
    }

    /// Hardware RESET (cassette motor off, cartridge stays inserted).
    pub fn reset(&mut self) {
        self.cassette.reset();
        self.printer.reset();
    }

    /// Board sound at `AUDIO_SAMPLE_RATE` (mono, -1.0..1.0).
    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.sound.drain()
    }

    /// Joystick `port` 0 = right, 1 = left; axes 0..63; button pressed = true.
    pub fn set_joystick(&mut self, port: usize, x: u8, y: u8, button: bool) {
        if let Some(stick) = self.joysticks.get_mut(port) {
            *stick = Joystick {
                x: x.min(63),
                y: y.min(63),
                button,
            };
        }
    }

    pub fn cassette_insert(&mut self, name: String, data: Vec<u8>) -> CassetteStateDto {
        self.cassette.insert(name, data);
        self.cassette_state()
    }

    pub fn cassette_eject(&mut self) -> CassetteStateDto {
        self.cassette.eject();
        self.cassette_state()
    }

    pub fn cassette_rewind(&mut self) -> CassetteStateDto {
        self.cassette.rewind();
        self.cassette_state()
    }

    pub fn cassette_state(&self) -> CassetteStateDto {
        CassetteStateDto {
            loaded: self.cassette.loaded(),
            name: self.cassette.name(),
            position: self.cassette.position(),
            length: self.cassette.length(),
            motor: self.cassette.motor(),
            recorded_bytes: self.cassette.recorded_bytes(),
        }
    }

    /// Take (and clear) the CSAVE output as a .CAS image.
    pub fn cassette_take_recording(&mut self) -> Vec<u8> {
        self.cassette.take_recording()
    }

    /// Take (and clear) the text sent to the printer.
    pub fn printer_take_output(&mut self) -> String {
        self.printer.take_output()
    }

    /// Insert a cartridge ROM (at most 16 KiB are used; smaller images are
    /// mirrored across $C000-$FEFF like on the real bus). Empty data ejects.
    pub fn cartridge_insert(
        &mut self,
        name: String,
        mut data: Vec<u8>,
        autostart: bool,
    ) -> CartridgeStateDto {
        if data.is_empty() {
            return self.cartridge_eject();
        }
        data.truncate(CARTRIDGE_MAX_BYTES);
        self.cartridge = Some(Cartridge {
            name,
            rom: data,
            autostart,
        });
        self.cart_q = false;
        self.cartridge_state()
    }

    pub fn cartridge_eject(&mut self) -> CartridgeStateDto {
        self.cartridge = None;
        self.cartridge_state()
    }

    pub fn cartridge_state(&self) -> CartridgeStateDto {
        match &self.cartridge {
            Some(c) => CartridgeStateDto {
                loaded: true,
                name: Some(c.name.clone()),
                size: c.rom.len(),
                autostart: c.autostart,
            },
            None => CartridgeStateDto::default(),
        }
    }

    /// Cartridge ROM byte at `addr` ($C000-$FEFF) when a cartridge is inserted.
    pub fn cartridge_read(&self, addr: u16) -> Option<u8> {
        let cart = self.cartridge.as_ref()?;
        if !(CART_BASE..=CART_END).contains(&addr) {
            return None;
        }
        // The ROM only decodes the address lines it needs: images smaller
        // than 16 KiB repeat; bytes past a non-power-of-two image read $FF.
        let offset = usize::from(addr - CART_BASE);
        let window = cart.rom.len().next_power_of_two();
        Some(cart.rom.get(offset & (window - 1)).copied().unwrap_or(0xFF))
    }
}

/// Serde helper: `Vec<u8>` as a lowercase hex string (compact in JSON).
pub(crate) mod hex_bytes {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            text.push(char::from(DIGITS[usize::from(b >> 4)]));
            text.push(char::from(DIGITS[usize::from(b & 15)]));
        }
        serializer.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text.as_bytes();
        if digits.len() % 2 != 0 {
            return Err(D::Error::custom("hex string has odd length"));
        }
        let nibble = |c: u8| -> Result<u8, D::Error> {
            (c as char)
                .to_digit(16)
                .map(|d| d as u8)
                .ok_or_else(|| D::Error::custom("invalid hex digit"))
        };
        digits
            .chunks(2)
            .map(|pair| Ok((nibble(pair[0])? << 4) | nibble(pair[1])?))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cassette::tests::basic_tape;

    const CLOCK: u32 = 894_886;

    /// Pins as Color BASIC leaves them after its PIA init: PIA1 PA = $02,
    /// PB1 input (pulled high), MUX 0, sound off, relay open.
    fn idle_pins() -> PiaPins {
        PiaPins {
            pia0_a: 0xFF,
            pia0_b: 0xFF,
            pia0_ca2: false,
            pia0_cb2: false,
            pia1_a: 0x02,
            pia1_b: 0x07,
            pia1_ca2: false,
            pia1_cb2: false,
        }
    }

    fn with_mux(mut pins: PiaPins, mux: u8) -> PiaPins {
        pins.pia0_ca2 = mux & 1 != 0;
        pins.pia0_cb2 = mux & 2 != 0;
        pins
    }

    /// The ROM's JOYSTK successive approximation (Color BASIC $A9EB).
    fn rom_joystk(p: &Peripherals, axis: u8) -> u8 {
        let mut b: u8 = 0x80;
        let mut step: u8 = 0x40;
        loop {
            let mut pins = with_mux(idle_pins(), axis);
            pins.pia1_a = b | 0x02;
            if p.inputs(&pins).pia0_pa7 {
                b = b.wrapping_add(step);
            } else {
                b = b.wrapping_sub(step);
            }
            step >>= 1;
            if step == 1 {
                break;
            }
        }
        b >> 2
    }

    #[test]
    fn joystk_successive_approximation_returns_the_set_values() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        assert_eq!(rom_joystk(&p, 0), 32, "centred by default");
        for &(rx, ry, lx, ly) in &[(0, 63, 17, 42), (63, 0, 1, 62), (31, 32, 33, 5)] {
            p.set_joystick(0, rx, ry, false);
            p.set_joystick(1, lx, ly, false);
            assert_eq!(rom_joystk(&p, 0), rx, "right X");
            assert_eq!(rom_joystk(&p, 1), ry, "right Y");
            assert_eq!(rom_joystk(&p, 2), lx, "left X");
            assert_eq!(rom_joystk(&p, 3), ly, "left Y");
        }
        p.set_joystick(0, 200, 99, false);
        assert_eq!(rom_joystk(&p, 0), 63, "clamped");
        p.set_joystick(5, 1, 1, true); // no such port: ignored
        assert_eq!(p.inputs(&idle_pins()).pia0_pa_buttons, 0x03);
    }

    #[test]
    fn fire_buttons_are_active_low_on_pa0_right_pa1_left() {
        let mut p = Peripherals::new(BoardKind::Dragon32);
        p.set_joystick(0, 32, 32, true);
        assert_eq!(p.inputs(&idle_pins()).pia0_pa_buttons, 0x02);
        p.set_joystick(1, 32, 32, true);
        assert_eq!(p.inputs(&idle_pins()).pia0_pa_buttons, 0x00);
        p.set_joystick(0, 32, 32, false);
        assert_eq!(p.inputs(&idle_pins()).pia0_pa_buttons, 0x01);
    }

    #[test]
    fn idle_levels_do_not_disturb_basic() {
        for kind in [BoardKind::Coco2, BoardKind::Dragon32] {
            let mut p = Peripherals::new(kind);
            // Power-on: every PIA pin is an input, pulled high.
            let reset_pins = PiaPins {
                pia0_a: 0xFF,
                pia0_b: 0xFF,
                pia0_ca2: true,
                pia0_cb2: true,
                pia1_a: 0xFF,
                pia1_b: 0xFF,
                pia1_ca2: false,
                pia1_cb2: false,
            };
            let mut pins = idle_pins();
            if kind == BoardKind::Dragon32 {
                pins.pia1_a = 0x00; // Dragon BASIC clears PIA1 PA (STROBE low)
            }
            for i in 0..1000 {
                let now = if i < 10 { reset_pins } else { pins };
                p.tick(5, 0, &now);
                let inp = p.inputs(&now);
                assert!(inp.pia1_pa0, "cassette in idles high");
                assert!(!inp.pia1_pb0, "printer ready / not busy");
                assert!(inp.pia1_ca1, "CD / ACK idle high");
                assert!(inp.pia1_cb1, "CART* idle high without a cartridge");
            }
            assert_eq!(p.printer_take_output(), "");
        }
    }

    #[test]
    fn sound_follows_dac_only_when_enabled_and_selected() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        let run = |p: &mut Peripherals, enabled: bool, mux: u8| {
            let _ = p.drain_audio();
            let mut t = 0u32;
            while t < CLOCK / 2 {
                let mut pins = with_mux(idle_pins(), mux);
                pins.pia1_cb2 = enabled;
                // 500 Hz square wave on the DAC, full scale.
                pins.pia1_a = if (t / (CLOCK / 1000)) % 2 == 0 { 0xFE } else { 0x02 };
                p.tick(8, CLOCK, &pins);
                t += 8;
            }
            let s = p.drain_audio();
            let tail = &s[s.len() / 4..];
            tail.iter().fold(0.0f32, |m, &x| m.max(x.abs()))
        };
        assert!(run(&mut p, true, 0) > 0.15, "SOUND audible");
        assert!(run(&mut p, false, 0) < 0.01, "sound disabled");
        assert!(run(&mut p, true, 1) < 0.01, "MUX on cassette");
        assert!(run(&mut p, true, 3) < 0.01, "MUX on nothing");
    }

    #[test]
    fn audio_rate_is_locked_to_emulated_time() {
        let mut p = Peripherals::new(BoardKind::Dragon32);
        for _ in 0..(888_625 / 5) {
            p.tick(5, 888_625, &idle_pins());
        }
        assert_eq!(p.drain_audio().len(), 44_100);
    }

    #[test]
    fn single_bit_sound_is_audible() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        let mut t = 0u32;
        while t < CLOCK / 4 {
            let mut pins = idle_pins();
            pins.pia1_b = if (t / 400) % 2 == 0 { 0x02 } else { 0x00 };
            p.tick(4, CLOCK, &pins);
            t += 4;
        }
        let s = p.drain_audio();
        let peak = s[s.len() / 2..].iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.15, "peak={peak}");
    }

    #[test]
    fn cartridge_mirrors_and_bounds() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        assert_eq!(p.cartridge_read(0xC000), None);
        let rom: Vec<u8> = (0..0x2000).map(|i| (i & 0xFF) as u8 ^ (i >> 8) as u8).collect();
        let st = p.cartridge_insert("game.rom".into(), rom.clone(), true);
        assert_eq!(
            st,
            CartridgeStateDto {
                loaded: true,
                name: Some("game.rom".into()),
                size: 0x2000,
                autostart: true
            }
        );
        assert_eq!(p.cartridge_read(0xC000), Some(rom[0]));
        assert_eq!(p.cartridge_read(0xDFFF), Some(rom[0x1FFF]));
        assert_eq!(p.cartridge_read(0xE123), Some(rom[0x0123]), "8K mirrored");
        assert_eq!(p.cartridge_read(0xFEFF), Some(rom[0x1EFF]));
        assert_eq!(p.cartridge_read(0xFF00), None, "I/O page is not cartridge");
        assert_eq!(p.cartridge_read(0xBFFF), None);
        // Odd size: missing bytes read $FF.
        p.cartridge_insert("odd".into(), vec![0xAA; 0x1800], false);
        assert_eq!(p.cartridge_read(0xC000 + 0x17FF), Some(0xAA));
        assert_eq!(p.cartridge_read(0xC000 + 0x1800), Some(0xFF));
        assert_eq!(p.cartridge_read(0xE000), Some(0xAA));
        // Oversized: first 16 KiB.
        let st = p.cartridge_insert("big".into(), vec![1; 0x8000], false);
        assert_eq!(st.size, CARTRIDGE_MAX_BYTES);
        let st = p.cartridge_eject();
        assert_eq!(st, CartridgeStateDto::default());
        assert_eq!(p.cartridge_read(0xC000), None);
        assert!(!p.cartridge_insert("empty".into(), vec![], true).loaded);
    }

    #[test]
    fn autostart_cartridge_toggles_cart_line_every_step() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        p.cartridge_insert("auto".into(), vec![0x12; 16], true);
        let mut rising = 0;
        let mut last = p.inputs(&idle_pins()).pia1_cb1;
        for _ in 0..100 {
            p.tick(3, CLOCK, &idle_pins());
            let now = p.inputs(&idle_pins()).pia1_cb1;
            if now && !last {
                rising += 1;
            }
            last = now;
        }
        assert_eq!(rising, 50);
        // Without autostart CART* stays high.
        p.cartridge_insert("plain".into(), vec![0x12; 16], false);
        for _ in 0..10 {
            p.tick(3, CLOCK, &idle_pins());
            assert!(p.inputs(&idle_pins()).pia1_cb1);
        }
        // Reset keeps the cartridge.
        p.reset();
        assert!(p.cartridge_state().loaded);
    }

    #[test]
    fn cassette_follows_the_relay_and_reports_state() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        let tape = basic_tape("GAME", &[1, 2, 3, 4], 0, 0);
        let st = p.cassette_insert("game.cas".into(), tape.clone());
        assert!(st.loaded && !st.motor && st.position == 0 && st.length == tape.len());
        let mut pins = idle_pins();
        pins.pia1_ca2 = true;
        let mut saw_low = false;
        for _ in 0..(3 * CLOCK / 10) {
            p.tick(10, CLOCK, &pins);
            saw_low |= !p.inputs(&pins).pia1_pa0;
        }
        assert!(saw_low, "FSK on PA0 while the motor runs");
        let st = p.cassette_state();
        assert!(st.motor && st.position > 0);
        pins.pia1_ca2 = false;
        p.tick(10, CLOCK, &pins);
        assert!(p.inputs(&pins).pia1_pa0, "idle high with the relay open");
        assert!(!p.cassette_state().motor);
        assert_eq!(p.cassette_rewind().position, 0);
        assert!(!p.cassette_eject().loaded);
        // Reset opens the relay.
        pins.pia1_ca2 = true;
        p.tick(10, CLOCK, &pins);
        p.reset();
        assert!(!p.cassette_state().motor);
    }

    #[test]
    fn coco_serial_printer_via_pins() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        let bit = 1488u32;
        let send = |p: &mut Peripherals, level: bool, cycles: u32| {
            let mut pins = idle_pins();
            pins.pia1_a = if level { 0x02 } else { 0x00 };
            let mut left = cycles;
            while left > 0 {
                let n = left.min(5);
                p.tick(n, CLOCK, &pins);
                left -= n;
            }
        };
        send(&mut p, true, 5000);
        for &b in b"OK\r" {
            send(&mut p, false, bit);
            for i in 0..8 {
                send(&mut p, (b >> i) & 1 != 0, bit);
            }
            send(&mut p, true, bit + 60);
        }
        send(&mut p, true, 20_000);
        assert_eq!(p.printer_take_output(), "OK\n");
        assert_eq!(p.printer_take_output(), "");
    }

    #[test]
    fn dragon_parallel_printer_via_pins() {
        let mut p = Peripherals::new(BoardKind::Dragon32);
        let mut pins = idle_pins();
        pins.pia1_a = 0x00;
        for &b in b"LIST\r" {
            pins.pia0_b = b;
            p.tick(5, 888_625, &pins);
            pins.pia1_a = 0x02; // STB $FF20
            p.tick(5, 888_625, &pins);
            assert!(!p.inputs(&pins).pia1_ca1, "ACK pulse");
            pins.pia1_a = 0x00; // CLR $FF20
            p.tick(7, 888_625, &pins);
            for _ in 0..5 {
                p.tick(5, 888_625, &pins);
            }
            assert!(p.inputs(&pins).pia1_ca1, "ACK released");
        }
        assert_eq!(p.printer_take_output(), "LIST\n");
    }

    #[test]
    fn serde_round_trip_keeps_media_and_accepts_old_snapshots() {
        let mut p = Peripherals::new(BoardKind::Coco2);
        p.set_joystick(1, 10, 20, true);
        p.cassette_insert("t.cas".into(), vec![0x55, 0x3C, 0x00]);
        p.cartridge_insert("c.rom".into(), vec![0xDE, 0xAD], true);
        let json = serde_json::to_value(&p).unwrap();
        let back: Peripherals = serde_json::from_value(json).unwrap();
        assert_eq!(back.cassette_state(), p.cassette_state());
        assert_eq!(back.cartridge_state(), p.cartridge_state());
        assert_eq!(back.cartridge_read(0xC001), Some(0xAD));
        assert_eq!(back.inputs(&idle_pins()).pia0_pa_buttons, 0x01);
        // The stub's snapshot format (kind only) still loads.
        let old: Peripherals = serde_json::from_str(r#"{"kind":"Dragon32"}"#).unwrap();
        assert_eq!(old.kind(), BoardKind::Dragon32);
        assert_eq!(old.inputs(&idle_pins()), PeripheralInputs::default());
    }

    #[test]
    fn hex_bytes_round_trip_and_errors() {
        #[derive(Serialize, Deserialize)]
        struct W(#[serde(with = "hex_bytes")] Vec<u8>);
        let json = serde_json::to_string(&W(vec![0x00, 0x7F, 0xA5, 0xFF])).unwrap();
        assert_eq!(json, "\"007fa5ff\"");
        let back: W = serde_json::from_str(&json).unwrap();
        assert_eq!(back.0, vec![0x00, 0x7F, 0xA5, 0xFF]);
        assert!(serde_json::from_str::<W>("\"abc\"").is_err());
        assert!(serde_json::from_str::<W>("\"zz\"").is_err());
    }
}
