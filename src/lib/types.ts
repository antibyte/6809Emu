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
  /** SYNC or CWAI is waiting for an interrupt (emulated time keeps running). */
  waiting?: boolean;
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
  /** Legacy JSON float array (unused when ay_audio_b64 is present). */
  ay_audio?: number[];
  /** Base64-encoded little-endian f32 PCM mono @ 44100 Hz. */
  ay_audio_b64?: string;
  acia?: AciaTerminalState;
  video?: VideoFrame | null;
}

export interface TraceEntry extends StepResult {
  id: number;
}

export type MachineKind = "bare" | "coco2" | "dragon32" | "ms_basic";

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
  /** Line rate with the /16 counter divide (TX/RX clock = 16 x baud). */
  baud: number;
  /** Rate of the CPU E cycles that clock the ACIA model. */
  e_clock_hz: number;
  /** Motorola RS = A0 order: status/control at base, data at base+1.
   *  Default (false): legacy order, data at base, status/control at base+1. */
  motorola?: boolean;
  /** Strict receive timing: the host sends back-to-back, unread characters
   *  are lost (OVRN). Default (false): next character only after RDR was read. */
  strict_rx?: boolean;
}

export interface AciaTerminalState {
  /** Rendered terminal text (BS/CR/LF applied, bytes >= $80 as Latin-1). */
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
  speech: SpeechConfig;
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
  /** /IRQA asserted (flag AND enable). */
  irq_a: boolean;
  /** /IRQB asserted (flag AND enable). */
  irq_b: boolean;
  irqa1?: boolean;
  irqa2?: boolean;
  irqb1?: boolean;
  irqb2?: boolean;
  ca1?: boolean;
  ca2?: boolean;
  cb1?: boolean;
  cb2?: boolean;
  ca2_is_output?: boolean;
  cb2_is_output?: boolean;
  ca2_out?: boolean;
  cb2_out?: boolean;
  ca2_strobes?: number;
  cb2_strobes?: number;
}

export interface AyConfig {
  enabled: boolean;
  base_addr: number;
  chip_clock_hz: number;
}

export interface AyState {
  config: AyConfig;
  /** R0-R15 as stored (masked); R14/R15 are the output latches. */
  registers: number[];
  selected_register: number;
  port_a_in: number;
  port_b_in: number;
  /** Last byte written to the address latch. */
  address_latch?: number;
  /** Latched A7-A4 were 0: data accesses reach the chip. */
  chip_selected?: boolean;
  /** Current envelope generator level (0-15). */
  envelope_level?: number;
}

export interface SpeechConfig {
  enabled: boolean;
  base_addr: number;
  xtal_hz: number;
  cts_enabled: boolean;
}

export interface SpeechState {
  config: SpeechConfig;
  lrq_ready: boolean;
  standby: boolean;
  speaking: boolean;
  last_allophone: number;
  cts_enabled: boolean;
  /** Text or allophones in flight, or the CTS ROM is still booting. */
  cts_busy: boolean;
  cts_last_allophone: number;
  /** CTS ROM initialising / saying its "O.K." greeting. */
  cts_booting?: boolean;
  /** Characters waiting (host FIFO + latch + CTS input buffer). */
  cts_input_pending?: number;
  /** Allophones waiting in the CTS output buffer. */
  cts_output_pending?: number;
  /** Host FIFO full: writes to base+2 are dropped. */
  cts_fifo_full?: boolean;
  /** BUSY* pin: CTS input buffer ≥ 87.5 % full. */
  cts_buffer_full?: boolean;
  /** Allophones recently sent to the SP0256, oldest first. */
  cts_recent?: number[];
  /** Carriage-return-only delimiter mode (text is spoken after a CR). */
  cts_cr_only?: boolean;
  /** The CTS says "O.K." after a reset. */
  cts_greeting?: boolean;
}

/** One rendered MC6847 / SAM field (backend `VideoFrameDto`). */
export interface VideoFrame {
  /** Framebuffer size in pixels (320 × 240: 256 × 192 active area plus border). */
  width: number;
  height: number;
  /** Position and size of the 256 × 192 active area inside the framebuffer. */
  active_x: number;
  active_y: number;
  active_width: number;
  active_height: number;
  /** Base64 of width × height palette indices, one byte per pixel, row-major. */
  pixels: string;
  /** Palette colours ("#rrggbb"), indexed by the pixel values. */
  palette: string[];
  /** "Text32x16", "SG6", "SG8", "SG12", "SG24", "CG1" … "RG6"; a foreign SAM mode adds "/V<n>". */
  mode: string;
  /** Video RAM base address (SAM F × 512). */
  base_addr: number;
  /** Bytes of video RAM shown, from base_addr. */
  vram_bytes: number;
  /** Logical resolution: 32 × 16 characters for text, elements / pixels otherwise. */
  cols: number;
  rows: number;
  /** Text decode of an alphanumeric screen (16 × 32); empty for graphics. */
  rows_text: string[];
  /** Hash of the picture: unchanged hash = identical frame. */
  hash: string;
  sam_v: number;
  sam_f: number;
  /** PIA1 port B VDG control levels (CSS, GM0-2, A/G). */
  vdg_ctrl: number;
}