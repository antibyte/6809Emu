use std::sync::atomic::Ordering;
use std::sync::Arc;

use m6809_asm::{assemble, disassemble_with_variant, DisassembledInsn};
use m6809_core::{CpuState, CpuVariant, EmulatorSnapshot, LoadConfig, StepResult};
use m6809_machine::{
    acia_send_input, apply_machine, ay_drain_audio, ay_set_port_input, board_drain_audio,
    cartridge_eject, cartridge_insert, cartridge_state, cassette_eject, cassette_insert,
    cassette_rewind, cassette_state, cassette_take_recording, clear_acia_terminal,
    printer_take_output, set_pia_control_line, CartridgeStateDto, CassetteStateDto,
    get_acia_config, get_acia_terminal, get_ay_config, get_ay_state, get_pia_config, get_pia_state,
    get_speech_config, get_speech_state, list_machines, machine_clear_keys, machine_host_key_event,
    machine_state, machine_video_frame, restore_machine_io, set_acia_config, set_ay_config,
    set_pia_config, set_pia_input, set_speech_config, set_speech_greeting, speech_drain_audio,
    speech_run_until_idle_collect, speech_say_text, AciaConfig, AciaTerminalDto, AyConfig, AyStateDto, MachineInfo, MachineKind,
    MachineStateDto, PiaConfig, PiaStateDto, SpeechConfig, SpeechStateDto, VideoFrameDto,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle, Emitter, State};

use crate::state::{AppState, RunSpeed, DEFAULT_E_CLOCK_HZ};
use std::time::{Duration, Instant};

/// Mix 44.1 kHz mono streams (AY, speech, board DAC) into one, clipping to ±1.
fn mix_audio(streams: Vec<Vec<f32>>) -> Vec<f32> {
    let mut streams: Vec<Vec<f32>> = streams.into_iter().filter(|s| !s.is_empty()).collect();
    if streams.len() <= 1 {
        return streams.pop().unwrap_or_default();
    }
    streams.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = streams.remove(0);
    for stream in &streams {
        for (dst, s) in out.iter_mut().zip(stream.iter()) {
            *dst += *s;
        }
    }
    for s in &mut out {
        *s = s.clamp(-1.0, 1.0);
    }
    out
}

/// Drain every audio source of the machine (AY, speech, board sound).
fn drain_machine_audio(emu: &mut m6809_core::Emulator) -> Vec<f32> {
    mix_audio(vec![
        ay_drain_audio(emu),
        speech_drain_audio(emu),
        board_drain_audio(emu),
    ])
}

fn encode_audio_f32_base64(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    B64.encode(bytes)
}

/// Release the emulator lock this often so UI/ACIA commands can interleave.
const RUN_LOCK_BATCH_STEPS: u32 = 2_048;
/// How far (in frames) emulation may fall behind wall time before the backlog
/// is dropped instead of being caught up.
const MAX_CATCHUP_FRAMES: f64 = 4.0;
/// MS BASIC needs ~400k steps to evaluate a line; the old 50k cap starved input.
const ACIA_CATCHUP_STEPS_CAP: u32 = 2_000_000;

fn clamp_acia_catchup_steps(steps: u32) -> u32 {
    steps.min(ACIA_CATCHUP_STEPS_CAP)
}

struct StepBatch {
    cycles_run: u64,
    steps: u32,
    last: Option<StepResult>,
    stop: bool,
}

fn step_batch(
    emu: &mut m6809_core::Emulator,
    mut cycles_run: u64,
    mut steps: u32,
    target_cycles: u64,
    max_steps: u32,
    batch_steps: u32,
) -> StepBatch {
    let mut last = None;
    let limit = steps.saturating_add(batch_steps.max(1)).min(max_steps);
    let mut stop = false;
    // SYNC/CWAI waits are not a stop condition: time keeps running so the
    // machine's devices can raise the interrupt the program waits for.
    while cycles_run < target_cycles && steps < limit {
        let result = emu.step();
        cycles_run += u64::from(result.cycles);
        steps += 1;
        let trapped = result.trap.is_some();
        last = Some(result);
        if trapped {
            stop = true;
            break;
        }
    }
    StepBatch {
        cycles_run,
        steps,
        last,
        stop,
    }
}

async fn yield_run_loop() {
    tokio::task::yield_now().await;
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryChunk {
    pub address: u16,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssembleResult {
    pub origin: u16,
    pub bytes: Vec<u8>,
    pub errors: Vec<AsmErrorDto>,
    /// Maps the 1-based source line number to the address of the code emitted
    /// by that line, so the UI can set breakpoints on source lines.
    pub line_map: std::collections::HashMap<u32, u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsmErrorDto {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisasmLine {
    pub address: u16,
    pub bytes: Vec<u8>,
    pub text: String,
}

#[tauri::command]
pub fn reset_emulator(state: State<'_, Arc<AppState>>) -> Result<CpuState, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let reset_pc = emu.memory.config.reset_pc;
    // Bare metal: the vector lives in RAM. On CoCo/Dragon the vectors are ROM
    // (writes ignored), so the configured entry point is also applied directly.
    emu.memory.write16(0xFFFE, reset_pc);
    emu.reset();
    emu.cpu.pc = reset_pc;
    state.clear_trace();
    Ok(emu.get_state())
}

#[tauri::command]
pub fn step(state: State<'_, Arc<AppState>>) -> Result<StepResult, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let result = emu.step();
    state.push_trace(result.clone());
    Ok(result)
}

#[tauri::command]
pub async fn run_emulator(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    // Audio produced while paused (single steps, ACIA catch-up) is stale now.
    if let Ok(mut emu) = state.emulator.lock() {
        let _ = drain_machine_audio(&mut emu);
    }

    let state = state.inner().clone();
    async_runtime::spawn(async move {
        // Wall-clock pacing: emulated time follows `rate` x elapsed real time, so
        // timer overshoot in one frame is caught up in the next (audio stays fed).
        let mut wall_start = Instant::now();
        let mut emulated_secs = 0.0f64;
        let mut paced_rate = f64::NAN;
        // Only send a video frame when its content changed (~100 KB of JSON each).
        let mut last_video_hash: Option<String> = None;
        loop {
            if !state.running.load(Ordering::SeqCst) {
                break;
            }

            let frame_start = Instant::now();
            let speed = state
                .run_speed
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
            let frame_ms = speed.frame_ms.max(1);
            let rate = speed.rate.max(0.01);
            let max_steps = speed.max_steps.max(1);
            if rate != paced_rate {
                wall_start = frame_start;
                emulated_secs = 0.0;
                paced_rate = rate;
            }

            // E clock of the machine (CoCo NTSC, Dragon PAL, MsBasic SBC, SAM speed-up).
            let clock_hz = match state.emulator.lock() {
                Ok(emu) => f64::from(emu.cpu_clock_hz().unwrap_or(DEFAULT_E_CLOCK_HZ)),
                Err(_) => break,
            };
            let frame_secs = frame_ms as f64 / 1000.0;
            let wall_secs = frame_start.duration_since(wall_start).as_secs_f64();
            let target_secs = (wall_secs + frame_secs) * rate;
            let max_lag = MAX_CATCHUP_FRAMES * frame_secs * rate;
            if target_secs - emulated_secs > max_lag {
                emulated_secs = target_secs - frame_secs * rate;
            }
            let target_cycles = ((target_secs - emulated_secs) * clock_hz).max(0.0).round() as u64;

            let mut cycles_run = 0u64;
            let mut steps = 0u32;
            let mut last = None;
            while cycles_run < target_cycles && steps < max_steps {
                if !state.running.load(Ordering::SeqCst) {
                    break;
                }
                let batch = {
                    let mut emu = match state.emulator.lock() {
                        Ok(emu) => emu,
                        Err(_) => break,
                    };
                    step_batch(
                        &mut emu,
                        cycles_run,
                        steps,
                        target_cycles,
                        max_steps,
                        RUN_LOCK_BATCH_STEPS,
                    )
                };
                cycles_run = batch.cycles_run;
                steps = batch.steps;
                if batch.last.is_some() {
                    last = batch.last;
                }
                if batch.stop {
                    state.running.store(false, Ordering::SeqCst);
                    break;
                }
                yield_run_loop().await;
            }
            emulated_secs += cycles_run as f64 / clock_hz;

            let tick = {
                let mut emu = match state.emulator.lock() {
                    Ok(emu) => emu,
                    Err(_) => break,
                };
                let audio = drain_machine_audio(&mut emu);
                let acia = get_acia_terminal(&emu);
                let video = machine_video_frame(&emu).filter(|frame| {
                    let changed = last_video_hash.as_deref() != Some(frame.hash.as_str());
                    if changed {
                        last_video_hash = Some(frame.hash.clone());
                    }
                    changed
                });
                last.map(|result| (result, emu.get_state(), audio, acia, video, steps))
            };

            if let Some((result, cpu_state, audio, acia, video, steps)) = tick {
                state.push_trace(result.clone());
                let mut payload = serde_json::json!({
                    "step": result,
                    "cpu": cpu_state,
                    "steps": steps,
                    "acia": acia,
                });
                // Omitted when unchanged: the UI keeps showing the previous frame.
                if let Some(video) = video {
                    payload["video"] = serde_json::json!(video);
                }
                if !audio.is_empty() {
                    payload["ay_audio_b64"] = serde_json::json!(encode_audio_f32_base64(&audio));
                }
                let _ = app.emit("emulator-tick", payload);
            } else {
                break;
            }

            if !state.running.load(Ordering::SeqCst) {
                break;
            }

            let deadline = frame_start + Duration::from_millis(frame_ms);
            let now = Instant::now();
            if deadline > now {
                tokio::time::sleep(deadline - now).await;
            }
        }

        state.running.store(false, Ordering::SeqCst);
        let _ = app.emit("emulator-stopped", ());
    });

    Ok(())
}

#[tauri::command]
pub fn is_emulator_running(state: State<'_, Arc<AppState>>) -> bool {
    state.running.load(Ordering::SeqCst)
}

#[tauri::command]
pub fn pause_emulator(state: State<'_, Arc<AppState>>) {
    state.running.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub fn get_cpu_state(state: State<'_, Arc<AppState>>) -> Result<CpuState, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(emu.get_state())
}

#[tauri::command]
pub async fn get_memory(
    address: u16,
    length: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryChunk, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let len = length.min(4096);
    let mut bytes = Vec::with_capacity(len as usize);
    for i in 0..len {
        // Side-effect-free: viewing I/O registers must not clear flags or eat input.
        bytes.push(emu.memory.peek8(address.wrapping_add(i)));
    }
    Ok(MemoryChunk { address, bytes })
}

#[tauri::command]
pub fn write_memory(
    address: u16,
    bytes: Vec<u8>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    for (i, b) in bytes.iter().enumerate() {
        emu.memory.write8(address.wrapping_add(i as u16), *b);
    }
    Ok(())
}

#[tauri::command]
pub fn load_binary_file(
    path: String,
    offset: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<CpuState, String> {
    let data = std::fs::read(&path).map_err(|e| format!("Failed to read {path}: {e}"))?;
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let reset_pc = emu.memory.config.reset_pc;
    emu.load_and_reset(offset, &data, reset_pc)?;
    state.clear_trace();
    Ok(emu.get_state())
}

#[tauri::command]
pub fn load_binary_bytes(
    data: Vec<u8>,
    offset: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<CpuState, String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let reset_pc = emu.memory.config.reset_pc;
    emu.load_and_reset(offset, &data, reset_pc)?;
    state.clear_trace();
    Ok(emu.get_state())
}

#[tauri::command]
pub fn export_binary_file(
    path: String,
    address: u16,
    length: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    if address as usize + length as usize > 0x10000 {
        return Err("Export range exceeds memory".into());
    }
    // What the CPU sees (ROM, cartridge, I/O), read without side effects.
    let data: Vec<u8> = (0..length)
        .map(|i| emu.memory.peek8(address.wrapping_add(i)))
        .collect();
    std::fs::write(&path, data).map_err(|e| format!("Failed to write {path}: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn assemble_source(
    source: String,
    origin: u16,
    write_to_memory: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<AssembleResult, String> {
    match assemble(&source) {
        Ok(program) => {
            if write_to_memory && !program.bytes.is_empty() {
                let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
                emu.load_program(program.origin, &program.bytes)?;
            }
            Ok(AssembleResult {
                origin: program.origin,
                bytes: program.bytes,
                errors: vec![],
                line_map: program
                    .line_map
                    .into_iter()
                    .map(|(line, addr)| (line as u32, addr))
                    .collect(),
            })
        }
        Err(error) => Ok(AssembleResult {
            origin,
            bytes: vec![],
            errors: vec![AsmErrorDto {
                line: error.line,
                message: error.message,
            }],
            line_map: std::collections::HashMap::new(),
        }),
    }
}

#[tauri::command]
pub async fn disassemble_range(
    address: u16,
    length: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DisasmLine>, String> {
    let (data, variant) = {
        let emu = state.emulator.lock().map_err(|e| e.to_string())?;
        let len = length.min(128);
        let mut bytes = Vec::with_capacity(len as usize);
        for i in 0..len {
            bytes.push(emu.memory.peek8(address.wrapping_add(i)));
        }
        (bytes, emu.get_variant())
    };

    let lines = async_runtime::spawn_blocking(move || disassemble_with_variant(&data, address, variant))
        .await
        .map_err(|e| e.to_string())?;

    Ok(lines
        .into_iter()
        .map(|l: DisassembledInsn| DisasmLine {
            address: l.address,
            bytes: l.bytes,
            text: l.text,
        })
        .collect())
}

#[tauri::command]
pub fn set_breakpoint(address: u16, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.set_breakpoint(address);
    Ok(())
}

#[tauri::command]
pub fn clear_breakpoint(address: u16, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.clear_breakpoint(address);
    Ok(())
}

#[tauri::command]
pub fn set_load_config(config: LoadConfig, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.memory.config = config;
    Ok(())
}

#[tauri::command]
pub fn get_trace(state: State<'_, Arc<AppState>>) -> Result<Vec<StepResult>, String> {
    let trace = state.trace.lock().map_err(|e| e.to_string())?;
    Ok(trace.clone())
}

#[tauri::command]
pub fn clear_trace(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.clear_trace();
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetRegisterDto {
    pub register: String,
    pub value: u16,
}

#[tauri::command]
pub fn set_cpu_register(
    dto: SetRegisterDto,
    state: State<'_, Arc<AppState>>,
) -> Result<CpuState, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.set_register(&dto.register, dto.value)?;
    Ok(emu.get_state())
}

#[tauri::command]
pub fn toggle_cpu_flag(
    flag: String,
    state: State<'_, Arc<AppState>>,
) -> Result<CpuState, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.toggle_flag(&flag)?;
    Ok(emu.get_state())
}

#[tauri::command]
pub fn get_breakpoints(state: State<'_, Arc<AppState>>) -> Result<Vec<u16>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(emu.get_breakpoints())
}

#[tauri::command]
pub fn clear_all_breakpoints(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.clear_all_breakpoints();
    Ok(())
}

#[tauri::command]
pub fn trigger_irq(state: State<'_, Arc<AppState>>) -> Result<CpuState, String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.trigger_irq();
    Ok(emu.get_state())
}

#[tauri::command]
pub fn trigger_firq(state: State<'_, Arc<AppState>>) -> Result<CpuState, String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.trigger_firq();
    Ok(emu.get_state())
}

#[tauri::command]
pub fn trigger_nmi(state: State<'_, Arc<AppState>>) -> Result<CpuState, String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.trigger_nmi();
    Ok(emu.get_state())
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunSpeedDto {
    pub rate: f64,
    pub frame_ms: u64,
    pub max_steps: u32,
}

#[tauri::command]
pub fn set_run_speed(
    speed: RunSpeedDto,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut run_speed = state.run_speed.lock().map_err(|e| e.to_string())?;
    *run_speed = RunSpeed {
        rate: speed.rate.max(0.01),
        frame_ms: speed.frame_ms.max(1),
        max_steps: speed.max_steps.max(1),
    };
    Ok(())
}

#[tauri::command]
pub fn set_watchpoint(address: u16, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.set_watchpoint(address);
    Ok(())
}

#[tauri::command]
pub fn clear_watchpoint(address: u16, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.clear_watchpoint(address);
    Ok(())
}

#[tauri::command]
pub fn get_watchpoints(state: State<'_, Arc<AppState>>) -> Result<Vec<u16>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(emu.get_watchpoints())
}

#[tauri::command]
pub fn clear_all_watchpoints(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.clear_all_watchpoints();
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub emulator: EmulatorSnapshot,
    pub asm_source: Option<String>,
}

const SESSION_VERSION: u32 = 2;

#[tauri::command]
pub fn save_session_file(
    path: String,
    asm_source: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let session = SessionFile {
        version: SESSION_VERSION,
        emulator: emu.snapshot(),
        asm_source,
    };
    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write {path}: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadSessionResult {
    pub cpu: CpuState,
    pub asm_source: Option<String>,
    pub breakpoints: Vec<u16>,
    pub watchpoints: Vec<u16>,
    pub load_config: LoadConfig,
    pub machine: MachineStateDto,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetMachineDto {
    pub kind: MachineKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetMachineResult {
    pub cpu: CpuState,
    pub load_config: LoadConfig,
    pub machine: MachineStateDto,
}

#[tauri::command]
pub fn load_session_file(
    path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadSessionResult, String> {
    state.running.store(false, Ordering::SeqCst);
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {path}: {e}"))?;
    let session: SessionFile =
        serde_json::from_str(&data).map_err(|e| format!("Invalid session file: {e}"))?;
    if session.version != SESSION_VERSION {
        return Err(format!(
            "Session version {} is unsupported (expected {}). Please re-save the session.",
            session.version, SESSION_VERSION
        ));
    }
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.restore(&session.emulator)?;
    restore_machine_io(
        &mut emu,
        &session.emulator.machine_kind,
        &session.emulator.machine_state,
    );
    let breakpoints = emu.get_breakpoints();
    let watchpoints = emu.get_watchpoints();
    let load_config = emu.memory.config.clone();
    let machine = machine_state(&emu);
    let cpu = emu.get_state();
    state.clear_trace();
    Ok(LoadSessionResult {
        cpu,
        asm_source: session.asm_source,
        breakpoints,
        watchpoints,
        load_config,
        machine,
    })
}

#[tauri::command]
pub fn list_machine_profiles() -> Vec<MachineInfo> {
    list_machines()
}

#[tauri::command]
pub fn get_machine_state(state: State<'_, Arc<AppState>>) -> Result<MachineStateDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(machine_state(&emu))
}

#[tauri::command]
pub async fn get_video_frame(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<VideoFrameDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(machine_video_frame(&emu))
}

#[tauri::command]
pub fn get_acia_config_cmd(state: State<'_, Arc<AppState>>) -> Result<AciaConfig, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_acia_config(&emu))
}

#[tauri::command]
pub fn set_acia_config_cmd(
    config: AciaConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<MachineStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_acia_config(&mut emu, config);
    Ok(machine_state(&emu))
}

#[tauri::command]
pub async fn get_acia_terminal_cmd(
    state: State<'_, Arc<AppState>>,
) -> Result<AciaTerminalDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_acia_terminal(&emu))
}

#[tauri::command]
pub async fn acia_send_input_cmd(
    text: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    acia_send_input(&emu, &text);
    Ok(())
}

#[tauri::command]
pub async fn acia_run_steps_cmd(
    steps: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<AciaTerminalDto, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("Cannot step while the emulator is running".into());
    }
    run_acia_catchup(state.inner(), None, steps).await
}

#[tauri::command]
pub async fn acia_send_and_run_cmd(
    text: String,
    steps: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<AciaTerminalDto, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("Cannot process ACIA input while the emulator is running".into());
    }
    run_acia_catchup(state.inner(), Some(text), steps).await
}

async fn run_acia_catchup(
    state: &Arc<AppState>,
    text: Option<String>,
    steps: u32,
) -> Result<AciaTerminalDto, String> {
    let n = clamp_acia_catchup_steps(steps);
    let mut cycles_run = 0u64;
    let mut done = 0u32;
    if let Some(text) = text {
        let emu = state.emulator.lock().map_err(|e| e.to_string())?;
        acia_send_input(&emu, &text);
    }
    while done < n {
        if state.running.load(Ordering::SeqCst) {
            break;
        }
        let batch = {
            let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
            step_batch(&mut emu, cycles_run, done, u64::MAX, n, RUN_LOCK_BATCH_STEPS)
        };
        cycles_run = batch.cycles_run;
        done = batch.steps;
        if batch.stop || done == 0 {
            break;
        }
        yield_run_loop().await;
    }
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_acia_terminal(&emu))
}

#[tauri::command]
pub fn clear_acia_terminal_cmd(state: State<'_, Arc<AppState>>) -> Result<AciaTerminalDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    clear_acia_terminal(&emu);
    Ok(get_acia_terminal(&emu))
}

#[tauri::command]
pub fn get_pia_config_cmd(state: State<'_, Arc<AppState>>) -> Result<Option<PiaConfig>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_pia_config(&emu))
}

#[tauri::command]
pub fn set_pia_config_cmd(
    config: PiaConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<MachineStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_pia_config(&mut emu, config);
    Ok(machine_state(&emu))
}

#[tauri::command]
pub async fn get_pia_state_cmd(state: State<'_, Arc<AppState>>) -> Result<Option<PiaStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_pia_state(&emu))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetPiaInputDto {
    pub port: String,
    pub bit: u8,
    pub on: bool,
}

#[tauri::command]
pub fn set_pia_input_cmd(
    dto: SetPiaInputDto,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PiaStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_pia_input(&emu, &dto.port, dto.bit, dto.on);
    Ok(get_pia_state(&emu))
}

// ---- AY-3-8910 commands ----

#[tauri::command]
pub fn get_ay_config_cmd(state: State<'_, Arc<AppState>>) -> Result<AyConfig, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_ay_config(&emu))
}

#[tauri::command]
pub fn set_ay_config_cmd(
    config: AyConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<MachineStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_ay_config(&mut emu, config);
    Ok(machine_state(&emu))
}

#[tauri::command]
pub async fn get_ay_state_cmd(state: State<'_, Arc<AppState>>) -> Result<Option<AyStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_ay_state(&emu))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetAyPortInputDto {
    pub port: String,
    pub value: u8,
}

#[tauri::command]
pub fn set_ay_port_input_cmd(
    dto: SetAyPortInputDto,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<AyStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let port = dto.port.chars().next().unwrap_or('a');
    ay_set_port_input(&emu, port, dto.value);
    Ok(get_ay_state(&emu))
}

// ---- SP0256 / CTS256 speech commands ----

/// Paused catch-up budget: ~20 s of 44.1 kHz audio covers a long sentence plus
/// the CTS256A's "O.K." greeting after a reset.
const SPEECH_CATCHUP_SAMPLES: usize = 900_000;

#[tauri::command]
pub fn get_speech_config_cmd(state: State<'_, Arc<AppState>>) -> Result<SpeechConfig, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_speech_config(&emu))
}

#[tauri::command]
pub fn set_speech_config_cmd(
    config: SpeechConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<MachineStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_speech_config(&mut emu, config);
    Ok(machine_state(&emu))
}

#[tauri::command]
pub async fn get_speech_state_cmd(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<SpeechStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_speech_state(&emu))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeechSayDto {
    pub text: String,
}

/// Speak a text via the CTS256 front end. If the emulator is paused, run a
/// catch-up so the utterance is fully rendered and audible, returning the mixed
/// audio as base64 for the frontend to play immediately.
#[tauri::command]
pub async fn speech_say_cmd(
    dto: SpeechSayDto,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let running = state.running.load(Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    speech_say_text(&mut emu, &dto.text);
    if running {
        // The run loop will render + emit audio on its own.
        return Ok(None);
    }
    // Everything that was rendered, not just what fits the 2 s playback buffer.
    let mut audio = speech_drain_audio(&mut emu);
    audio.extend(speech_run_until_idle_collect(&mut emu, SPEECH_CATCHUP_SAMPLES));
    if audio.is_empty() {
        Ok(None)
    } else {
        Ok(Some(encode_audio_f32_base64(&audio)))
    }
}

/// Switch the CTS256A "O.K." greeting after reset on/off.
#[tauri::command]
pub fn set_speech_greeting_cmd(
    on: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<SpeechStateDto>, String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_speech_greeting(&mut emu, on);
    Ok(get_speech_state(&emu))
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetCpuVariantDto {
    pub variant: CpuVariant,
}

#[tauri::command]
pub fn get_cpu_variant(state: State<'_, Arc<AppState>>) -> Result<CpuVariant, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(emu.get_variant())
}

#[tauri::command]
pub fn set_cpu_variant(
    dto: SetCpuVariantDto,
    state: State<'_, Arc<AppState>>,
) -> Result<CpuState, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    emu.set_variant(dto.variant);
    Ok(emu.get_state())
}

#[tauri::command]
pub fn set_machine_profile(
    dto: SetMachineDto,
    state: State<'_, Arc<AppState>>,
) -> Result<SetMachineResult, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let load_config = apply_machine(&mut emu, dto.kind);
    let machine = machine_state(&emu);
    let cpu = emu.get_state();
    state.clear_trace();
    Ok(SetMachineResult {
        cpu,
        load_config,
        machine,
    })
}

#[tauri::command]
pub async fn machine_key_event(
    code: String,
    down: bool,
    key: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    machine_host_key_event(&mut emu, &code, key.as_deref(), down);
    Ok(())
}

#[tauri::command]
pub fn machine_keys_clear(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    machine_clear_keys(&mut emu);
    Ok(())
}

// ---- CoCo / Dragon peripherals ----

const NO_BOARD: &str = "This machine profile has no CoCo/Dragon peripheral port";

#[tauri::command]
pub fn machine_set_joystick(
    port: u8,
    x: u8,
    y: u8,
    button: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    m6809_machine::machine_set_joystick(&emu, usize::from(port.min(1)), x, y, button);
    Ok(())
}

#[tauri::command]
pub fn machine_cassette_insert(
    name: String,
    data: Vec<u8>,
    state: State<'_, Arc<AppState>>,
) -> Result<CassetteStateDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    cassette_insert(&emu, name, data).ok_or_else(|| NO_BOARD.to_string())
}

#[tauri::command]
pub fn machine_cassette_eject(state: State<'_, Arc<AppState>>) -> Result<CassetteStateDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    cassette_eject(&emu).ok_or_else(|| NO_BOARD.to_string())
}

#[tauri::command]
pub fn machine_cassette_rewind(state: State<'_, Arc<AppState>>) -> Result<CassetteStateDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    cassette_rewind(&emu).ok_or_else(|| NO_BOARD.to_string())
}

#[tauri::command]
pub fn machine_cassette_state(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<CassetteStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(cassette_state(&emu))
}

#[tauri::command]
pub fn machine_cassette_take_recording(state: State<'_, Arc<AppState>>) -> Result<Vec<u8>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(cassette_take_recording(&emu))
}

#[tauri::command]
pub fn machine_printer_take_output(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(printer_take_output(&emu))
}

#[tauri::command]
pub fn machine_cartridge_insert(
    name: String,
    data: Vec<u8>,
    autostart: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<CartridgeStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let result =
        cartridge_insert(&mut emu, name, data, autostart).ok_or_else(|| NO_BOARD.to_string());
    state.clear_trace();
    result
}

#[tauri::command]
pub fn machine_cartridge_eject(state: State<'_, Arc<AppState>>) -> Result<CartridgeStateDto, String> {
    state.running.store(false, Ordering::SeqCst);
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let result = cartridge_eject(&mut emu).ok_or_else(|| NO_BOARD.to_string());
    state.clear_trace();
    result
}

#[tauri::command]
pub fn machine_cartridge_state(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<CartridgeStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(cartridge_state(&emu))
}

/// Drive a control line of the bare-metal 6821 from the UI.
#[tauri::command]
pub fn set_pia_control_line_cmd(
    line: String,
    level: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PiaStateDto>, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    set_pia_control_line(&emu, &line.to_ascii_lowercase(), level);
    Ok(get_pia_state(&emu))
}

#[tauri::command]
pub fn set_trace_limit(limit: usize, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let clamped = limit.clamp(10, 1000);
    let mut trace_limit = state.trace_limit.lock().map_err(|e| e.to_string())?;
    *trace_limit = clamped;
    Ok(())
}

impl AppState {
    fn clear_trace(&self) {
        if let Ok(mut trace) = self.trace.lock() {
            trace.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Wire-format contract: assemble IPC exposes `line_map` (snake_case), matching the UI.
    #[test]
    fn assemble_result_serializes_line_map_snake_case() {
        let mut line_map = HashMap::new();
        line_map.insert(3u32, 0x0100u16);
        let result = AssembleResult {
            origin: 0x0100,
            bytes: vec![0x86, 0x42],
            errors: vec![],
            line_map,
        };
        let value = serde_json::to_value(&result).expect("serialize");
        assert!(
            value.get("line_map").is_some(),
            "expected snake_case line_map key, got: {value}"
        );
        assert!(
            value.get("lineMap").is_none(),
            "must not emit camelCase lineMap"
        );
        assert_eq!(value["line_map"]["3"], 0x0100);
        assert_eq!(value["origin"], 0x0100);
        assert_eq!(value["bytes"], serde_json::json!([0x86, 0x42]));
    }

    #[test]
    fn assemble_error_dto_serializes_expected_fields() {
        let result = AssembleResult {
            origin: 0x0100,
            bytes: vec![],
            errors: vec![AsmErrorDto {
                line: 4,
                message: "unknown mnemonic".into(),
            }],
            line_map: HashMap::new(),
        };
        let value = serde_json::to_value(&result).expect("serialize");
        assert_eq!(value["errors"][0]["line"], 4);
        assert_eq!(value["errors"][0]["message"], "unknown mnemonic");
    }

    #[test]
    fn acia_catchup_budget_covers_msbasic_line() {
        assert!(
            clamp_acia_catchup_steps(400_000) >= 400_000,
            "MS BASIC needs ~400k steps to evaluate a line"
        );
    }

    #[test]
    fn run_lock_batch_stays_ui_sized() {
        const _: () = assert!(RUN_LOCK_BATCH_STEPS <= 4_096 && RUN_LOCK_BATCH_STEPS >= 256);
    }

    #[test]
    fn emulator_lock_is_released_between_batches() {
        use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
        use std::sync::Mutex;
        use std::thread;
        use std::time::{Duration, Instant};

        let mut seed = m6809_core::Emulator::new();
        // Tight BRA * loop so the slice actually burns instructions.
        seed.memory.write8(0x0100, 0x20);
        seed.memory.write8(0x0101, 0xFE);
        seed.cpu.pc = 0x0100;

        let emu = Arc::new(Mutex::new(seed));
        let acquired = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));

        let waiter_emu = emu.clone();
        let waiter_flag = acquired.clone();
        let waiter_started = started.clone();
        let waiter = thread::spawn(move || {
            while !waiter_started.load(AtomicOrdering::SeqCst) {
                thread::yield_now();
            }
            let start = Instant::now();
            let _guard = waiter_emu.lock().expect("lock");
            waiter_flag.store(true, AtomicOrdering::SeqCst);
            start.elapsed()
        });

        let mut cycles_run = 0u64;
        let mut steps = 0u32;
        const TOTAL: u32 = 80_000;
        while steps < TOTAL {
            {
                let mut guard = emu.lock().expect("lock");
                started.store(true, AtomicOrdering::SeqCst);
                let batch = step_batch(
                    &mut guard,
                    cycles_run,
                    steps,
                    u64::MAX,
                    TOTAL,
                    RUN_LOCK_BATCH_STEPS,
                );
                cycles_run = batch.cycles_run;
                steps = batch.steps;
                if batch.stop {
                    break;
                }
            }
            thread::yield_now();
        }

        let wait = waiter.join().expect("waiter");
        assert!(
            acquired.load(AtomicOrdering::SeqCst),
            "UI thread never got the emulator lock"
        );
        assert!(
            wait < Duration::from_millis(80),
            "lock wait {wait:?} is too long for a UI command"
        );
        assert!(steps >= RUN_LOCK_BATCH_STEPS);
    }
}
