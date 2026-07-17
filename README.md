# 6809 Emulator

MC6809 / HD6309 CPU debugger with Rust core, Tauri desktop shell, and Svelte UI.

## Features

- Full MC6809 instruction set emulation
- HD6309 extensions: W/V/MD registers, LEA*, MULD/DIVD/DIVQ, TFM, bit ops, inter-register math
- CPU variant switch (MC6809 ↔ HD6309) with variant-aware disassembly
- Machine profiles: Bare Metal, TRS-80 CoCo 2, Dragon 32
- **Microsoft BASIC firmware** (embedded):
  - CoCo 2: Extended Color BASIC 1.1 (`$8000`) + Color BASIC 1.2 (`$A000`)
  - Dragon 32: Microsoft BASIC (`$8000`)
- PIA/SAM/VDG, VSYNC IRQ, host keyboard → matrix
- Register viewer with editable values and condition flags
- Disassembler synchronized to PC
- Motorola-syntax assembler with HD6309 mnemonics
- Memory hex viewer with inline editing and watchpoints
- Breakpoints, execution trace, session save/load
- Binary import/export
- Bilingual UI (DE / EN)

## Microsoft BASIC quick start

1. Start the app (`npm run tauri:dev`)
2. In **Setup**, choose **TRS-80 CoCo 2** or **Dragon 32**
3. Open the **VDG Text** panel and click the screen (keyboard capture)
4. Press **Run** — cold start should show the BASIC banner and `OK`
5. Type BASIC (e.g. `PRINT "HI"` then Enter)

ROMs live under `crates/m6809-machine/roms/` and are embedded at compile time.
They are copyrighted by Microsoft / Tandy / Dragon Data; redistribute only if
you have the right to do so. Refresh copies with `scripts/fetch-roms.ps1`.

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
crates/m6809-machine — CoCo 2 / Dragon 32 machine profiles
src-tauri/           — Tauri backend and commands
src/                 — Svelte frontend
```

## HD6309 Quick Start

1. Select **HD6309** in the CPU dropdown
2. Load the **hd6309** assembler example
3. Assemble and step — `LEAX 5,X`, `MULD`, `TFM+`, `DIVD` should work

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

## License

MIT