//! TRS-80 Color Computer 2 (64K) board, plus the board core it shares with
//! the Dragon 32 (`dragon32.rs`): two MC6821 PIAs, the MC6883 SAM, the
//! keyboard matrix, MC6847 sync timing and the ROM / cartridge / I/O map.
//!
//! Memory map (SAM map type 0 unless noted):
//! - `$0000-$7FFF` RAM (`P1` in 64K mode maps it to the upper 32K of RAM)
//! - `$8000-$BFFF` BASIC ROM, `$C000-$FEFF` cartridge ROM (open bus without
//!   one); writes are ignored. Map type 1 (`TY`, `$FFDF`) makes
//!   `$8000-$FEFF` RAM (the 64K CoCo has real RAM there; the Dragon 32's
//!   32K x 1 RAM does not decode A15, so it mirrors `$0000-$7EFF`).
//! - `$FF00-$FF1F` PIA0, `$FF20-$FF3F` PIA1 (4 registers, mirrored)
//! - `$FF40-$FF5F` cartridge I/O (SCS), `$FF60-$FFBF` unused: read `$FF`
//!   (open-bus approximation), writes ignored
//! - `$FFC0-$FFDF` SAM control register (write-only, reads `$FF`)
//! - `$FFE0-$FFFF` vectors: ROM `$BFE0-$BFFF`, writes ignored
//!
//! Interrupts: PIA0 IRQA (HS on CA1) / IRQB (FS on CB1) → CPU /IRQ;
//! PIA1 IRQA (CA1: RS-232 CD / printer ACK) / IRQB (CB1: CART*) → /FIRQ.
//! Both are level outputs; a handler acknowledges by reading the port.
//!
//! Timing: the VDG runs from its own clock, so HS/FS keep real time when
//! the SAM speeds the CPU up. Board time is counted in ticks of half a slow
//! E cycle: a slow CPU cycle is 2 ticks, a fast one 1 tick. A scan line is
//! 57 slow E cycles (228 VDG clocks); HS is low for the first 4 E cycles of
//! every line. NTSC: 262 lines/field (14,934 E cycles, 59.92 Hz). FS rises
//! at the start of vertical blanking and falls at the end of the active
//! area 230 lines later (low for 32 lines). The Dragon's PAL circuit adds
//! 50 lines: 312 lines/field (17,784 E cycles, 49.97 Hz), FS low 57 lines.
//!
//! SAM MPU rate: R1 = 1 → every cycle fast (1.79 MHz); R1R0 = 01 →
//! address-dependent: RAM and PIA0 (`$FF00-$FF1F`) accesses are slow, ROM,
//! other I/O and the CPU's dead cycles ($FFFF) fast (as XRoar). The board
//! counts the slow bus accesses of every instruction to convert its cycles
//! into real time; `cpu_clock_hz` reports the rate measured over the last
//! field (interleave penalties are not modelled).

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

use crate::basic_rom;
use crate::board_pia::BoardPia;
use crate::keyboard::{KeyboardLayout, KeyboardMatrix};
use crate::peripherals::{BoardKind, Peripherals, PiaPins};
use crate::sam::{CpuRate, Sam};
use crate::vdg::VdgInputs;
use m6809_core::{IoRegisterView, IoWriteResult};
use serde::{Deserialize, Serialize};

/// NTSC E clock: 14.31818 MHz / 16.
pub const CPU_CLOCK_HZ: u32 = 894_886;

/// Board ticks per slow E cycle (a fast cycle is one tick).
pub(crate) const TICKS_PER_E: u32 = 2;
/// One scan line: 228 VDG clocks = 57 E cycles.
pub(crate) const TICKS_PER_LINE: u32 = 57 * TICKS_PER_E;
/// HS is low for 16 VDG clocks = 4 E cycles at the start of each line.
pub(crate) const HS_LOW_TICKS: u32 = 4 * TICKS_PER_E;
/// What a read of an address nobody drives returns.
pub(crate) const OPEN_BUS: u8 = 0xFF;

/// Machine-specific wiring of the shared CoCo / Dragon board.
pub(crate) trait BoardSpec: Default + Clone + fmt::Debug + 'static {
    /// `MemoryIo::kind_id`.
    const KIND_ID: &'static str;
    /// Peripheral signal routing.
    const PERIPHERALS: BoardKind;
    /// Keyboard matrix wiring.
    const LAYOUT: KeyboardLayout;
    /// E clock at the slow SAM rate (Hz).
    const BASE_CLOCK_HZ: u32;
    /// Scan lines per field.
    const LINES_PER_FIELD: u32;
    /// Lines from the FS rising edge (start of vertical blanking) to the FS
    /// falling edge (end of the active area).
    const FS_HIGH_LINES: u32;
    /// RAM address lines decoded by the fitted RAM.
    const RAM_MASK: u16;
    /// ROM byte for `$8000-$BFFF`.
    fn rom_byte(addr: u16) -> u8;
    /// Level of the RAM size sense input on PIA1 PB2.
    fn ram_size_sense(pia0: &BoardPia) -> bool;

    fn default_peripherals() -> Peripherals {
        Peripherals::new(Self::PERIPHERALS)
    }

    /// Board ticks per field.
    fn field_ticks() -> u32 {
        Self::LINES_PER_FIELD * TICKS_PER_LINE
    }
}

/// CoCo 2 with 64K RAM (4164 chips), NTSC.
#[derive(Debug, Clone, Default)]
pub(crate) struct CocoSpec;

impl BoardSpec for CocoSpec {
    const KIND_ID: &'static str = "coco2";
    const PERIPHERALS: BoardKind = BoardKind::Coco2;
    const LAYOUT: KeyboardLayout = KeyboardLayout::Coco;
    const BASE_CLOCK_HZ: u32 = CPU_CLOCK_HZ;
    const LINES_PER_FIELD: u32 = 262;
    const FS_HIGH_LINES: u32 = 230;
    const RAM_MASK: u16 = 0xFFFF;

    fn rom_byte(addr: u16) -> u8 {
        basic_rom::coco_rom_byte(addr)
    }

    /// 64K CoCo: PIA1 PB2 is linked to PIA0 PB6, so it reads high only while
    /// PB6 drives high (Color BASIC 1.2 toggles PB6 to detect 64K RAM).
    fn ram_size_sense(pia0: &BoardPia) -> bool {
        pia0.port_b_driven_high() & 0x40 != 0
    }
}

/// VDG sync position within the field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct VideoTiming {
    /// Scan line within the field; line 0 starts with the FS rising edge.
    line: u32,
    /// Ticks since the HS falling edge that started the line.
    tick: u32,
    /// FS rising edges since power-up.
    fields: u64,
    /// HS falling edges since power-up.
    lines: u64,
}

/// External pin levels of both PIAs as the CPU would read them.
#[derive(Debug, Clone, Copy)]
struct PinInputs {
    pia0_a: u8,
    pia0_b: u8,
    pia1_a: u8,
    pia1_b: u8,
}

/// Shared CoCo / Dragon board state. Everything lives behind the
/// machine's `RefCell` because `MemoryIo::read` takes `&self`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub(crate) struct BoardCore<S: BoardSpec> {
    pia0: BoardPia,
    pia1: BoardPia,
    sam: Sam,
    #[serde(default)]
    keyboard: KeyboardMatrix,
    #[serde(default)]
    video: VideoTiming,
    #[serde(default = "S::default_peripherals")]
    peripherals: Peripherals,
    /// Slow bus accesses (RAM, PIA0) since the last tick; only matters in
    /// the SAM's address-dependent rate.
    #[serde(skip)]
    slow_accesses: u32,
    /// CPU cycles / ticks of the current field (rate measurement).
    #[serde(default)]
    rate_cycles: u64,
    #[serde(default)]
    rate_ticks: u64,
    /// Effective E clock measured over the last complete field.
    #[serde(default)]
    measured_clock_hz: u32,
    #[serde(skip)]
    spec: PhantomData<S>,
}

impl<S: BoardSpec> BoardCore<S> {
    pub(crate) fn new() -> Self {
        let mut core = Self {
            pia0: BoardPia::new(),
            pia1: BoardPia::new(),
            sam: Sam::new(),
            keyboard: KeyboardMatrix::default(),
            video: VideoTiming::default(),
            peripherals: S::default_peripherals(),
            slow_accesses: 0,
            rate_cycles: 0,
            rate_ticks: 0,
            measured_clock_hz: 0,
            spec: PhantomData,
        };
        core.reset();
        core
    }

    /// Minimum time a host key stays down (and up): two fields.
    fn min_key_hold_ticks() -> u64 {
        2 * S::field_ticks() as u64
    }

    /// Hardware RESET: both PIAs and the SAM return to their reset state
    /// (MAME and XRoar clear the SAM on every reset), the VDG restarts its
    /// field and the peripherals reset. Keys held on the host stay down.
    pub(crate) fn reset(&mut self) {
        self.pia0.reset();
        self.pia1.reset();
        self.sam.reset();
        self.video.line = 0;
        self.video.tick = 0;
        // Line 0 starts with HS low and FS high.
        self.pia0.preset_control_inputs(false, true);
        self.slow_accesses = 0;
        self.rate_cycles = 0;
        self.rate_ticks = 0;
        self.peripherals.reset();
    }

    // ---- time -------------------------------------------------------------

    pub(crate) fn tick(&mut self, cycles: u32) {
        let ticks = match self.sam.cpu_rate() {
            CpuRate::Slow => cycles * TICKS_PER_E,
            CpuRate::Fast => cycles,
            CpuRate::AddressDependent => cycles + self.slow_accesses.min(cycles),
        };
        self.slow_accesses = 0;
        self.rate_cycles += cycles as u64;
        self.rate_ticks += ticks as u64;
        self.advance_video(ticks);
        self.keyboard.advance(ticks as u64);

        let pins = self.pins();
        // (ticks, 2 × base clock) is the elapsed real time.
        self.peripherals
            .tick(ticks, TICKS_PER_E * S::BASE_CLOCK_HZ, &pins);
        let inputs = self.peripherals.inputs(&pins);
        self.pia1.set_ca1(inputs.pia1_ca1);
        self.pia1.set_cb1(inputs.pia1_cb1);
    }

    fn advance_video(&mut self, mut ticks: u32) {
        while ticks > 0 {
            let boundary = if self.video.tick < HS_LOW_TICKS {
                HS_LOW_TICKS
            } else {
                TICKS_PER_LINE
            };
            let step = boundary - self.video.tick;
            if ticks < step {
                self.video.tick += ticks;
                return;
            }
            ticks -= step;
            if boundary == HS_LOW_TICKS {
                // HS rising edge.
                self.video.tick = HS_LOW_TICKS;
                self.pia0.set_ca1(true);
                continue;
            }
            // Next line: HS falling edge.
            self.video.tick = 0;
            self.video.line += 1;
            if self.video.line >= S::LINES_PER_FIELD {
                self.video.line = 0;
            }
            self.video.lines += 1;
            self.pia0.set_ca1(false);
            if self.video.line == 0 {
                // FS rising edge: start of vertical blanking.
                self.video.fields += 1;
                self.pia0.set_cb1(true);
                self.end_of_field();
            } else if self.video.line == S::FS_HIGH_LINES {
                // FS falling edge: end of the active display area.
                self.pia0.set_cb1(false);
            }
        }
    }

    fn end_of_field(&mut self) {
        if self.rate_ticks > 0 {
            let hz = TICKS_PER_E as u64 * S::BASE_CLOCK_HZ as u64 * self.rate_cycles
                / self.rate_ticks;
            self.measured_clock_hz = hz.min(u32::MAX as u64) as u32;
        }
        self.rate_cycles = 0;
        self.rate_ticks = 0;
    }

    /// Effective CPU E clock: 0.89 MHz slow, twice that with R1, and the
    /// rate measured over the last field in the address-dependent mode.
    pub(crate) fn cpu_clock_hz(&self) -> u32 {
        let base = S::BASE_CLOCK_HZ;
        match self.sam.cpu_rate() {
            CpuRate::Slow => base,
            CpuRate::Fast => 2 * base,
            CpuRate::AddressDependent => {
                let measured = self.measured_clock_hz;
                if (base..=2 * base).contains(&measured) {
                    measured
                } else {
                    base + base / 2
                }
            }
        }
    }

    // ---- PIA pins ---------------------------------------------------------

    /// Pin levels driven by the PIAs (outputs where DDR = 1, high otherwise).
    fn pins(&self) -> PiaPins {
        PiaPins {
            pia0_a: self.pia0.port_a_pins(),
            pia0_b: self.pia0.port_b_pins(),
            pia0_ca2: self.pia0.ca2_output(),
            pia0_cb2: self.pia0.cb2_output(),
            pia1_a: self.pia1.port_a_pins(),
            pia1_b: self.pia1.port_b_pins(),
            pia1_ca2: self.pia1.ca2_output(),
            pia1_cb2: self.pia1.cb2_output(),
        }
    }

    /// External levels on the PIA port pins: keyboard (both directions),
    /// joystick comparator and buttons, cassette, serial / printer, RAM sense.
    fn pin_inputs(&self) -> PinInputs {
        let inputs = self.peripherals.inputs(&self.pins());
        // Fire buttons pull PA0 (right) / PA1 (left) low.
        let buttons_low = !inputs.pia0_pa_buttons & 0x03;
        let rows_low = self.pia0.port_a_driven_low() | buttons_low;
        let cols_low = self.pia0.port_b_driven_low();
        let mut pia0_a = !(self.keyboard.rows_pulled_low(cols_low) | buttons_low);
        if !inputs.pia0_pa7 {
            pia0_a &= 0x7F;
        }
        let pia0_b = !self.keyboard.cols_pulled_low(rows_low);
        let pia1_a = 0xFE | inputs.pia1_pa0 as u8;
        // PIA1 port B has no pull-ups: undriven inputs read low.
        let mut pia1_b = inputs.pia1_pb0 as u8;
        if S::ram_size_sense(&self.pia0) {
            pia1_b |= 0x04;
        }
        PinInputs {
            pia0_a,
            pia0_b,
            pia1_a,
            pia1_b,
        }
    }

    fn apply_pin_inputs(&mut self) {
        let inputs = self.pin_inputs();
        self.pia0.set_ira(inputs.pia0_a);
        self.pia0.set_irb(inputs.pia0_b);
        self.pia1.set_ira(inputs.pia1_a);
        self.pia1.set_irb(inputs.pia1_b);
    }

    // ---- memory map -------------------------------------------------------

    /// Physical RAM address of a CPU RAM access (P1 page, RAM decode).
    fn ram_phys(&self, addr: u16) -> u16 {
        let addr = if addr < 0x8000 && self.sam.page1_active() {
            addr | 0x8000
        } else {
            addr
        };
        addr & S::RAM_MASK
    }

    fn ram_read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        let phys = self.ram_phys(addr);
        if phys == addr {
            None
        } else {
            Some(ram[phys as usize])
        }
    }

    fn ram_write(&self, addr: u16, value: u8, ram: &mut [u8; 0x10000]) -> IoWriteResult {
        let phys = self.ram_phys(addr);
        if phys == addr {
            IoWriteResult::PassThrough
        } else {
            ram[phys as usize] = value;
            IoWriteResult::Consumed
        }
    }

    /// Map type 0 `$8000-$FEFF`: BASIC ROM, then the cartridge.
    fn rom_read(&self, addr: u16) -> u8 {
        if addr < 0xC000 {
            S::rom_byte(addr)
        } else {
            self.peripherals.cartridge_read(addr).unwrap_or(OPEN_BUS)
        }
    }

    /// CPU bus read (PIA data reads acknowledge interrupts).
    pub(crate) fn cpu_read(&mut self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        match addr {
            0x0000..=0x7FFF => {
                self.slow_accesses += 1;
                self.ram_read(addr, ram)
            }
            0x8000..=0xFEFF => {
                if self.sam.map_type_all_ram() {
                    self.slow_accesses += 1;
                    self.ram_read(addr, ram)
                } else {
                    Some(self.rom_read(addr))
                }
            }
            0xFF00..=0xFF1F => {
                self.slow_accesses += 1;
                self.apply_pin_inputs();
                Some(self.pia0.read((addr & 3) as u8))
            }
            0xFF20..=0xFF3F => {
                self.apply_pin_inputs();
                Some(self.pia1.read((addr & 3) as u8))
            }
            0xFFE0..=0xFFFF => Some(S::rom_byte(0xBFE0 | (addr & 0x1F))),
            // $FF40-$FF5F cartridge I/O, $FF60-$FFBF unused, $FFC0-$FFDF SAM.
            _ => Some(OPEN_BUS),
        }
    }

    /// Side-effect-free read for the debugger.
    pub(crate) fn peek(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
        match addr {
            0x0000..=0x7FFF => self.ram_read(addr, ram),
            0x8000..=0xFEFF => {
                if self.sam.map_type_all_ram() {
                    self.ram_read(addr, ram)
                } else {
                    Some(self.rom_read(addr))
                }
            }
            0xFF00..=0xFF1F => {
                let inputs = self.pin_inputs();
                Some(
                    self.pia0
                        .peek_with_inputs((addr & 3) as u8, inputs.pia0_a, inputs.pia0_b),
                )
            }
            0xFF20..=0xFF3F => {
                let inputs = self.pin_inputs();
                Some(
                    self.pia1
                        .peek_with_inputs((addr & 3) as u8, inputs.pia1_a, inputs.pia1_b),
                )
            }
            0xFFE0..=0xFFFF => Some(S::rom_byte(0xBFE0 | (addr & 0x1F))),
            _ => Some(OPEN_BUS),
        }
    }

    pub(crate) fn cpu_write(
        &mut self,
        addr: u16,
        value: u8,
        ram: &mut [u8; 0x10000],
    ) -> IoWriteResult {
        match addr {
            0x0000..=0x7FFF => {
                self.slow_accesses += 1;
                self.ram_write(addr, value, ram)
            }
            0x8000..=0xFEFF => {
                if self.sam.map_type_all_ram() {
                    self.slow_accesses += 1;
                    self.ram_write(addr, value, ram)
                } else {
                    IoWriteResult::Ignored
                }
            }
            0xFF00..=0xFF1F => {
                self.slow_accesses += 1;
                self.pia0.write((addr & 3) as u8, value);
                IoWriteResult::Consumed
            }
            0xFF20..=0xFF3F => {
                self.pia1.write((addr & 3) as u8, value);
                IoWriteResult::Consumed
            }
            0xFFC0..=0xFFDF => {
                self.sam.write(addr);
                IoWriteResult::Consumed
            }
            // Cartridge I/O, unused space and the ROM vectors.
            _ => IoWriteResult::Ignored,
        }
    }

    // ---- outputs ----------------------------------------------------------

    /// /IRQ: PIA0 IRQA (HS) | IRQB (FS).
    pub(crate) fn irq(&self) -> bool {
        self.pia0.irq_asserted()
    }

    /// /FIRQ: PIA1 IRQA (CD / ACK) | IRQB (CART*).
    pub(crate) fn firq(&self) -> bool {
        self.pia1.irq_asserted()
    }

    /// SAM and PIA1 port B signals for the VDG. The PIA1 port B pins have
    /// no pull-ups, so bits configured as inputs are seen low (as MAME and
    /// XRoar do): after RESET the VDG shows alphanumerics.
    pub(crate) fn vdg_inputs(&self) -> VdgInputs {
        VdgInputs {
            sam_v: self.sam.v_mode_bits(),
            sam_f: self.sam.f_bits(),
            vdg_ctrl: self.pia1.port_b_outputs(),
        }
    }

    // ---- keyboard ---------------------------------------------------------

    pub(crate) fn host_key_event(&mut self, code: &str, key: Option<&str>, down: bool) {
        self.keyboard.set_min_hold(Self::min_key_hold_ticks());
        self.keyboard.host_event(S::LAYOUT, code, key, down);
    }

    pub(crate) fn clear_keys(&mut self) {
        self.keyboard.clear();
    }

    pub(crate) fn peripherals_mut(&mut self) -> &mut Peripherals {
        &mut self.peripherals
    }

    // ---- debugger ---------------------------------------------------------

    pub(crate) fn io_registers(&self) -> Vec<IoRegisterView> {
        let inputs = self.pin_inputs();
        let reg = |address: u16, name: String, value: u8| IoRegisterView {
            address,
            name,
            value,
        };
        let sam = &self.sam;
        vec![
            reg(
                0xFF00,
                format!("PIA0 PA kbd rows/joy (DDRA ${:02X})", self.pia0.ddra),
                self.pia0.port_a_read(inputs.pia0_a),
            ),
            reg(0xFF01, "PIA0 CRA (HS)".into(), self.pia0.peek(1)),
            reg(
                0xFF02,
                format!("PIA0 PB kbd cols (DDRB ${:02X})", self.pia0.ddrb),
                self.pia0.port_b_read(inputs.pia0_b),
            ),
            reg(0xFF03, "PIA0 CRB (FS)".into(), self.pia0.peek(3)),
            reg(
                0xFF20,
                format!("PIA1 PA DAC/cass (DDRA ${:02X})", self.pia1.ddra),
                self.pia1.port_a_read(inputs.pia1_a),
            ),
            reg(0xFF21, "PIA1 CRA".into(), self.pia1.peek(1)),
            reg(
                0xFF22,
                format!("PIA1 PB VDG (DDRB ${:02X})", self.pia1.ddrb),
                self.pia1.port_b_read(inputs.pia1_b),
            ),
            reg(0xFF23, "PIA1 CRB".into(), self.pia1.peek(3)),
            reg(0xFFC0, "SAM V2-V0".into(), sam.v_mode_bits()),
            reg(0xFFC6, "SAM F6-F0 (video/512)".into(), sam.f_bits()),
            reg(0xFFD4, "SAM P1".into(), sam.page1() as u8),
            reg(0xFFD6, "SAM R1R0 (rate)".into(), sam.rate_bits()),
            reg(0xFFDA, "SAM M1M0 (RAM size)".into(), sam.memory_size_bits()),
            reg(0xFFDE, "SAM TY (map type)".into(), sam.map_type_all_ram() as u8),
        ]
    }

    /// FS rising edges since power-up.
    #[allow(dead_code)]
    pub(crate) fn field_count(&self) -> u64 {
        self.video.fields
    }

    /// HS falling edges since power-up.
    #[allow(dead_code)]
    pub(crate) fn line_count(&self) -> u64 {
        self.video.lines
    }

    #[allow(dead_code)]
    pub(crate) fn sam(&self) -> &Sam {
        &self.sam
    }
}

/// Public board API and `MemoryIo` for a machine type that wraps the shared
/// core as `inner: RefCell<BoardCore<Spec>>` (used by `Coco2Machine` and
/// `Dragon32Machine`).
macro_rules! board_machine_impl {
    ($machine:ident, $spec:ty) => {
        impl Default for $machine {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $machine {
            pub fn new() -> Self {
                Self {
                    inner: std::cell::RefCell::new($crate::coco2::BoardCore::<$spec>::new()),
                }
            }

            /// Positional host key event (`code` = KeyboardEvent.code).
            pub fn host_key(&mut self, code: &str, down: bool) {
                self.host_key_event(code, None, down);
            }

            /// Host keyboard event: `code` = KeyboardEvent.code, `key` =
            /// KeyboardEvent.key (typed characters are mapped to the keys that
            /// produce them on this machine, independent of the host layout).
            pub fn host_key_event(&mut self, code: &str, key: Option<&str>, down: bool) {
                self.inner.get_mut().host_key_event(code, key, down);
            }

            /// Release every key at once (focus loss).
            pub fn clear_keys(&mut self) {
                self.inner.get_mut().clear_keys();
            }

            /// Advance the board by `cycles` CPU (E) cycles.
            pub fn board_tick(&mut self, cycles: u32) {
                self.inner.get_mut().tick(cycles);
            }

            /// Run `f` on the peripherals (joystick, cassette, printer, cartridge).
            pub fn with_peripherals<R>(
                &self,
                f: impl FnOnce(&mut $crate::peripherals::Peripherals) -> R,
            ) -> R {
                let mut inner = self.inner.borrow_mut();
                f(inner.peripherals_mut())
            }

            /// Level of the /IRQ line: PIA0 IRQA (HS) | IRQB (FS).
            pub fn board_poll_irq(&mut self) -> bool {
                self.inner.get_mut().irq()
            }

            /// Level of the /FIRQ line: PIA1 IRQA | IRQB (CART*).
            pub fn board_poll_firq(&mut self) -> bool {
                self.inner.get_mut().firq()
            }

            /// Hardware RESET: PIAs, SAM, video timing and peripherals reset;
            /// held keys stay down.
            pub fn board_reset(&mut self) {
                self.inner.get_mut().reset();
            }

            /// Effective CPU E clock in Hz (SAM rate aware).
            pub fn cpu_clock_hz(&self) -> u32 {
                self.inner.borrow().cpu_clock_hz()
            }

            /// Current VDG / SAM video inputs for the renderer.
            pub fn vdg_inputs(&self) -> $crate::vdg::VdgInputs {
                self.inner.borrow().vdg_inputs()
            }

            /// Board audio (6-bit DAC / single-bit sound) at `AUDIO_SAMPLE_RATE`.
            pub fn drain_audio(&mut self) -> Vec<f32> {
                self.inner.get_mut().peripherals_mut().drain_audio()
            }
        }

        impl ::m6809_core::MemoryIo for $machine {
            fn kind_id(&self) -> &str {
                <$spec as $crate::coco2::BoardSpec>::KIND_ID
            }

            fn read(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
                self.inner.borrow_mut().cpu_read(addr, ram)
            }

            fn peek(&self, addr: u16, ram: &[u8; 0x10000]) -> Option<u8> {
                self.inner.borrow().peek(addr, ram)
            }

            fn write(
                &mut self,
                addr: u16,
                value: u8,
                ram: &mut [u8; 0x10000],
            ) -> ::m6809_core::IoWriteResult {
                self.inner.get_mut().cpu_write(addr, value, ram)
            }

            fn clone_box(&self) -> Box<dyn ::m6809_core::MemoryIo> {
                Box::new(self.clone())
            }

            fn snapshot(&self) -> serde_json::Value {
                serde_json::to_value(self).unwrap_or_default()
            }

            fn restore(&mut self, snapshot: &serde_json::Value) {
                if let Ok(state) = serde_json::from_value(snapshot.clone()) {
                    *self = state;
                }
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }

            fn io_registers(&self) -> Vec<::m6809_core::IoRegisterView> {
                self.inner.borrow().io_registers()
            }

            fn tick(&mut self, cycles: u32) {
                self.board_tick(cycles);
            }

            fn poll_irq(&mut self) -> bool {
                self.board_poll_irq()
            }

            fn poll_firq(&mut self) -> bool {
                self.board_poll_firq()
            }

            fn reset(&mut self) {
                self.board_reset();
            }

            fn cpu_clock_hz(&self) -> Option<u32> {
                Some($machine::cpu_clock_hz(self))
            }
        }
    };
}
pub(crate) use board_machine_impl;

/// TRS-80 Color Computer 2, 64K, NTSC, Extended Color BASIC 1.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coco2Machine {
    inner: RefCell<BoardCore<CocoSpec>>,
}

board_machine_impl!(Coco2Machine, CocoSpec);

#[cfg(test)]
pub(crate) mod test_support {
    //! Helpers for board tests: a bare board plugged straight into an
    //! `Emulator`, running the real BASIC ROMs.

    use m6809_asm::assemble;
    use m6809_core::{Emulator, MemoryIo};

    use crate::vdg::VdgInputs;

    /// What the shared tests need from a CoCo / Dragon board.
    pub(crate) trait TestBoard: MemoryIo + Sized + 'static {
        /// E cycles per field at the slow rate.
        const FIELD_CYCLES: u64;
        fn fresh() -> Self;
        fn key(&mut self, code: &str, key: Option<&str>, down: bool);
        fn video(&self) -> VdgInputs;
        fn fields(&self) -> u64;
        fn lines(&self) -> u64;
        fn sam_bits(&self) -> u16;
    }

    pub fn emu_with<B: TestBoard>() -> Emulator {
        let mut emu = Emulator::new();
        emu.memory.io = Some(Box::new(B::fresh()));
        emu.reset();
        emu
    }

    pub fn board<B: TestBoard>(emu: &Emulator) -> &B {
        emu.memory
            .io
            .as_ref()
            .and_then(|io| io.as_any().downcast_ref::<B>())
            .expect("board")
    }

    pub fn board_mut<B: TestBoard>(emu: &mut Emulator) -> &mut B {
        emu.memory
            .io
            .as_mut()
            .and_then(|io| io.as_any_mut().downcast_mut::<B>())
            .expect("board")
    }

    /// Run the CPU for at least `cycles` E cycles.
    pub fn run_cycles(emu: &mut Emulator, cycles: u64) {
        let start = emu.cpu.total_cycles;
        while emu.cpu.total_cycles - start < cycles {
            emu.step();
        }
    }

    /// MC6847 internal character set as used by BASIC: bits 0-5 select the
    /// glyph, bit 6 set = normal video, bit 7 = semigraphics. Inverse-video
    /// letters (lower case in BASIC) are returned in lower case.
    pub fn decode_cell(b: u8) -> char {
        if b & 0x80 != 0 {
            return '#';
        }
        let code = b & 0x3F;
        let ch = if code < 0x20 { code + 0x40 } else { code } as char;
        if b & 0x40 == 0 && ch.is_ascii_uppercase() {
            ch.to_ascii_lowercase()
        } else {
            ch
        }
    }

    /// 32×16 text page at `base`.
    pub fn text_at(ram: &[u8; 0x10000], base: usize) -> String {
        let mut out = String::new();
        for row in 0..16 {
            for col in 0..32 {
                out.push(decode_cell(ram[(base + row * 32 + col) & 0xFFFF]));
            }
            out.push('\n');
        }
        out
    }

    /// Text page at the SAM display offset.
    pub fn screen_text<B: TestBoard>(emu: &Emulator) -> String {
        let base = board::<B>(emu).video().sam_f as usize * 512;
        text_at(&emu.memory.ram, base)
    }

    /// The first screen line containing `needle` (or the whole screen).
    pub fn screen_line_with<B: TestBoard>(emu: &Emulator, needle: &str) -> String {
        let screen = screen_text::<B>(emu);
        screen
            .lines()
            .find(|l| l.contains(needle))
            .map(|l| l.to_string())
            .unwrap_or(screen)
    }

    /// Cold-start BASIC and run until the `OK` prompt is on screen.
    pub fn boot_basic<B: TestBoard>() -> Emulator {
        let mut emu = emu_with::<B>();
        for _ in 0..60 {
            run_cycles(&mut emu, 50_000);
            if screen_text::<B>(&emu).contains("OK") {
                run_cycles(&mut emu, 50_000);
                return emu;
            }
        }
        panic!("BASIC did not boot:\n{}", screen_text::<B>(&emu));
    }

    /// Press and release a host key at once (a tap shorter than any
    /// keyboard scan) and give BASIC time to see it.
    pub fn tap<B: TestBoard>(emu: &mut Emulator, code: &str, key: Option<&str>) {
        let m = board_mut::<B>(emu);
        m.key(code, key, true);
        m.key(code, key, false);
        run_cycles(emu, 4 * B::FIELD_CYCLES);
    }

    /// Type text through the character (`key`) mapping.
    pub fn type_text<B: TestBoard>(emu: &mut Emulator, text: &str) {
        for ch in text.chars() {
            let code = if ch.is_ascii_alphabetic() {
                format!("Key{}", ch.to_ascii_uppercase())
            } else if ch.is_ascii_digit() {
                format!("Digit{ch}")
            } else if ch == ' ' {
                "Space".to_string()
            } else {
                format!("Char{}", ch as u32)
            };
            tap::<B>(emu, &code, Some(&ch.to_string()));
        }
    }

    /// Load an FS-interrupt counter: PIA0 CB1 IRQ on the falling edge, the
    /// handler counts in $0300 and acknowledges by reading $FF02 (or not).
    pub fn load_irq_counter(emu: &mut Emulator, acknowledge: bool) {
        let ack = if acknowledge { "LDA $FF02" } else { "NOP" };
        let src = format!(
            "
        ORG $0200
START   LDS  #$0400
        LDA  #$34
        STA  $FF01
        LDA  #$35
        STA  $FF03
        LDA  $FF02
        ANDCC #$EF
LOOP    BRA  LOOP
HANDLER LDX  $0300
        LEAX 1,X
        STX  $0300
        {ack}
        RTI
        END
"
        );
        let program = assemble(&src).expect("assemble irq counter");
        emu.load_program(program.origin, &program.bytes).expect("load");
        let pos = program
            .bytes
            .windows(3)
            .position(|w| w == [0xBE, 0x03, 0x00])
            .expect("handler");
        let handler = program.origin + pos as u16;
        // BASIC's IRQ vector points into RAM ($010C): put a JMP there.
        let vector = emu.memory.read16(0xFFF8) as usize;
        emu.memory.ram[vector] = 0x7E;
        emu.memory.ram[vector + 1] = (handler >> 8) as u8;
        emu.memory.ram[vector + 2] = handler as u8;
        emu.cpu.pc = program.origin;
    }

    /// `BRA *` at $0400 as the running program.
    pub fn idle_loop(emu: &mut Emulator) {
        emu.memory.ram[0x0400] = 0x20;
        emu.memory.ram[0x0401] = 0xFE;
        emu.cpu.pc = 0x0400;
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::{apply_machine, machine_container, machine_host_key_event, MachineKind};
    use m6809_core::{Emulator, MemoryIo};

    type M = Coco2Machine;
    const FIELD: u64 = 262 * 57;

    impl TestBoard for Coco2Machine {
        const FIELD_CYCLES: u64 = 262 * 57;

        fn fresh() -> Self {
            Coco2Machine::new()
        }

        fn key(&mut self, code: &str, key: Option<&str>, down: bool) {
            self.host_key_event(code, key, down);
        }

        fn video(&self) -> VdgInputs {
            self.vdg_inputs()
        }

        fn fields(&self) -> u64 {
            self.inner.borrow().field_count()
        }

        fn lines(&self) -> u64 {
            self.inner.borrow().line_count()
        }

        fn sam_bits(&self) -> u16 {
            self.inner.borrow().sam().bits()
        }
    }

    #[test]
    fn standalone_board_basics() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        // Vectors come from Color BASIC's last 32 bytes.
        assert_eq!(m.read(0xFFFE, &ram), Some(0xA0));
        assert_eq!(m.read(0xFFFF, &ram), Some(0x27));
        assert_eq!(m.write(0xFFF8, 0x12, &mut ram), IoWriteResult::Ignored);
        assert_eq!(m.read(0xFFF8, &ram), Some(basic_rom::coco_rom_byte(0xBFF8)));
        // RAM passes through, ROM is write-protected.
        assert_eq!(m.read(0x1234, &ram), None);
        assert_eq!(m.write(0x1234, 1, &mut ram), IoWriteResult::PassThrough);
        assert_eq!(m.write(0xA000, 0, &mut ram), IoWriteResult::Ignored);
        assert_eq!(m.read(0xA000, &ram), Some(basic_rom::COCO_COLOR_BASIC[0]));
        assert_eq!(m.read(0x8000, &ram), Some(basic_rom::COCO_EXTENDED_BASIC[0]));
        assert_eq!(m.kind_id(), "coco2");
        assert_eq!(Coco2Machine::cpu_clock_hz(&m), 894_886);
    }

    #[test]
    fn io_holes_are_not_ram() {
        let mut emu = emu_with::<M>();
        for addr in [0xFF40u16, 0xFF41, 0xFF5F, 0xFF60, 0xFF80, 0xFFBF] {
            emu.memory.write8(addr, 0x5A);
            assert_eq!(emu.memory.read8(addr), OPEN_BUS, "${addr:04X}");
            assert_eq!(emu.memory.ram[addr as usize], 0, "${addr:04X} reached RAM");
        }
        // SAM register space is write-only.
        assert_eq!(emu.memory.read8(0xFFC9), OPEN_BUS);
        // No cartridge: $C000-$FEFF is open bus and not writable.
        emu.memory.write8(0xC000, 0x12);
        assert_eq!(emu.memory.read8(0xC000), OPEN_BUS);
        assert_eq!(emu.memory.read8(0xFEFF), OPEN_BUS);
        assert_eq!(emu.memory.ram[0xC000], 0);
    }

    #[test]
    fn vectors_come_from_rom_and_ignore_writes() {
        let mut emu = emu_with::<M>();
        assert_eq!(emu.cpu.pc, 0xA027, "RESET vector fetched through the board");
        let irq = emu.memory.read16(0xFFF8);
        assert_eq!(
            irq,
            u16::from_be_bytes([basic_rom::coco_rom_byte(0xBFF8), basic_rom::coco_rom_byte(0xBFF9)])
        );
        emu.memory.write16(0xFFF8, 0x1234);
        assert_eq!(emu.memory.read16(0xFFF8), irq);
        assert_eq!(emu.memory.ram[0xFFF8], 0x00, "vector writes are discarded");
    }

    #[test]
    fn rom_is_write_protected_and_separate_from_ram() {
        let mut emu = emu_with::<M>();
        let rom = emu.memory.read8(0xA000);
        emu.memory.write8(0xA000, !rom);
        assert_eq!(emu.memory.read8(0xA000), rom);
        assert_eq!(emu.memory.ram[0xA000], 0, "ROM image not copied into RAM");
        assert_eq!(emu.memory.peek16(0xFFFE), 0xA027);
        assert_eq!(emu.memory.peek8(0xA000), rom);
    }

    #[test]
    fn sam_map_type_switches_upper_32k_to_ram() {
        let mut emu = emu_with::<M>();
        let rom = emu.memory.read8(0x8000);
        emu.memory.write8(0x8000, 0x55); // TY=0: ignored
        assert_eq!(emu.memory.read8(0x8000), rom);
        emu.memory.write8(0xFFDF, 0); // TY=1 (all RAM)
        assert_eq!(emu.memory.read8(0x8000), 0x00, "RAM, not ROM");
        emu.memory.write8(0x8000, 0x55);
        emu.memory.write8(0xFEFF, 0x66);
        assert_eq!(emu.memory.read8(0x8000), 0x55);
        assert_eq!(emu.memory.read8(0xFEFF), 0x66);
        assert_eq!(emu.memory.ram[0x8000], 0x55);
        // Vectors and I/O are unaffected by TY.
        assert_eq!(emu.memory.read16(0xFFFE), 0xA027);
        // ROM-to-RAM copy idiom: read ROM with TY=0, write RAM with TY=1.
        emu.memory.write8(0xFFDE, 0);
        let b = emu.memory.read8(0xA123);
        emu.memory.write8(0xFFDF, 0);
        emu.memory.write8(0xA123, b ^ 0xFF);
        assert_eq!(emu.memory.read8(0xA123), b ^ 0xFF);
        emu.memory.write8(0xFFDE, 0); // back to ROM
        assert_eq!(emu.memory.read8(0x8000), rom);
        assert_eq!(emu.memory.read8(0xA123), b);
    }

    #[test]
    fn sam_page1_maps_lower_32k_to_upper_ram() {
        let mut emu = emu_with::<M>();
        emu.memory.write8(0x0100, 0x11);
        emu.memory.write8(0xFFD5, 0); // P1 without 64K mode: no effect
        assert_eq!(emu.memory.read8(0x0100), 0x11);
        emu.memory.write8(0xFFDD, 0); // M1: 64K mode
        emu.memory.write8(0x0100, 0x22);
        assert_eq!(emu.memory.ram[0x8100], 0x22, "page 1 is the upper 32K");
        assert_eq!(emu.memory.ram[0x0100], 0x11);
        assert_eq!(emu.memory.read8(0x0100), 0x22);
        assert_eq!(emu.memory.peek8(0x0100), 0x22);
        emu.memory.write8(0xFFD4, 0); // P1 off
        assert_eq!(emu.memory.read8(0x0100), 0x11);
    }

    #[test]
    fn sam_speed_pokes_change_clock_but_not_video_timing() {
        let mut emu = emu_with::<M>();
        idle_loop(&mut emu);
        assert_eq!(emu.cpu_clock_hz(), Some(894_886));
        emu.memory.write8(0xFFD9, 0); // R1: fast (POKE 65497,0)
        assert_eq!(emu.cpu_clock_hz(), Some(2 * 894_886));
        let f0 = board::<M>(&emu).fields();
        run_cycles(&mut emu, 2 * 10 * FIELD);
        let fields = board::<M>(&emu).fields() - f0;
        assert!((9..=11).contains(&fields), "20 fields of fast cycles = {fields} fields");
        emu.memory.write8(0xFFD8, 0); // R1 off
        emu.memory.write8(0xFFD7, 0); // R0: address-dependent (POKE 65495,0)
        // BRA * in RAM: 2 slow RAM reads + 1 fast dead cycle = 5 ticks / 3 cycles.
        run_cycles(&mut emu, 3 * FIELD);
        let hz = emu.cpu_clock_hz().unwrap() as f64;
        let expect = 894_886.0 * 2.0 * 3.0 / 5.0;
        assert!((hz - expect).abs() < expect * 0.01, "address-dependent clock {hz}");
        emu.memory.write8(0xFFD6, 0);
        assert_eq!(emu.cpu_clock_hz(), Some(894_886));
    }

    #[test]
    fn hsync_and_fsync_rates() {
        let mut emu = emu_with::<M>();
        idle_loop(&mut emu);
        let (l0, f0) = (board::<M>(&emu).lines(), board::<M>(&emu).fields());
        run_cycles(&mut emu, 894_886);
        let lines = board::<M>(&emu).lines() - l0;
        let fields = board::<M>(&emu).fields() - f0;
        assert!((15_600..=15_800).contains(&lines), "HS per second {lines}");
        assert!((59..=61).contains(&fields), "FS per second {fields}");
        assert_eq!(<CocoSpec as BoardSpec>::field_ticks(), 14_934 * 2);
    }

    #[test]
    fn hs_sets_pia0_ca1_flag_every_line_and_fs_pulse_has_width() {
        let mut emu = emu_with::<M>();
        idle_loop(&mut emu);
        emu.memory.write8(0xFF01, 0x04); // CRA: data, CA1 falling edge, IRQ off
        emu.memory.write8(0xFF03, 0x04); // CRB: data, CB1 falling edge, IRQ off
        let _ = emu.memory.read8(0xFF00);
        run_cycles(&mut emu, 60);
        assert_eq!(emu.memory.read8(0xFF01) & 0x80, 0x80, "HS edge within one line");
        let _ = emu.memory.read8(0xFF00);
        assert_eq!(emu.memory.read8(0xFF01) & 0x80, 0);
        assert!(!emu.cpu.irq_line, "HS IRQ disabled");
        // FS: wait for a falling edge, then measure until the rising edge.
        let _ = emu.memory.read8(0xFF02);
        while emu.memory.read8(0xFF03) & 0x80 == 0 {
            emu.step();
        }
        let fall = emu.cpu.total_cycles;
        emu.memory.write8(0xFF03, 0x06); // CB1 rising edge
        let _ = emu.memory.read8(0xFF02);
        while emu.memory.read8(0xFF03) & 0x80 == 0 {
            emu.step();
        }
        let low = emu.cpu.total_cycles - fall;
        assert!(low.abs_diff(32 * 57) < 20, "FS low for {low} cycles");
    }

    #[test]
    fn fs_irq_handler_runs_once_per_field_60_hz() {
        let mut emu = emu_with::<M>();
        load_irq_counter(&mut emu, true);
        run_cycles(&mut emu, 2_000);
        let f0 = board::<M>(&emu).fields();
        let c0 = emu.memory.read16(0x0300);
        run_cycles(&mut emu, 894_886); // one emulated second
        let count = emu.memory.read16(0x0300) - c0;
        let fields = board::<M>(&emu).fields() - f0;
        assert!((59..=61).contains(&count), "IRQs per second {count}");
        assert!(u64::from(count).abs_diff(fields) <= 1, "one IRQ per field ({count} vs {fields})");
    }

    #[test]
    fn unacknowledged_irq_is_level_triggered() {
        let mut emu = emu_with::<M>();
        load_irq_counter(&mut emu, false);
        run_cycles(&mut emu, 3 * FIELD);
        assert!(emu.memory.read16(0x0300) > 100, "handler re-entered while the level stays asserted");
        assert!(emu.cpu.irq_line);
    }

    #[test]
    fn pia1_interrupts_route_to_firq() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF23, 0x05, &mut ram); // CB1 IRQ on, falling edge
        assert!(!m.board_poll_firq());
        m.inner.get_mut().pia1.set_cb1(false); // CART* edge
        assert!(m.board_poll_firq());
        assert!(!m.board_poll_irq(), "PIA1 is not wired to /IRQ");
        let _ = m.read(0xFF22, &ram);
        assert!(!m.board_poll_firq(), "acknowledged by reading port B");
        // PIA0 goes to /IRQ only.
        m.write(0xFF01, 0x05, &mut ram);
        m.inner.get_mut().pia0.set_ca1(true);
        m.inner.get_mut().pia0.set_ca1(false);
        assert!(m.board_poll_irq());
        assert!(!m.board_poll_firq());
    }

    #[test]
    fn reset_clears_pias_and_sam_but_keeps_keys() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF01, 0x3F, &mut ram);
        m.write(0xFF23, 0x3F, &mut ram);
        m.write(0xFFDF, 0, &mut ram);
        m.write(0xFFC9, 0, &mut ram);
        m.host_key_event("KeyA", Some("a"), true);
        m.board_reset();
        let core = m.inner.borrow();
        assert_eq!(core.pia0.peek(1), 0);
        assert_eq!(core.pia1.peek(3), 0);
        assert_eq!(core.sam.bits(), 0);
        assert!(core.keyboard.is_pressed(0, 1), "held key survives RESET");
    }

    #[test]
    fn keyboard_scan_reads_rows_and_reverse_scan_reads_columns() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        // PIA0: A inputs, B outputs.
        m.write(0xFF00, 0x00, &mut ram);
        m.write(0xFF01, 0x04, &mut ram);
        m.write(0xFF02, 0xFF, &mut ram);
        m.write(0xFF03, 0x04, &mut ram);
        m.host_key_event("KeyA", Some("a"), true); // row 0, col 1
        m.write(0xFF02, 0xFD, &mut ram); // column 1 low
        // PA7 is the joystick comparator (PIA1 port A still inputs: DAC 63,
        // centred sticks read low), so compare the keyboard rows only.
        assert_eq!(m.read(0xFF00, &ram).map(|v| v & 0x7F), Some(0x7E));
        m.write(0xFF02, 0xFE, &mut ram); // column 0 low
        assert_eq!(m.read(0xFF00, &ram).map(|v| v & 0x7F), Some(0x7F));
        // Reverse: rows as outputs, columns as inputs.
        m.write(0xFF01, 0x00, &mut ram);
        m.write(0xFF00, 0x7F, &mut ram);
        m.write(0xFF01, 0x04, &mut ram);
        m.write(0xFF00, 0xFE, &mut ram); // row 0 low
        m.write(0xFF03, 0x00, &mut ram);
        m.write(0xFF02, 0x00, &mut ram); // DDRB inputs
        m.write(0xFF03, 0x04, &mut ram);
        assert_eq!(m.read(0xFF02, &ram), Some(0xFD));
        // SHIFT is on row 6, column 7.
        m.host_key_event("ShiftLeft", Some("Shift"), true);
        m.write(0xFF00, 0xBF, &mut ram); // row 6 low
        assert_eq!(m.read(0xFF02, &ram), Some(0x7F));
        assert_eq!(m.peek(0xFF02, &ram), Some(0x7F));
    }

    #[test]
    fn peek_does_not_acknowledge_interrupts() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF03, 0x05, &mut ram);
        m.inner.get_mut().pia0.set_cb1(true);
        m.inner.get_mut().pia0.set_cb1(false);
        assert!(m.board_poll_irq());
        let _ = m.peek(0xFF02, &ram);
        let _ = m.peek(0xFF03, &ram);
        assert!(m.board_poll_irq());
        let _ = m.read(0xFF02, &ram);
        assert!(!m.board_poll_irq());
    }

    #[test]
    fn vdg_sees_pia1_port_b_outputs_only() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        // After RESET port B is all inputs: the VDG sees 0 (text mode).
        assert_eq!(m.vdg_inputs().vdg_ctrl, 0x00);
        m.write(0xFF23, 0x04, &mut ram);
        m.write(0xFF22, 0xF8, &mut ram); // latch only, still inputs
        assert_eq!(m.vdg_inputs().vdg_ctrl, 0x00);
        m.write(0xFF23, 0x00, &mut ram);
        m.write(0xFF22, 0xF8, &mut ram); // DDRB
        assert_eq!(m.vdg_inputs().vdg_ctrl, 0xF8);
        m.write(0xFFC9, 0, &mut ram);
        m.write(0xFFC1, 0, &mut ram);
        let v = m.vdg_inputs();
        assert_eq!((v.sam_v, v.sam_f), (1, 2));
    }

    #[test]
    fn io_registers_show_live_pia_and_sam_state() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFF21, 0x3C, &mut ram);
        m.write(0xFFDF, 0, &mut ram);
        m.write(0xFFD7, 0, &mut ram);
        let regs = m.io_registers();
        let find = |a: u16| regs.iter().find(|r| r.address == a).expect("reg").value;
        assert_eq!(find(0xFF21), 0x3C);
        assert_eq!(find(0xFFDE), 1);
        assert_eq!(find(0xFFD6), 1);
        assert!(regs.iter().any(|r| r.address == 0xFF22));
    }

    #[test]
    fn basic_boots_selects_64k_mode_and_finds_ram_top() {
        let emu = boot_basic::<M>();
        let screen = screen_text::<M>(&emu);
        assert!(screen.contains("EXTENDED COLOR BASIC"), "{screen}");
        let sam = Sam::from_bits(board::<M>(&emu).sam_bits());
        assert_eq!(sam.memory_size_bits(), 2, "BASIC 1.2 selects the 64K RAM mode");
        assert_eq!(sam.f_bits(), 2, "text screen at $0400");
        // Top of RAM found by the memory test: $7FFE (ROM starts at $8000).
        assert_eq!(emu.memory.read16(0x0074), 0x7FFE);
    }

    #[test]
    fn basic_timer_counts_60_per_second() {
        let mut emu = boot_basic::<M>();
        let t0 = emu.memory.read16(0x0112);
        run_cycles(&mut emu, 894_886);
        let dt = emu.memory.read16(0x0112).wrapping_sub(t0);
        assert!((59..=61).contains(&dt), "TIMER advanced by {dt}");
    }

    #[test]
    fn typing_shift_2_gives_double_quote() {
        let mut emu = boot_basic::<M>();
        // Positional: host Shift + the "2" key, no typed character supplied.
        board_mut::<M>(&mut emu).key("ShiftLeft", None, true);
        run_cycles(&mut emu, FIELD);
        tap::<M>(&mut emu, "Digit2", None);
        board_mut::<M>(&mut emu).key("ShiftLeft", None, false);
        run_cycles(&mut emu, 4 * FIELD);
        // Character mapping: key '"' (whatever the host layout).
        tap::<M>(&mut emu, "Digit2", Some("\""));
        let line = screen_line_with::<M>(&emu, "\"\"");
        assert!(line.starts_with("\"\""), "screen line {line:?}");
    }

    #[test]
    fn typing_all_basic_characters_through_key_mapping() {
        let mut emu = boot_basic::<M>();
        let text = "AZ09!\"#$%&'()*+,-./:;<=>?@^";
        type_text::<M>(&mut emu, text);
        let line = screen_line_with::<M>(&emu, "AZ09");
        assert!(line.starts_with(text), "screen line {line:?}");
    }

    #[test]
    fn typing_shifted_arrow_characters() {
        let mut emu = boot_basic::<M>();
        type_text::<M>(&mut emu, "Q[]\\_");
        let screen = screen_text::<M>(&emu);
        let line = screen_line_with::<M>(&emu, "Q");
        assert!(line.starts_with("Q[]\\_"), "screen line {line:?}\n{screen}");
    }

    #[test]
    fn german_layout_characters() {
        let mut emu = boot_basic::<M>();
        // QWERTZ: Shift+0 = '=', Shift+7 = '/', Shift+ß = '?', AltGr+Q = '@'.
        board_mut::<M>(&mut emu).key("ShiftLeft", Some("Shift"), true);
        tap::<M>(&mut emu, "Digit0", Some("="));
        tap::<M>(&mut emu, "Digit7", Some("/"));
        tap::<M>(&mut emu, "Minus", Some("?"));
        board_mut::<M>(&mut emu).key("ShiftLeft", Some("Shift"), false);
        tap::<M>(&mut emu, "KeyQ", Some("@"));
        tap::<M>(&mut emu, "KeyZ", Some("y")); // the QWERTZ Z key types 'y'
        tap::<M>(&mut emu, "BracketRight", Some("+"));
        let line = screen_line_with::<M>(&emu, "=/?");
        assert!(line.starts_with("=/?@Y+"), "screen line {line:?}");
    }

    #[test]
    fn short_taps_are_not_lost_and_double_letters_repeat() {
        let mut emu = boot_basic::<M>();
        // Down and up with no emulated time in between (one host event batch).
        for ch in ["L", "L", "I", "S", "T"] {
            let code = format!("Key{ch}");
            let m = board_mut::<M>(&mut emu);
            m.key(&code, Some(ch), true);
            m.key(&code, Some(ch), false);
        }
        run_cycles(&mut emu, 30 * FIELD);
        let line = screen_line_with::<M>(&emu, "LLIST");
        assert!(line.starts_with("LLIST"), "screen line {line:?}");
    }

    #[test]
    fn lowercase_host_letters_type_uppercase_and_run() {
        let mut emu = boot_basic::<M>();
        type_text::<M>(&mut emu, "print 1+1");
        tap::<M>(&mut emu, "Enter", Some("Enter"));
        run_cycles(&mut emu, 20 * FIELD);
        let screen = screen_text::<M>(&emu);
        assert!(screen.contains("PRINT 1+1"), "{screen}");
        assert!(screen.lines().any(|l| l.trim() == "2"), "{screen}");
    }

    #[test]
    fn typing_through_the_machine_container() {
        // Integration: apply_machine + machine_host_key_event (public API).
        let mut emu = Emulator::new();
        apply_machine(&mut emu, MachineKind::Coco2);
        let screen = |emu: &Emulator| {
            let f = machine_container(emu).and_then(|c| c.vdg_inputs()).expect("vdg").sam_f;
            text_at(&emu.memory.ram, f as usize * 512)
        };
        for _ in 0..60 {
            run_cycles(&mut emu, 50_000);
            if screen(&emu).contains("OK") {
                break;
            }
        }
        run_cycles(&mut emu, 50_000);
        machine_host_key_event(&mut emu, "Digit8", Some("("), true);
        machine_host_key_event(&mut emu, "Digit8", Some("("), false);
        run_cycles(&mut emu, 4 * FIELD);
        let text = screen(&emu);
        assert!(text.lines().any(|l| l.starts_with('(')), "{text}");
    }

    #[test]
    fn snapshot_roundtrip_keeps_board_state() {
        let mut m = Coco2Machine::new();
        let mut ram = [0u8; 0x10000];
        m.write(0xFFDF, 0, &mut ram);
        m.write(0xFF21, 0x3C, &mut ram);
        let snap = m.snapshot();
        let mut other = Coco2Machine::new();
        other.restore(&snap);
        assert!(other.inner.borrow().sam.map_type_all_ram());
        assert_eq!(other.inner.borrow().pia1.peek(1), 0x3C);
        // Sessions saved before the board rewrite still load.
        let row = serde_json::json!([false, false, false, false, false, false, false, false]);
        let old = serde_json::json!({ "inner": {
            "pia0": { "ddra": 0, "ddrb": 255, "ora": 0, "orb": 0, "ira": 255, "irb": 255,
                      "cra": 52, "crb": 53, "ca1": true, "cb1": true },
            "pia1": { "ddra": 254, "ddrb": 248, "ora": 2, "orb": 0, "ira": 255, "irb": 255,
                      "cra": 52, "crb": 55, "ca1": false, "cb1": false },
            "sam": { "bits": 16, "base": 65472 },
            "keyboard": { "keys": [row, row, row, row, row, row, row] },
            "cycle_acc": 100, "irq_pending": false
        }});
        let mut restored = Coco2Machine::new();
        restored.restore(&old);
        assert_eq!(restored.inner.borrow().sam.f_bits(), 2);
        assert_eq!(restored.inner.borrow().pia1.ddrb, 248);
    }
}
