import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  CpuState,
  DisasmLine,
  CpuVariant,
  AciaConfig,
  AciaTerminalState,
  AyConfig,
  AyState,
  SpeechConfig,
  SpeechState,
  MachineInfo,
  MachineKind,
  MachineState,
  PiaConfig,
  PiaState,
  VideoFrame,
  StepResult,
  TickPayload,
} from "./types";

export type { TickPayload };

export interface RunSpeedConfig {
  rate: number;
  frame_ms: number;
  max_steps: number;
}

export async function resetEmulator(): Promise<CpuState> {
  return invoke("reset_emulator");
}

export async function stepEmulator(): Promise<StepResult> {
  return invoke("step");
}

export async function runEmulator(): Promise<void> {
  return invoke("run_emulator");
}

export async function pauseEmulator(): Promise<void> {
  return invoke("pause_emulator");
}

export async function getCpuState(): Promise<CpuState> {
  return invoke("get_cpu_state");
}

export async function getMemory(address: number, length: number) {
  return invoke<{ address: number; bytes: number[] }>("get_memory", {
    address,
    length,
  });
}

export async function writeMemory(address: number, bytes: number[]) {
  return invoke("write_memory", { address, bytes });
}

export async function disassembleRange(
  address: number,
  length: number
): Promise<DisasmLine[]> {
  return invoke("disassemble_range", { address, length });
}

export interface AssembleResult {
  origin: number;
  bytes: number[];
  errors: { line: number; message: string }[];
  /** 1-based source line → address (snake_case matches Rust serde IPC). */
  line_map: Record<number, number>;
}

export async function assembleSource(
  source: string,
  origin: number,
  writeToMemory: boolean
): Promise<AssembleResult> {
  return invoke<AssembleResult>("assemble_source", {
    source,
    origin,
    writeToMemory,
  });
}

export async function setBreakpoint(address: number) {
  return invoke("set_breakpoint", { address });
}

export async function clearBreakpoint(address: number) {
  return invoke("clear_breakpoint", { address });
}

export async function getBreakpoints(): Promise<number[]> {
  return invoke("get_breakpoints");
}

export async function clearAllBreakpoints() {
  return invoke("clear_all_breakpoints");
}

export async function setLoadConfig(loadAddr: number, resetPc: number) {
  return invoke("set_load_config", {
    config: { load_addr: loadAddr, reset_pc: resetPc },
  });
}

export async function getTrace(): Promise<StepResult[]> {
  return invoke("get_trace");
}

export async function clearTrace(): Promise<void> {
  return invoke("clear_trace");
}

export async function setCpuRegister(
  register: string,
  value: number
): Promise<CpuState> {
  return invoke("set_cpu_register", { dto: { register, value } });
}

export async function toggleCpuFlag(flag: string): Promise<CpuState> {
  return invoke("toggle_cpu_flag", { flag });
}

export async function triggerIrq(): Promise<CpuState> {
  return invoke("trigger_irq");
}

export async function triggerFirq(): Promise<CpuState> {
  return invoke("trigger_firq");
}

export async function triggerNmi(): Promise<CpuState> {
  return invoke("trigger_nmi");
}

export async function setRunSpeed(speed: RunSpeedConfig): Promise<void> {
  return invoke("set_run_speed", { speed });
}

export async function setTraceLimit(limit: number): Promise<void> {
  return invoke("set_trace_limit", { limit });
}

export async function importBinary(
  loadAddr: number,
  dialogTitle?: string
): Promise<CpuState | null> {
  const path = await open({
    title: dialogTitle,
    multiple: false,
    filters: [
      { name: "Binary", extensions: ["bin", "rom", "dat"] },
      { name: "All", extensions: ["*"] },
    ],
  });
  if (!path) return null;
  try {
    return await invoke("load_binary_file", { path, offset: loadAddr });
  } catch (e) {
    throw new Error(e instanceof Error ? e.message : String(e));
  }
}

export async function exportBinary(
  startAddr: number,
  length: number,
  dialogTitle?: string
): Promise<boolean> {
  const path = await save({
    title: dialogTitle,
    filters: [{ name: "Binary", extensions: ["bin"] }],
  });
  if (!path) return false;
  try {
    await invoke("export_binary_file", { path, address: startAddr, length });
    return true;
  } catch (e) {
    throw new Error(e instanceof Error ? e.message : String(e));
  }
}

export function onEmulatorTick(
  callback: (payload: TickPayload) => void
): Promise<() => void> {
  return listen<TickPayload>("emulator-tick", (event) => {
    callback(event.payload);
  });
}

const stopWaiters: Array<() => void> = [];

function resolveStopWaiters() {
  while (stopWaiters.length > 0) {
    const resolve = stopWaiters.pop();
    resolve?.();
  }
}

export function onEmulatorStopped(
  callback: () => void
): Promise<() => void> {
  return listen("emulator-stopped", () => {
    resolveStopWaiters();
    callback();
  });
}

export async function isEmulatorRunning(): Promise<boolean> {
  return invoke<boolean>("is_emulator_running");
}

export async function waitForEmulatorStop(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  stopWaiters.push(resolve);

  isEmulatorRunning()
    .then((running) => {
      if (!running) {
        const idx = stopWaiters.indexOf(resolve);
        if (idx !== -1) {
          stopWaiters.splice(idx, 1);
          resolve();
        }
      }
    })
    .catch(() => {
      const idx = stopWaiters.indexOf(resolve);
      if (idx !== -1) stopWaiters.splice(idx, 1);
      resolve();
    });

  return promise;
}

export async function setWatchpoint(address: number) {
  return invoke("set_watchpoint", { address });
}

export async function clearWatchpoint(address: number) {
  return invoke("clear_watchpoint", { address });
}

export async function getWatchpoints(): Promise<number[]> {
  return invoke("get_watchpoints");
}

export async function clearAllWatchpoints() {
  return invoke("clear_all_watchpoints");
}

export interface LoadSessionResult {
  cpu: CpuState;
  asm_source: string | null;
  breakpoints: number[];
  watchpoints: number[];
  load_config: { load_addr: number; reset_pc: number };
  machine: MachineState;
}

export interface SetMachineResult {
  cpu: CpuState;
  load_config: { load_addr: number; reset_pc: number };
  machine: MachineState;
}

export async function saveSession(
  path: string,
  asmSource: string | null
): Promise<void> {
  return invoke("save_session_file", { path, asmSource });
}

export async function loadSession(path: string): Promise<LoadSessionResult> {
  return invoke("load_session_file", { path });
}

export async function saveSessionDialog(
  asmSource: string | null,
  dialogTitle?: string
): Promise<boolean> {
  const path = await save({
    title: dialogTitle,
    filters: [{ name: "Session", extensions: ["json"] }],
  });
  if (!path) return false;
  await saveSession(path, asmSource);
  return true;
}

export async function loadSessionDialog(
  dialogTitle?: string
): Promise<LoadSessionResult | null> {
  const path = await open({
    title: dialogTitle,
    multiple: false,
    filters: [{ name: "Session", extensions: ["json"] }],
  });
  if (!path || Array.isArray(path)) return null;
  return loadSession(path);
}

export async function listMachineProfiles(): Promise<MachineInfo[]> {
  return invoke("list_machine_profiles");
}

export async function getMachineState(): Promise<MachineState> {
  return invoke("get_machine_state");
}

export async function getVideoFrame(): Promise<VideoFrame | null> {
  return invoke("get_video_frame");
}

export async function getAciaConfig(): Promise<AciaConfig> {
  return invoke("get_acia_config_cmd");
}

export async function setAciaConfig(config: AciaConfig): Promise<MachineState> {
  return invoke("set_acia_config_cmd", { config });
}

export async function getAciaTerminal(): Promise<AciaTerminalState> {
  return invoke("get_acia_terminal_cmd");
}

export async function aciaSendInput(text: string): Promise<void> {
  return invoke("acia_send_input_cmd", { text });
}

export async function aciaRunSteps(steps: number): Promise<AciaTerminalState> {
  return invoke("acia_run_steps_cmd", { steps });
}

export async function aciaSendAndRun(
  text: string,
  steps = 12000
): Promise<AciaTerminalState> {
  return invoke("acia_send_and_run_cmd", { text, steps });
}

export async function clearAciaTerminal(): Promise<AciaTerminalState> {
  return invoke("clear_acia_terminal_cmd");
}

export async function setMachineProfile(kind: MachineKind): Promise<SetMachineResult> {
  return invoke("set_machine_profile", { dto: { kind } });
}

/** Host key event: `code` = KeyboardEvent.code, `key` = KeyboardEvent.key (typed character). */
export async function machineKeyEvent(code: string, down: boolean, key?: string): Promise<void> {
  return invoke("machine_key_event", { code, down, key: key ?? null });
}

export async function machineKeysClear(): Promise<void> {
  return invoke("machine_keys_clear");
}

export async function getCpuVariant(): Promise<CpuVariant> {
  return invoke("get_cpu_variant");
}

export async function setCpuVariant(variant: CpuVariant): Promise<CpuState> {
  return invoke("set_cpu_variant", { dto: { variant } });
}

export async function getPiaConfig(): Promise<PiaConfig | null> {
  return invoke("get_pia_config_cmd");
}

export async function setPiaConfig(config: PiaConfig): Promise<MachineState> {
  return invoke("set_pia_config_cmd", { config });
}

export async function getPiaState(): Promise<PiaState | null> {
  return invoke("get_pia_state_cmd");
}

export async function setPiaInput(
  port: "a" | "b",
  bit: number,
  on: boolean
): Promise<PiaState | null> {
  return invoke("set_pia_input_cmd", { dto: { port, bit, on } });
}

// ---- AY-3-8910 ----

export async function getAyConfig(): Promise<AyConfig> {
  return invoke("get_ay_config_cmd");
}

export async function setAyConfig(config: AyConfig): Promise<MachineState> {
  return invoke("set_ay_config_cmd", { config });
}

export async function getAyState(): Promise<AyState | null> {
  return invoke("get_ay_state_cmd");
}

export async function setAyPortInput(
  port: "a" | "b",
  value: number
): Promise<AyState | null> {
  return invoke("set_ay_port_input_cmd", { dto: { port, value } });
}

// ---- SP0256 / CTS256 speech ----

export async function getSpeechConfig(): Promise<SpeechConfig> {
  return invoke("get_speech_config_cmd");
}

export async function setSpeechConfig(config: SpeechConfig): Promise<MachineState> {
  return invoke("set_speech_config_cmd", { config });
}

export async function getSpeechState(): Promise<SpeechState | null> {
  return invoke("get_speech_state_cmd");
}

/** Speak text via CTS256. Returns base64 audio when the emulator is paused. */
export async function speechSay(text: string): Promise<string | null> {
  return invoke("speech_say_cmd", { dto: { text } });
}

/** CTS256A "O.K." greeting after reset on/off. */
export async function setSpeechGreeting(on: boolean): Promise<SpeechState | null> {
  return invoke("set_speech_greeting_cmd", { on });
}

export const RUN_SPEED_PRESETS: { label: string; config: RunSpeedConfig }[] = [
  { label: "1×", config: { rate: 1.0, frame_ms: 50, max_steps: 250_000 } },
  { label: "2×", config: { rate: 2.0, frame_ms: 50, max_steps: 500_000 } },
  { label: "5×", config: { rate: 5.0, frame_ms: 50, max_steps: 500_000 } },
  { label: "Max", config: { rate: 50.0, frame_ms: 16, max_steps: 1_000_000 } },
];