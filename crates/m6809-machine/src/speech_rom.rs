//! Speech chipset mask-ROM images for the SP0256-AL2 / CTS256A-AL2 port.
//!
//! ROM binaries live in `crates/m6809-machine/roms/` and are embedded at
//! compile time (fetched by `scripts/fetch-roms.ps1`). They are the internal
//! mask ROMs of General Instrument / Microchip parts and remain the IP of
//! Microchip; redistribute only where you have the right to do so, exactly
//! like the Microsoft/Tandy BASIC images in [`crate::basic_rom`].
//!
//! - `sp0256-al2.bin` (2 KiB): SP0256-AL2 "Narrator" allophone ROM. The public
//!   dump from spatula-city stores each byte with its bits reversed relative to
//!   the order the microsequencer reads them, so [`crate::sp0256::Sp0256`]
//!   bit-reverses every byte at load time.
//! - `cts256a.bin` (4 KiB): CTS256A-AL2 code-to-speech ROM (TMS7000 program),
//!   mapped at `$F000` inside the on-chip address space.

/// SP0256-AL2 allophone mask ROM (2 KiB, bit-reversed on load).
pub const SP0256_AL2: &[u8] = include_bytes!("../roms/sp0256-al2.bin");

/// CTS256A-AL2 code-to-speech mask ROM (4 KiB, maps at `$F000`).
pub const CTS256A: &[u8] = include_bytes!("../roms/cts256a.bin");
