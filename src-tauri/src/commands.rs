use std::sync::atomic::Ordering;
use std::sync::Arc;

use m6809_asm::{assemble, disassemble_with_variant, DisassembledInsn};
use m6809_core::{CpuState, CpuVariant, EmulatorSnapshot, LoadConfig, StepResult};
use m6809_machine::{
    acia_send_input, apply_machine, ay_set_port_input, ay_take_samples, clear_acia_terminal,
    get_acia_config, get_acia_terminal, get_ay_config, get_ay_state, get_pia_config, get_pia_state,
    list_machines, machine_clear_keys, machine_host_key, machine_state, machine_video_frame,
    restore_machine_io, set_acia_config, set_ay_config, set_pia_config, set_pia_input, AciaConfig,
    AciaTerminalDto, AyConfig, AyStateDto, MachineInfo, MachineKind, MachineStateDto, PiaConfig,
    PiaStateDto, VideoFrameDto, AUDIO_SAMPLE_RATE,
};
use serde::{Deserialize, Serialize};
use tauri::{async_runtime, AppHandle, Emitter, State};

use crate::state::{AppState, RunSpeed};
use std::time::{Duration, Instant};

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
    emu.memory.write16(0xFFFE, reset_pc);
    emu.reset();
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

    let state = state.inner().clone();
    async_runtime::spawn(async move {
        // Prime so the first chunk covers a full UI frame (builds a small lead).
        let mut last_audio_at = Instant::now()
            - Duration::from_millis(
                state
                    .run_speed
                    .lock()
                    .map(|s| s.frame_ms)
                    .unwrap_or(50),
            );

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

            // Match audio length to real wall time since the last chunk so
            // work+IPC overhead cannot starve the stream (that sounded "choppy").
            let elapsed = frame_start.saturating_duration_since(last_audio_at);
            let mut target_samples =
                ((elapsed.as_secs_f64() * f64::from(AUDIO_SAMPLE_RATE)).round() as usize).max(1);
            // Clamp: 5 ms .. 100 ms per emit (keeps IPC and latency bounded).
            let min_samples = (AUDIO_SAMPLE_RATE as usize) / 200;
            let max_samples = (AUDIO_SAMPLE_RATE as usize) / 10;
            target_samples = target_samples.clamp(min_samples, max_samples);

            let tick = {
                let mut emu = match state.emulator.lock() {
                    Ok(emu) => emu,
                    Err(_) => break,
                };

                if emu.cpu.halted {
                    state.running.store(false, Ordering::SeqCst);
                    break;
                }

                let mut last = None;
                for _ in 0..speed.steps_per_tick {
                    if !state.running.load(Ordering::SeqCst) || emu.cpu.halted {
                        break;
                    }
                    let result = emu.step();
                    let stop = result.trap.is_some();
                    last = Some(result);
                    if stop {
                        state.running.store(false, Ordering::SeqCst);
                        break;
                    }
                }

                let audio = ay_take_samples(&mut emu, target_samples);
                last.map(|result| (result, emu.get_state(), audio))
            };

            last_audio_at = frame_start;

            if let Some((result, cpu_state, audio)) = tick {
                state.push_trace(result.clone());
                let mut payload = serde_json::json!({
                    "step": result,
                    "cpu": cpu_state,
                });
                if !audio.is_empty() {
                    payload["ay_audio"] = serde_json::json!(audio);
                }
                let _ = app.emit("emulator-tick", payload);
            } else {
                break;
            }

            if !state.running.load(Ordering::SeqCst) {
                break;
            }

            // Sleep only the remainder of the frame budget (never sleep full
            // frame_ms on top of work time — that was the main underrun source).
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
pub fn get_memory(
    address: u16,
    length: u16,
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryChunk, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let len = length.min(4096);
    let mut bytes = Vec::with_capacity(len as usize);
    for i in 0..len {
        bytes.push(emu.memory.read8(address.wrapping_add(i)));
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
    let data = emu.memory.export_range(address, length)?;
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
            bytes.push(emu.memory.read8(address.wrapping_add(i)));
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
    pub steps_per_tick: u32,
    pub frame_ms: u64,
}

#[tauri::command]
pub fn set_run_speed(
    speed: RunSpeedDto,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut run_speed = state.run_speed.lock().map_err(|e| e.to_string())?;
    *run_speed = RunSpeed {
        steps_per_tick: speed.steps_per_tick.max(1),
        frame_ms: speed.frame_ms.max(1),
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
pub fn get_video_frame(
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
pub fn get_acia_terminal_cmd(state: State<'_, Arc<AppState>>) -> Result<AciaTerminalDto, String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    Ok(get_acia_terminal(&emu))
}

#[tauri::command]
pub fn acia_send_input_cmd(text: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let emu = state.emulator.lock().map_err(|e| e.to_string())?;
    acia_send_input(&emu, &text);
    Ok(())
}

#[tauri::command]
pub fn acia_run_steps_cmd(
    steps: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<AciaTerminalDto, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("Cannot step while the emulator is running".into());
    }
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    let n = steps.min(50_000);
    for _ in 0..n {
        emu.step();
    }
    Ok(get_acia_terminal(&emu))
}

#[tauri::command]
pub fn acia_send_and_run_cmd(
    text: String,
    steps: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<AciaTerminalDto, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("Cannot process ACIA input while the emulator is running".into());
    }
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    acia_send_input(&emu, &text);
    let n = steps.min(50_000);
    for _ in 0..n {
        emu.step();
    }
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
pub fn get_pia_state_cmd(state: State<'_, Arc<AppState>>) -> Result<Option<PiaStateDto>, String> {
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
pub fn get_ay_state_cmd(state: State<'_, Arc<AppState>>) -> Result<Option<AyStateDto>, String> {
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
pub fn machine_key_event(
    code: String,
    down: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    machine_host_key(&mut emu, &code, down);
    Ok(())
}

#[tauri::command]
pub fn machine_keys_clear(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut emu = state.emulator.lock().map_err(|e| e.to_string())?;
    machine_clear_keys(&mut emu);
    Ok(())
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