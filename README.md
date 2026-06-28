# 6809 Emulator

MC6809 / HD6309 CPU debugger with Rust core, Tauri desktop shell, and Svelte UI.

## Features

- Full MC6809 instruction set emulation
- HD6309 extensions: W/V/MD registers, LEA*, MULD/DIVD/DIVQ, TFM, bit ops, inter-register math
- CPU variant switch (MC6809 ↔ HD6309) with variant-aware disassembly
- Machine profiles: Bare Metal, TRS-80 CoCo 2, Dragon 32 (memory map + I/O stubs)
- Register viewer with editable values and condition flags
- Disassembler synchronized to PC
- Motorola-syntax assembler with HD6309 mnemonics
- Memory hex viewer with inline editing and watchpoints
- Breakpoints, execution trace, session save/load
- Binary import/export
- Bilingual UI (DE / EN)

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

## License

MIT