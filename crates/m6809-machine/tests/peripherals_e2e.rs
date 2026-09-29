//! End-to-end tests of the CoCo 2 / Dragon 32 peripherals running the real
//! BASIC ROMs: JOYSTK, SOUND/PLAY, LLIST, CLOAD/CLOADM/CSAVE, cartridges.

use m6809_core::Emulator;
use m6809_machine::*;

fn run_cycles(emu: &mut Emulator, n: u64) {
    let start = emu.cpu.total_cycles;
    while emu.cpu.total_cycles - start < n {
        emu.step();
    }
}

fn screen(emu: &Emulator) -> String {
    machine_video_frame(emu).expect("frame").rows_text.join("\n")
}

fn boot(kind: MachineKind) -> Emulator {
    let mut emu = Emulator::new();
    apply_machine(&mut emu, kind);
    for _ in 0..40 {
        run_cycles(&mut emu, 100_000);
        if screen(&emu).contains("OK") {
            break;
        }
    }
    assert!(screen(&emu).contains("OK"), "booted: {}", screen(&emu));
    emu
}

fn codes_for(c: char) -> Vec<&'static str> {
    const LETTERS: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK",
        "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV",
        "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const DIGITS: [&str; 10] = [
        "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8",
        "Digit9",
    ];
    match c {
        'A'..='Z' => vec![LETTERS[(c as u8 - b'A') as usize]],
        '0'..='9' => vec![DIGITS[(c as u8 - b'0') as usize]],
        ' ' => vec!["Space"],
        '\r' | '\n' => vec!["Enter"],
        ':' => vec!["Quote"],
        ';' => vec!["Semicolon"],
        ',' => vec!["Comma"],
        '-' => vec!["Minus"],
        '.' => vec!["Period"],
        '/' => vec!["Slash"],
        '"' => vec!["ShiftLeft", "Digit2"],
        '(' => vec!["ShiftLeft", "Digit8"],
        ')' => vec!["ShiftLeft", "Digit9"],
        '$' => vec!["ShiftLeft", "Digit4"],
        '=' => vec!["ShiftLeft", "Minus"],
        '+' => vec!["ShiftLeft", "Semicolon"],
        '*' => vec!["ShiftLeft", "Quote"],
        '<' => vec!["ShiftLeft", "Comma"],
        '>' => vec!["ShiftLeft", "Period"],
        '?' => vec!["ShiftLeft", "Slash"],
        '!' => vec!["ShiftLeft", "Digit1"],
        '#' => vec!["ShiftLeft", "Digit3"],
        '%' => vec!["ShiftLeft", "Digit5"],
        '&' => vec!["ShiftLeft", "Digit6"],
        _ => panic!("no key for {c:?}"),
    }
}

fn type_text(emu: &mut Emulator, text: &str) {
    for c in text.chars() {
        let codes = codes_for(c);
        for code in &codes {
            machine_host_key(emu, code, true);
        }
        run_cycles(emu, 40_000);
        for code in codes.iter().rev() {
            machine_host_key(emu, code, false);
        }
        run_cycles(emu, 40_000);
    }
}

fn wait_for(emu: &mut Emulator, needle: &str, max_cycles: u64) -> bool {
    let start = emu.cpu.total_cycles;
    while emu.cpu.total_cycles - start < max_cycles {
        run_cycles(emu, 50_000);
        if screen(emu).contains(needle) {
            return true;
        }
    }
    false
}

fn is_cursor_only(line: &str) -> bool {
    !line.chars().any(|c| c.is_ascii_graphic())
}

/// BASIC is back at the prompt: the last text line is "OK".
fn prompt_ready(emu: &Emulator) -> bool {
    let s = screen(emu);
    let lines: Vec<&str> = s
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !is_cursor_only(l))
        .collect();
    lines.last().is_some_and(|l| *l == "OK")
}

fn run_command(emu: &mut Emulator, line: &str, max_cycles: u64) {
    type_text(emu, line);
    type_text(emu, "\r");
    let start = emu.cpu.total_cycles;
    while emu.cpu.total_cycles - start < max_cycles {
        run_cycles(emu, 50_000);
        if prompt_ready(emu) {
            break;
        }
    }
}

fn tokenized_program(lines: &[(u16, &[u8])], base: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut addr = base;
    for (num, body) in lines {
        let len = 4 + body.len() + 1;
        let next = addr + len as u16;
        out.extend_from_slice(&next.to_be_bytes());
        out.extend_from_slice(&num.to_be_bytes());
        out.extend_from_slice(body);
        out.push(0);
        addr = next;
    }
    out.extend_from_slice(&[0, 0]);
    out
}

fn block(block_type: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x3C, block_type, data.len() as u8];
    out.extend_from_slice(data);
    let sum = data
        .iter()
        .fold(block_type.wrapping_add(data.len() as u8), |s, &b| s.wrapping_add(b));
    out.push(sum);
    out
}

fn cas_file(name: &str, program: &[u8]) -> Vec<u8> {
    let mut header = [b' '; 8].to_vec();
    for (i, b) in name.bytes().take(8).enumerate() {
        header[i] = b;
    }
    header.extend_from_slice(&[0x00, 0x00, 0x00, 0, 0, 0, 0]);
    let mut tape = vec![0x55; 128];
    tape.extend(block(0x00, &header));
    tape.push(0x55);
    tape.extend(vec![0x55; 128]);
    for chunk in program.chunks(255) {
        tape.push(0x55);
        tape.extend(block(0x01, chunk));
        tape.push(0x55);
    }
    tape.push(0x55);
    tape.extend(block(0xFF, &[]));
    tape.push(0x55);
    tape
}

#[test]
fn coco_joystk_reads_set_values() {
    let mut emu = boot(MachineKind::Coco2);
    machine_set_joystick(&emu, 0, 12, 50, false);
    machine_set_joystick(&emu, 1, 63, 0, false);
    run_command(&mut emu, "PRINT JOYSTK(0);JOYSTK(1);JOYSTK(2);JOYSTK(3)", 3_000_000);
    let s = screen(&emu);
    assert!(s.contains(" 12  50  63  0"), "screen:\n{s}");
}

#[test]
fn dragon_joystk_reads_set_values() {
    let mut emu = boot(MachineKind::Dragon32);
    machine_set_joystick(&emu, 0, 7, 33, false);
    machine_set_joystick(&emu, 1, 40, 61, false);
    run_command(&mut emu, "PRINT JOYSTK(0);JOYSTK(1);JOYSTK(2);JOYSTK(3)", 3_000_000);
    let s = screen(&emu);
    assert!(s.contains(" 7  33  40  61"), "screen:\n{s}");
}

#[test]
fn coco_fire_button_reads_through_peek() {
    let mut emu = boot(MachineKind::Coco2);
    // Wait for the right fire button (PIA0 PA0 low) in a loop.
    run_command(&mut emu, "10 A=PEEK(65280) AND 3:IF A=3 THEN 10", 500_000);
    run_command(&mut emu, "20 PRINT \"BUTTON\";A", 500_000);
    type_text(&mut emu, "RUN\r");
    run_cycles(&mut emu, 300_000);
    machine_set_joystick(&emu, 0, 32, 32, true);
    run_cycles(&mut emu, 100_000);
    machine_set_joystick(&emu, 0, 32, 32, false);
    assert!(wait_for(&mut emu, "BUTTON 2", 2_000_000), "{}", screen(&emu));
}

fn dominant_hz(samples: &[f32]) -> f64 {
    let mut crossings = 0usize;
    for pair in samples.windows(2) {
        if (pair[0] < 0.0) != (pair[1] < 0.0) {
            crossings += 1;
        }
    }
    crossings as f64 / 2.0 / (samples.len() as f64 / 44_100.0)
}

fn sound_test(kind: MachineKind) {
    let mut emu = boot(kind);
    let _ = board_drain_audio(&mut emu);
    let clock = emu.cpu_clock_hz().unwrap();
    let start = emu.cpu.total_cycles;
    let mut audio = Vec::new();
    type_text(&mut emu, "SOUND 200,30\r");
    audio.extend(board_drain_audio(&mut emu));
    for _ in 0..6 {
        run_cycles(&mut emu, 500_000);
        audio.extend(board_drain_audio(&mut emu));
    }
    let elapsed = emu.cpu.total_cycles - start;
    let expected = elapsed as f64 * 44_100.0 / f64::from(clock);
    assert!((audio.len() as f64 - expected).abs() < 3.0, "{} vs {expected}", audio.len());
    // Find the loud part.
    let loud: Vec<f32> = audio
        .chunks(441)
        .filter(|c| c.iter().fold(0.0f32, |m, &x| m.max(x.abs())) > 0.1)
        .flatten()
        .copied()
        .collect();
    let secs = loud.len() as f64 / 44_100.0;
    assert!(secs > 1.0, "SOUND 200,30 plays ~2 s, got {secs} s");
    let hz = dominant_hz(&loud);
    println!("{kind:?}: SOUND 200 -> {hz:.0} Hz for {secs:.2} s");
    assert!(hz > 500.0 && hz < 5000.0, "hz={hz}");
}

#[test]
fn coco_sound_is_audible() {
    sound_test(MachineKind::Coco2);
}

#[test]
fn dragon_sound_is_audible() {
    sound_test(MachineKind::Dragon32);
}

#[test]
fn coco_play_is_audible() {
    let mut emu = boot(MachineKind::Coco2);
    let _ = board_drain_audio(&mut emu);
    type_text(&mut emu, "PLAY \"CDEFG\"\r");
    run_cycles(&mut emu, 2_000_000);
    let audio = board_drain_audio(&mut emu);
    let peak = audio.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    assert!(peak > 0.1, "PLAY peak {peak}");
}

#[test]
fn coco_llist_prints() {
    let mut emu = boot(MachineKind::Coco2);
    run_command(&mut emu, "10 PRINT \"HELLO\"", 500_000);
    run_command(&mut emu, "20 GOTO 10", 500_000);
    let _ = printer_take_output(&emu);
    run_command(&mut emu, "LLIST", 6_000_000);
    let out = printer_take_output(&emu);
    assert_eq!(out, "10 PRINT \"HELLO\"\n20 GOTO 10\n", "screen:\n{}", screen(&emu));
    // PRINT #-2 too.
    run_command(&mut emu, "PRINT #-2,\"ABC\";123", 6_000_000);
    assert_eq!(printer_take_output(&emu), "ABC 123 \n");
}

#[test]
fn coco_llist_at_1200_baud() {
    let mut emu = boot(MachineKind::Coco2);
    run_command(&mut emu, "POKE 150,41", 500_000);
    run_command(&mut emu, "10 REM FAST", 500_000);
    let _ = printer_take_output(&emu);
    run_command(&mut emu, "LLIST", 6_000_000);
    assert_eq!(printer_take_output(&emu), "10 REM FAST\n");
}

#[test]
fn dragon_llist_prints() {
    let mut emu = boot(MachineKind::Dragon32);
    run_command(&mut emu, "10 PRINT \"HELLO\"", 500_000);
    let _ = printer_take_output(&emu);
    run_command(&mut emu, "LLIST", 6_000_000);
    let out = printer_take_output(&emu);
    assert_eq!(out, "10 PRINT \"HELLO\"\n", "screen:\n{}", screen(&emu));
    // SOUND toggles the strobe pin (PA1) on the Dragon: no garbage text.
    run_command(&mut emu, "SOUND 100,5", 3_000_000);
    let junk = printer_take_output(&emu);
    println!("dragon SOUND printer junk: {junk:?}");
    assert!(junk.trim().is_empty(), "junk {junk:?}");
}

fn cload_handmade(kind: MachineKind, base: u16) {
    let mut emu = boot(kind);
    // 10 PRINT "HELLO"  (PRINT token $87)
    let program = tokenized_program(&[(10, b"\x87 \"HELLO\"")], base);
    let tape = cas_file("HELLO", &program);
    let st = cassette_insert(&emu, "hello.cas".into(), tape.clone()).expect("board");
    assert!(st.loaded);
    type_text(&mut emu, "CLOAD\r");
    assert!(wait_for(&mut emu, "HELLO", 20_000_000), "found: {}", screen(&emu));
    let start = emu.cpu.total_cycles;
    while !prompt_ready(&emu) && emu.cpu.total_cycles - start < 40_000_000 {
        run_cycles(&mut emu, 100_000);
    }
    let st = cassette_state(&emu).unwrap();
    println!("{kind:?} after CLOAD: {st:?}\n{}", screen(&emu));
    assert!(!screen(&emu).contains("I/O ERROR"), "{}", screen(&emu));
    run_command(&mut emu, "LIST", 2_000_000);
    let s = screen(&emu);
    assert!(s.contains("10 PRINT \"HELLO\""), "screen:\n{s}");
    assert!(!st.motor, "relay open after load");
}

#[test]
fn coco_cload_handmade_cas() {
    cload_handmade(MachineKind::Coco2, 0x1E01);
}

#[test]
fn dragon_cload_handmade_cas() {
    cload_handmade(MachineKind::Dragon32, 0x1E01);
}

fn csave_cload_round_trip(kind: MachineKind) {
    let mut emu = boot(kind);
    run_command(&mut emu, "10 A=1234", 500_000);
    run_command(&mut emu, "20 PRINT A*2", 500_000);
    let _ = cassette_take_recording(&emu);
    run_command(&mut emu, "CSAVE \"RT\"", 30_000_000);
    let rec = cassette_take_recording(&emu);
    println!("{kind:?}: recorded {} bytes", rec.len());
    assert!(rec.len() > 256 + 30, "recorded {}", rec.len());
    run_command(&mut emu, "NEW", 500_000);
    cassette_insert(&emu, "rt.cas".into(), rec).unwrap();
    type_text(&mut emu, "CLOAD \"RT\"\r");
    let start = emu.cpu.total_cycles;
    run_cycles(&mut emu, 200_000);
    while !prompt_ready(&emu) && emu.cpu.total_cycles - start < 40_000_000 {
        run_cycles(&mut emu, 100_000);
    }
    assert!(!screen(&emu).contains("ERROR"), "{}", screen(&emu));
    run_command(&mut emu, "RUN", 2_000_000);
    let s = screen(&emu);
    assert!(s.contains(" 2468"), "screen:\n{s}");
}

#[test]
fn coco_csave_then_cload() {
    csave_cload_round_trip(MachineKind::Coco2);
}

#[test]
fn dragon_csave_then_cload() {
    csave_cload_round_trip(MachineKind::Dragon32);
}

fn cartridge_autostart(kind: MachineKind) {
    let mut emu = boot(kind);
    // $C000: LDA #$5A ; STA $3000 ; BRA *
    let mut rom = vec![0x86, 0x5A, 0xB7, 0x30, 0x00, 0x20, 0xFE];
    rom.resize(0x2000, 0xFF);
    emu.memory.write8(0x3000, 0);
    let st = cartridge_insert(&mut emu, "auto.rom".into(), rom.clone(), true).unwrap();
    assert!(st.loaded && st.autostart);
    run_cycles(&mut emu, 4_000_000);
    assert_eq!(emu.memory.read8(0x3000), 0x5A, "cartridge ran; pc={:04X}", emu.cpu.pc);
    assert!((0xC000..0xC010).contains(&emu.cpu.pc), "pc={:04X}", emu.cpu.pc);
    // Without autostart BASIC stays in charge (cold start keeps the ROM visible).
    let mut emu = boot(kind);
    emu.memory.write8(0x3000, 0);
    cartridge_insert(&mut emu, "plain.rom".into(), rom, false).unwrap();
    run_cycles(&mut emu, 4_000_000);
    assert_eq!(emu.memory.read8(0x3000), 0);
    assert_eq!(emu.memory.read8(0xC000), 0x86, "ROM visible at $C000");
    assert!(emu.cpu.pc < 0xC000 || emu.cpu.pc > 0xFEFF);
}

#[test]
fn coco_cartridge_autostart() {
    cartridge_autostart(MachineKind::Coco2);
}

#[test]
fn dragon_cartridge_autostart() {
    cartridge_autostart(MachineKind::Dragon32);
}

#[test]
fn basic_takes_no_spurious_firq() {
    for kind in [MachineKind::Coco2, MachineKind::Dragon32] {
        let mut emu = boot(kind);
        // Run a while and make sure BASIC still answers (no FIRQ storm).
        run_cycles(&mut emu, 2_000_000);
        run_command(&mut emu, "PRINT 6*7", 2_000_000);
        assert!(screen(&emu).contains(" 42"), "{kind:?}: {}", screen(&emu));
    }
}
fn ml_cas(name: &str, code: &[u8], load: u16, exec: u16) -> Vec<u8> {
    let mut header = [b' '; 8].to_vec();
    for (i, b) in name.bytes().take(8).enumerate() {
        header[i] = b;
    }
    header.extend_from_slice(&[0x02, 0x00, 0x00]);
    header.extend_from_slice(&exec.to_be_bytes());
    header.extend_from_slice(&load.to_be_bytes());
    // Short leaders, like many .CAS dumps.
    let mut tape = vec![0x55; 2];
    tape.extend(block(0x00, &header));
    tape.push(0x55);
    for chunk in code.chunks(255) {
        tape.push(0x55);
        tape.extend(block(0x01, chunk));
    }
    tape.push(0x55);
    tape.extend(block(0xFF, &[]));
    tape
}

fn cloadm_multi_block(kind: MachineKind) {
    let mut emu = boot(kind);
    // $3000: LDA #$A5 ; STA $3100 ; RTS ; then filler over three blocks.
    let mut code = vec![0x86, 0xA5, 0xB7, 0x31, 0x00, 0x39];
    code.extend((0..600u32).map(|i| (i * 7 + 3) as u8));
    cassette_insert(&emu, "ml.cas".into(), ml_cas("MLPROG", &code, 0x3000, 0x3000)).unwrap();
    type_text(&mut emu, "CLOADM\r");
    let start = emu.cpu.total_cycles;
    run_cycles(&mut emu, 200_000);
    while !prompt_ready(&emu) && emu.cpu.total_cycles - start < 40_000_000 {
        run_cycles(&mut emu, 100_000);
    }
    assert!(!screen(&emu).contains("ERROR"), "{}", screen(&emu));
    let loaded: Vec<u8> = (0..code.len()).map(|i| emu.memory.read8(0x3000 + i as u16)).collect();
    assert_eq!(loaded, code, "{kind:?}: all three blocks loaded");
    run_command(&mut emu, "EXEC", 1_000_000);
    assert_eq!(emu.memory.read8(0x3100), 0xA5);
}

#[test]
fn coco_cloadm_multi_block_short_leaders() {
    cloadm_multi_block(MachineKind::Coco2);
}

#[test]
fn dragon_cloadm_multi_block_short_leaders() {
    cloadm_multi_block(MachineKind::Dragon32);
}

#[test]
fn coco_audio_on_plays_the_tape() {
    let mut emu = boot(MachineKind::Coco2);
    let tape = cas_file("NOISE", &[0u8; 200]);
    cassette_insert(&emu, "n.cas".into(), tape).unwrap();
    run_command(&mut emu, "MOTOR ON:AUDIO ON", 1_000_000);
    let _ = board_drain_audio(&mut emu);
    run_cycles(&mut emu, 1_500_000);
    let audio = board_drain_audio(&mut emu);
    let loud: Vec<f32> = audio
        .chunks(2205)
        .filter(|w| w.iter().fold(0.0f32, |m, &x| m.max(x.abs())) > 0.01)
        .flatten()
        .copied()
        .collect();
    let hz = dominant_hz(&loud);
    let peak = loud.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    println!("tape monitor: {hz:.0} Hz peak {peak:.3} over {} samples", loud.len());
    assert!(loud.len() > 20_000, "tape audible for a while");
    assert!(peak > 0.02, "tape audible (peak {peak})");
    assert!(hz > 1200.0 && hz < 2400.0, "FSK tones, got {hz}");
    run_command(&mut emu, "AUDIO OFF:MOTOR OFF", 1_000_000);
    let st = cassette_state(&emu).unwrap();
    assert!(!st.motor && st.position > 0);
}
