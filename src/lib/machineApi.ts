// CoCo 2 / Dragon 32 peripherals: joystick, cassette, printer, cartridge.
import { invoke } from "@tauri-apps/api/core";

/** 0 = right joystick, 1 = left joystick. */
export type JoystickPort = 0 | 1;

/** Joystick axes 0..63 (BASIC JOYSTK scale) plus the fire button. */
export async function setJoystick(
  port: JoystickPort,
  x: number,
  y: number,
  button: boolean
): Promise<void> {
  return invoke("machine_set_joystick", { port, x, y, button });
}

export interface CassetteState {
  loaded: boolean;
  name: string | null;
  /** Byte position within the tape image. */
  position: number;
  length: number;
  /** Cassette relay (PIA1 CA2). */
  motor: boolean;
  /** Bytes captured from CSAVE since the last take. */
  recorded_bytes: number;
}

/** Insert a .CAS tape image for CLOAD / CLOADM. */
export async function cassetteInsert(name: string, data: number[]): Promise<CassetteState> {
  return invoke("machine_cassette_insert", { name, data });
}

export async function cassetteEject(): Promise<CassetteState> {
  return invoke("machine_cassette_eject");
}

export async function cassetteRewind(): Promise<CassetteState> {
  return invoke("machine_cassette_rewind");
}

export async function cassetteState(): Promise<CassetteState | null> {
  return invoke("machine_cassette_state");
}

/** Take (and clear) the bytes written by CSAVE, as a .CAS image. */
export async function cassetteTakeRecording(): Promise<number[]> {
  return invoke("machine_cassette_take_recording");
}

/** Take (and clear) text sent to the printer (LLIST / PRINT #-2). */
export async function printerTakeOutput(): Promise<string> {
  return invoke("machine_printer_take_output");
}

export interface CartridgeState {
  loaded: boolean;
  name: string | null;
  size: number;
  /** CART* toggled by Q: BASIC autostarts the ROM at $C000. */
  autostart: boolean;
}

/** Insert a ROM cartridge at $C000 and cold-start the machine. */
export async function cartridgeInsert(
  name: string,
  data: number[],
  autostart: boolean
): Promise<CartridgeState> {
  return invoke("machine_cartridge_insert", { name, data, autostart });
}

export async function cartridgeEject(): Promise<CartridgeState> {
  return invoke("machine_cartridge_eject");
}

export async function cartridgeState(): Promise<CartridgeState | null> {
  return invoke("machine_cartridge_state");
}

/** Bare-metal 6821: drive a control line ("ca1" | "ca2" | "cb1" | "cb2") from the UI. */
export async function setPiaControlLine(
  line: "ca1" | "ca2" | "cb1" | "cb2",
  level: boolean
): Promise<void> {
  return invoke("set_pia_control_line_cmd", { line, level });
}
