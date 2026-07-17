export interface FlagState {
  c: boolean;
  v: boolean;
  z: boolean;
  n: boolean;
  i: boolean;
  h: boolean;
  f: boolean;
  e: boolean;
}

export type CpuVariant = "mc6809" | "hd6309";

export interface CpuState {
  a: number;
  b: number;
  d: number;
  x: number;
  y: number;
  u: number;
  s: number;
  pc: number;
  dp: number;
  cc: number;
  flags: FlagState;
  total_cycles: number;
  halted: boolean;
  irq_pending: boolean;
  firq_pending: boolean;
  nmi_pending: boolean;
  lds_encountered?: boolean;
  variant?: CpuVariant;
  w?: number;
  v?: number;
  mode_reg?: number;
}

export interface StepResult {
  cycles: number;
  pc_before: number;
  pc_after: number;
  opcode: number;
  bytes: number[];
  mnemonic: string;
  operands: string;
  trap: string | null;
}

export interface DisasmLine {
  address: number;
  bytes: number[];
  text: string;
}

export interface TickPayload {
  step: StepResult;
  cpu: CpuState;
  steps?: number;
  ay_audio?: number[];
}

export interface TraceEntry extends StepResult {
  id: number;
}

export type MachineKind = "bare" | "coco2" | "dragon32";

export interface MachineInfo {
  kind: MachineKind;
  name: string;
  load_addr: number;
  reset_pc: number;
  description: string;
}

export interface IoRegister {
  address: number;
  name: string;
  value: number;
}

export interface AciaConfig {
  enabled: boolean;
  base_addr: number;
  baud: number;
  e_clock_hz: number;
}

export interface AciaTerminalState {
  tx_text: string;
  rdrf: boolean;
  tdre: boolean;
  irq: boolean;
}

export interface FirmwareRegion {
  name: string;
  address: number;
  size: number;
}

export interface FirmwareInfo {
  kind: MachineKind;
  name: string;
  present: boolean;
  reset_pc: number;
  regions: FirmwareRegion[];
}

export interface MachineState {
  kind: MachineKind;
  io_registers: IoRegister[];
  acia: AciaConfig;
  pia: PiaConfig | null;
  ay: AyConfig;
  firmware?: FirmwareInfo | null;
}

export interface PiaConfig {
  enabled: boolean;
  base_addr: number;
}

export interface PiaState {
  config: PiaConfig;
  ddra: number;
  ddrb: number;
  ora: number;
  orb: number;
  ira: number;
  irb: number;
  cra: number;
  crb: number;
  port_a_read: number;
  port_b_read: number;
  irq_a: boolean;
  irq_b: boolean;
}

export interface AyConfig {
  enabled: boolean;
  base_addr: number;
  chip_clock_hz: number;
}

export interface AyState {
  config: AyConfig;
  registers: number[];
  selected_register: number;
  port_a_in: number;
  port_b_in: number;
}

export interface VideoFrame {
  cols: number;
  rows: number;
  base_addr: number;
  mode: string;
  cells: number[];
  rows_text: string[];
}