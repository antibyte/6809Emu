//! MC6809 functional test ROM (W. Schwotzer / flexemu cputest, binary from gtoal 6809sbt).
//!
//! The ROM expects FLEX monitor routines at $CD03–$CD39. This harness emulates
//! those stubs and checks for the success string at $93A3.

use m6809_core::Emulator;

const ROM: &[u8] = include_bytes!("../test-data/cputest.bin");

const LOAD_ADDR: u16 = 0x8100;
const STACK: u16 = 0x4000;

const WARMS: u16 = 0xCD03;
const PUTCHR: u16 = 0xCD18;
const PSTRNG: u16 = 0xCD1E;
const PCRLF: u16 = 0xCD24;
const OUTDEC: u16 = 0xCD39;

const SUCCESS_STR: u16 = 0x93A3;
const ERROR_STR: u16 = 0x9395;
const EOT: u8 = 4;

const MAX_CYCLES: u64 = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Running,
    Passed,
    Failed(String),
}

fn emulate_rts(emu: &mut Emulator) {
    emu.cpu.pc = emu.cpu.pull16(&emu.memory);
}

fn read_fcc_string(mem: &m6809_core::Memory, mut addr: u16) -> String {
    let mut out = String::new();
    loop {
        let ch = mem.read8(addr);
        addr = addr.wrapping_add(1);
        if ch == EOT {
            break;
        }
        if ch.is_ascii_graphic() || ch == b' ' {
            out.push(ch as char);
        } else {
            out.push_str(&format!("<{ch:02X}>"));
        }
    }
    out
}

fn emulate_pstring(emu: &mut Emulator) -> Outcome {
    let start = emu.cpu.x;
    let text = read_fcc_string(&emu.memory, start);

    let outcome = if start == SUCCESS_STR {
        Outcome::Passed
    } else if start == ERROR_STR {
        let test_name = read_fcc_string(&emu.memory, emu.cpu.u);
        Outcome::Failed(format!("functional test failed: {text}{test_name}"))
    } else {
        // Progress / per-test label (FCC "MUL", etc.) — keep running.
        Outcome::Running
    };

    emulate_rts(emu);
    outcome
}

fn run_functional_test() -> Result<(), String> {
    let mut emu = Emulator::new();
    emu.load_program(LOAD_ADDR, ROM)?;
    emu.cpu.pc = LOAD_ADDR;
    emu.cpu.s = STACK;
    emu.cpu.a = 0;
    emu.cpu.b = 0;
    emu.cpu.x = 0;
    emu.cpu.y = 0;
    emu.cpu.u = 0;
    emu.cpu.dp = 0;
    emu.cpu.cc = m6809_core::Flags::empty();
    emu.cpu.halted = false;

    let start_cycles = emu.cpu.total_cycles;
    let mut outcome = Outcome::Running;

    while emu.cpu.total_cycles - start_cycles < MAX_CYCLES {
        match emu.cpu.pc {
            WARMS => {
                return match outcome {
                    Outcome::Passed => Ok(()),
                    Outcome::Failed(msg) => Err(msg),
                    Outcome::Running => Err(format!(
                        "WARMS reached without success marker (pc=${:04X})",
                        emu.cpu.pc
                    )),
                };
            }
            PSTRNG => {
                outcome = emulate_pstring(&mut emu);
                if let Outcome::Failed(ref msg) = outcome {
                    return Err(msg.clone());
                }
            }
            PUTCHR | PCRLF | OUTDEC => emulate_rts(&mut emu),
            _ => {
                emu.step();
                if emu.cpu.halted {
                    return Err(format!("CPU halted at ${:04X}", emu.cpu.pc));
                }
            }
        }
    }

    Err(format!(
        "functional test exceeded {MAX_CYCLES} cycles (pc=${:04X})",
        emu.cpu.pc
    ))
}

#[test]
fn schwotzer_cputest_rom_passes() {
    run_functional_test().expect("MC6809 functional test ROM should pass");
}