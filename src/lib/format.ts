/**
 * Hex formatting helpers for the 6809 debugger surfaces.
 *
 * Addresses are 16-bit ($XXXX), data values are 8-bit ($XX).
 * All output is uppercase, zero-padded, Motorola-style with a `$` prefix,
 * matching the disassembler/assembler convention used throughout the app.
 */

/** Raw uppercase hex string, zero-padded to `width`, no prefix. */
export function toHex(value: number, width: number): string {
  return value.toString(16).toUpperCase().padStart(width, "0");
}

/** Format a 16-bit address as `$XXXX`. */
export function fmtAddr(address: number): string {
  return `$${toHex(address, 4)}`;
}

/** Format an 8-bit value as `$XX`. */
export function fmtByte(value: number): string {
  return `$${toHex(value, 2)}`;
}

/** Format a byte sequence as space-separated `XX XX XX` (no prefix). */
export function fmtBytes(bytes: number[]): string {
  return bytes.map((b) => toHex(b, 2)).join(" ");
}
