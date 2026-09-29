# 6809 Emulator

MC6809 / HD6309 CPU debugger with Rust core, Tauri desktop shell, and Svelte UI.

## Features

- Full MC6809 instruction set incl. the undocumented opcodes, datasheet cycle
  counts (indexed-mode extras, 3-cycle short branches, 10-cycle fast FIRQ …)
- Level-sensitive `/IRQ` and `/FIRQ` lines, edge-triggered NMI (armed by the
  first write to S), SYNC/CWAI waits during which emulated time keeps running
- HD6309 extensions: W/V/MD registers, native mode with its own timing,
  MULD/DIVD/DIVQ, TFM (interruptible), bit ops, inter-register math,
  OIM/AIM/EIM/TIM, illegal-instruction and divide-by-zero traps
- CPU variant switch (MC6809 ↔ HD6309) with variant-aware disassembly
- Machine profiles: Bare Metal, TRS-80 CoCo 2, Dragon 32, Microsoft BASIC (ACIA)
- **Microsoft BASIC firmware** (embedded):
  - CoCo 2: Extended Color BASIC 1.1 (`$8000`) + Color BASIC 1.2 (`$A000`)
  - Dragon 32: Microsoft BASIC (`$8000`)
  - Microsoft BASIC (ACIA): Grant Searle ExBasROM (`$C000`), console via 6850 at `$A000`
- CoCo 2 / Dragon 32 hardware: two MC6821 PIAs (all C1/C2 modes), MC6883 SAM
  (video address counter, CPU rate, map type, page bit), MC6847 VDG with all
  alphanumeric, semigraphics (SG4/6/8/12/24) and graphics modes (CG1 … RG6),
  60 Hz NTSC / 50 Hz PAL line and field timing, keyboard matrices, joysticks,
  6-bit DAC and single-bit sound, cassette (CLOAD/CSAVE via `.CAS`), printer,
  ROM cartridges
- MC6850 ACIA (divide/word select, overrun, TDR + shift register) and
  AY-3-8910 PSG (all envelope shapes, measured volume curve)
- Register viewer with editable values and condition flags
- Disassembler synchronized to PC
- Motorola-syntax assembler with HD6309 mnemonics
- Memory hex viewer with inline editing and watchpoints (debugger reads never
  disturb I/O devices)
- Breakpoints, execution trace, session save/load
- Binary import/export
- Bilingual UI (DE / EN)
- In-app updates from GitHub Releases
- Optional SP0256-AL2 speech + CTS256A-AL2 text-to-speech (MMIO `$FF50`, Speak UI)

## Microsoft BASIC quick start

**CoCo 2 / Dragon (video + keyboard)**

1. Start the app (`npm run tauri:dev`)
2. In **Setup**, choose **TRS-80 CoCo 2** or **Dragon 32**
3. Open the **VDG Screen** panel and click the screen (keyboard capture)
4. Press **Run** — cold start should show the BASIC banner and `OK`
5. Type BASIC (e.g. `PRINT "HI"` then Enter)

Typed characters are mapped to the emulated keyboard independent of the host
layout (a German keyboard types `"`, `=`, `*` … correctly). Special keys:
Backspace/← = LEFT ARROW, Esc/Home = CLEAR, End/Pause/F1 = BREAK. The mouse
can act as the right joystick (toggle in the video panel). The **Peripherals**
panel holds the cassette deck (`.CAS` files for CLOAD/CLOADM, CSAVE
recordings), the printer output (LLIST / `PRINT #-2`), ROM cartridges
(autostart via CART*) and joystick sliders. `SOUND`/`PLAY` are audible.

**Serial console (ACIA terminal)**

1. In **Setup**, choose **Microsoft BASIC (ACIA)**
2. Open the **Serial Terminal** panel (opens automatically)
3. Press **Run** — banner and `OK` appear in the terminal
4. Type a line (e.g. `PRINT 1+1`) and press Enter

This profile uses Grant Searle's port of Microsoft Extended Color BASIC with
the board's memory map: 32K RAM at `$0000`, `$8000-$9FFF` unmapped, the 6850
minimally decoded across `$A000-$BFFF` (status/control at even, data at odd
addresses), 16K ROM at `$C000`. The CPU runs at the board's 1.8432 MHz and the
ACIA at 115200 baud (÷16). Keywords are **uppercase**.

ROMs live under `crates/m6809-machine/roms/` and are embedded at compile time.
They are copyrighted by Microsoft / Tandy / Dragon Data; redistribute only if
you have the right to do so. Refresh copies with `scripts/fetch-roms.ps1`.

## Timing

Each machine runs at its own E clock: CoCo 2 894,886 Hz (14.31818 MHz / 16,
262 lines per field), Dragon 32 888,625 Hz (14.218 MHz / 16, 312 lines),
Microsoft BASIC SBC 1,843,200 Hz. The SAM speed pokes (`POKE 65495,0` /
`POKE 65497,0`) change the CPU rate while video, sound and tape stay in real
time. The run loop follows wall-clock time, so emulation speed and audio stay
in sync. Timing is instruction-granular: every instruction, interrupt entry
or wait quantum advances the devices by its exact cycle count.

## Speech (SP0256-AL2 / CTS256A-AL2)

Optional I/O block at `$FF50` (Setup → Speech), same style as the AY-3-8910:

| Addr | Write | Read |
| --- | --- | --- |
| `$FF50` | 6-bit allophone (ALD strobe; ignored while LRQ is busy) | status (same as `$FF51`) |
| `$FF51` | — | bit0 LRQ ready, bit1 SBY (standby) |
| `$FF52` | ASCII byte to the CTS256A-AL2 parallel port | last allophone sent by the CTS256 |
| `$FF53` | bit0 = 1: reset CTS256 + SP0256 (aborts speech, CTS reboots) | bit0 CTS busy, bit1 input FIFO full |

**SP0256-AL2** — LPC speech synthesizer (clean-room port of Joe Zbiciak's
model; every allophone renders bit-identically to MAME), ~10 kHz from a
3.12 MHz crystal, resampled to 44.1 kHz. Poll LRQ (`$FF51` bit 0) before
each allophone write to `$FF50`.

**CTS256A-AL2** — the real code-to-speech chip: its 4 KiB mask ROM runs on an
emulated TMS7041 (PIC7041) at 2.5 MHz (10 MHz crystal ÷ 4), wired like
schematic 2 of GI application note AN-0505D: parallel input, 2 KiB external
RAM (1792-byte text buffer + 256-byte allophone buffer), carriage-return-only
delimiter mode. The ROM's letter-to-sound rules turn English text into
allophones and feed the SP0256 over the LRQ → INT1 / ALD handshake. After
power-on or a reset it says “O.K.” (`OW PA1 PA3 KK1 EY PA3`), like the chip.

To speak from a program, write ASCII to `$FF52` and end each phrase with a
carriage return (`$0D`): the ROM speaks only after the CR. Lowercase,
digits (`123` is read digit by digit), `$` and punctuation are handled by the
ROM; ESC (`$1B`) discards pending text, backspace erases the last character.
Bytes wait in a 4 KiB FIFO and are strobed into the chip's input latch at the
pace the ROM accepts them (at least 450 µs apart, as the datasheet requires),
so a tight `STA $FF52` loop and slow writes produce identical speech.
`$FF53` bit 0 stays set while the chip boots and while text or allophones are
in flight; bit 1 means the FIFO is full and further bytes are dropped. As on
the real chip, a phrase longer than ~1.5 KB without a CR fills the buffer
and stalls until a reset.

Load the **SP0256 Speech** example, enable Speech, then **Run**. The Speech
panel shows LRQ/SBY, the CTS state with pending input/output and the recently
spoken allophones, and can send text through the CTS256 without the 6809
(`Speak` appends the CR).

Mask ROMs (`sp0256-al2.bin`, `cts256a.bin`) are General Instrument / Microchip
IP; fetch them with `scripts/fetch-roms.ps1` and redistribute only where you
have the right to do so. `al2.bin` is bit-reversed on load (public dump order).

## Prerequisites

- Rust (stable)
- Node.js 18+
- Windows: WebView2 (pre-installed on Windows 10+), MSVC Build Tools

## Development

```bash
npm install
npm run tauri:dev
```

Stop any running instance with `Ctrl+C` before restarting.

## Build

```bash
npm run build
cargo tauri build
```

## Project Structure

```
crates/m6809-core    — CPU, memory, execution engine
crates/m6809-asm     — Assembler and disassembler
crates/m6809-machine — CoCo 2 / Dragon 32 / MsBasic machines and chips
                       (PIA, SAM, VDG, ACIA, AY-3-8910, SP0256, CTS256A/TMS7000)
src-tauri/           — Tauri backend and commands
src/                 — Svelte frontend
```

## HD6309 Quick Start

1. Select **HD6309** in the CPU dropdown. Like the real chip it starts in 6809
   emulation mode; `LDMD #$01` switches to native mode (faster timing,
   interrupts also stack W).
2. Load the **hd6309** assembler example.
3. Assemble and step through `TFM X+,Y+`, `MULD`, `DIVD`, `DIVQ`, `ADDR W,D`,
   `AIM`/`OIM`/`EIM` and `LDBT`/`STBT`; the results land at `$0800-$0811`
   (listed in the example header).

HD6309 syntax in the assembler:

- `AIM #$F0,<$20` / `OIM #$0F,$1234` / `TIM #$80,5,X` — immediate first, then
  the address (direct, extended or indexed)
- `ADDR src,dst` (also `ADCR SUBR SBCR ANDR ORR EORR CMPR`, `TFR`, `EXG`):
  `dst = dst OP src`; registers `D X Y U S PC W V A B CC DP 0 E F`. As on the
  chip, the destination decides the size: an 8-bit destination takes the low
  byte of a 16-bit source (`ADDR X,A`), a 16-bit one promotes A/B to D, E/F
  to W, CC to `$00:CC` and DP to `DP:$00`
- `DIVD #n` divides D by a signed byte (B = quotient, A = remainder), `DIVQ`
  divides Q = D:W (W = quotient, D = remainder), `MULD` puts D × operand in Q
- `BAND reg,srcbit,dstbit,<addr` (also `BIAND BOR BIOR BEOR BIEOR LDBT`):
  memory bit → bit of `CC`/`A`/`B`; `STBT` copies the register bit to memory
- `TFM X+,Y+` / `X-,Y-` / `X+,Y` / `X,Y+` with `D X Y U S`, W = byte count;
  an interrupt can be taken after every byte, RTI resumes the transfer
- 6309 indexed modes: `,W` `n,W` `,W++` `,--W` `E,X` `F,X` `W,X` (and `[...]`)
- Illegal opcodes, illegal index postbytes (`[,-R]`) and division by zero
  trap through `$FFF0` and set MD bit 6/7; a handler tests and clears them
  with `BITMD #$40` / `BITMD #$80`. `NEGW`, `ASRW` and `ASLW` (MAME-only
  opcodes that trap on real chips) are rejected by the assembler.

Flags and cycle counts follow real HD6309 silicon (Atkinson's reference and
hoglet67's logic-analyser verified 6809Decoder) where it differs from MAME.

## Releases (GitHub Actions)

Automated builds run on every version tag (`v*`).

### One-command release (recommended)

```powershell
# bumps version files, commits, tags v0.2.0, pushes → Actions builds installers
pwsh ./scripts/release.ps1 -Version 0.2.0
```

```bash
./scripts/release.sh 0.2.0
```

### Manual / UI

1. **Actions → Release → Run workflow** and enter a version, or  
2. Create and push a tag:

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

Artifacts (Windows `.msi`/`.exe`, macOS `.dmg`, Linux `.AppImage`/`.deb`) appear on the
[Releases](https://github.com/antibyte/6809Emu/releases) page when the workflow finishes.

## In-App Updates

Installed builds check [GitHub Releases](https://github.com/antibyte/6809Emu/releases)
for updates (silent check ~2s after start, and via **Help → Check for updates…**).

When a newer version is available, a dialog offers install with progress; on confirm
the update is downloaded, signature-verified, installed, and the app restarts.

### Release signing (required for updates)

Tauri signs update artifacts. Without the private key, release builds that produce
updater packages will fail.

1. Generate keys once (re-generate only if the key is lost):

```powershell
# empty password is OK for CI; a password is recommended for local keys
$env:CI = "true"
npx tauri signer generate -w $env:USERPROFILE\.tauri\6809emu.key -f --ci
```

2. Put the **public** key contents into `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
   (already configured for this project).

3. Add GitHub repository secrets (Settings → Secrets and variables → Actions):

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Full contents of `6809emu.key` (or the private key string printed by the CLI) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password if the key has one; omit or leave empty otherwise |

4. Ship a release as usual (`scripts/release.ps1`). The workflow uploads installers
   plus `latest.json` and `.sig` files used by the in-app updater.

**Important:** If you lose the private key, existing installs cannot verify new updates
until users reinstall from a full installer signed with a new key pair (and you update
`pubkey` in the app).

### Local signed build (optional)

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\6809emu.key"
# $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "..."  # if set
npm run tauri build
```

## License

MIT
