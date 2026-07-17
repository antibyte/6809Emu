/**
 * Centralized data for the ASM editor: mnemonics, directives, registers,
 * and instruction documentation for highlighting, autocomplete, and F1 help.
 *
 * Lists are derived from crates/m6809-asm (is_statement_keyword + encode) + examples + core.
 * Keep in sync when adding 6809/6309 support.
 */

export interface InstructionDoc {
  desc: string;
  syntax: string[];
  flags: string; // e.g. "N Z V C"
  cycles: string; // "6809: 2 | 6309: 2"
  notes?: string;
  variant?: "6809" | "6309" | "both";
}

export const DIRECTIVES = ["ORG", "FCB", "FDB", "RMB", "EQU", "SET", "END"] as const;

export const REGISTERS = [
  "A", "B", "D", "X", "Y", "U", "S", "PC", "DP", "CC",
  // 6309
  "W", "V", "E", "F", "MD", "Q",
] as const;

export const MNEMONICS = [
  // Base 6809 + common
  "NOP", "SYNC", "SWI", "RTS", "RTI", "ABX", "MUL", "SEX", "CWAI", "ORCC", "ANDCC", "DAA",
  "INX", "DEX", "INY", "DEY",
  "LDA", "LDB", "LDX", "LDY", "LDU", "LDD", "LDS",
  "STA", "STB", "STX", "STY", "STD", "STU",
  "ADDA", "ADDB", "ADDD", "SUBA", "SUBB", "SUBD",
  "CMPA", "CMPB", "CMPX", "CMPY", "CMPD", "CMPU", "CMPS",
  "ORA", "ORB", "ANDA", "ANDB", "EORA", "EORB",
  "ADCA", "ADCB", "SBCA", "SBCB", "BITA", "BITB",
  "BRA", "BRN", "BNE", "BEQ", "BCC", "BCS", "BPL", "BMI", "BVC", "BVS", "BGE", "BLT", "BGT", "BLE", "BSR",
  "LBRA", "LBRN", "LBNE", "LBEQ", "LBCC", "LBCS", "LBPL", "LBMI", "LBVC", "LBVS", "LBGE", "LBLT", "LBGT", "LBLE", "LBSR",
  "JMP", "JSR",
  "LEAX", "LEAY", "LEAS", "LEAU",
  "PSHS", "PULS", "PSHU", "PULU",
  "TFR", "EXG",
  "INC", "DEC", "NEG", "COM", "CLR", "TST",
  "LSR", "ROR", "ASR", "ASL", "ROL",
  // 6309 extensions
  "SEXW", "PSHSW", "PULSW", "PSHUW", "PULUW",
  "AIM", "OIM", "EIM", "TIM",
  "MULD", "ADDW", "SUBW", "CMPW", "LDW", "STW",
  "LDQ", "STQ",
  "LDMD", "BITMD",
  "INCW", "DECW", "CLRW", "TSTW",
  "DIVD", "DIVQ",
  "TFM", "TFM+", "TFM-", "TFM+R", "TFM+W",
  "ADDR", "ADCR", "SUBR", "SBCR", "ANDR", "ORR", "EORR", "CMPR",
  "BAND", "BIAND", "BOR", "BIOR", "BEOR", "BIEOR", "LDBT", "STBT",
] as const;

export const ALL_MNEMONICS = [...MNEMONICS] as string[];

export const INSTRUCTION_DOCS: Record<string, InstructionDoc> = {
  NOP: {
    desc: "No operation. Does nothing but advance PC and consume cycles.",
    syntax: ["NOP"],
    flags: "-",
    cycles: "6809: 2 | 6309: 1",
    notes: "Useful for timing or alignment.",
  },
  LDA: {
    desc: "Load accumulator A from memory or immediate.",
    syntax: ["LDA #imm", "LDA addr", "LDA addr,X", "LDA ,Y++"],
    flags: "N Z",
    cycles: "6809: 2-5 | 6309: 2-5 (varies by mode)",
    variant: "both",
  },
  STA: {
    desc: "Store accumulator A to memory.",
    syntax: ["STA addr", "STA addr,X"],
    flags: "N Z",
    cycles: "6809: 4-6",
    variant: "both",
  },
  BRA: {
    desc: "Branch always (unconditional short branch).",
    syntax: ["BRA label"],
    flags: "-",
    cycles: "3",
  },
  BEQ: {
    desc: "Branch if equal (Z=1).",
    syntax: ["BEQ label"],
    flags: "Z",
    cycles: "3 (taken) / 2 (not)",
  },
  JSR: {
    desc: "Jump to subroutine (pushes return address).",
    syntax: ["JSR addr", "JSR addr,X"],
    flags: "-",
    cycles: "6809: 8-9",
  },
  RTS: {
    desc: "Return from subroutine (pulls PC).",
    syntax: ["RTS"],
    flags: "-",
    cycles: "5",
  },
  LDX: {
    desc: "Load index register X.",
    syntax: ["LDX #imm", "LDX addr"],
    flags: "N Z",
    cycles: "3-6",
  },
  LEAX: {
    desc: "Load effective address into X (no memory access for simple offsets).",
    syntax: ["LEAX 5,X", "LEAX ,Y++"],
    flags: "- (Z only on 6309 for some forms)",
    cycles: "4-8",
    variant: "both",
  },
  PSHS: {
    desc: "Push registers onto hardware stack S.",
    syntax: ["PSHS A", "PSHS A,B,X,CC"],
    flags: "-",
    cycles: "varies (2 + #regs)",
  },
  TFR: {
    desc: "Transfer register to register (8<->8 or 16<->16).",
    syntax: ["TFR A,B", "TFR X,Y"],
    flags: "some transfers affect flags",
    cycles: "6-7 (6809)",
  },
  ORCC: {
    desc: "OR immediate value into condition code register (set flags).",
    syntax: ["ORCC #$50"],
    flags: "as specified",
    cycles: "3",
  },
  MUL: {
    desc: "Multiply A * B → D (unsigned).",
    syntax: ["MUL"],
    flags: "C (Z on 6309?)",
    cycles: "11",
  },
  MULD: {
    desc: "Multiply D * operand → Q (6309).",
    syntax: ["MULD #$1234", "MULD ,X"],
    flags: "C V N Z",
    cycles: "6309: 11-28",
    variant: "6309",
    notes: "Extended 6309 instruction. Result in W (high) + D? See docs.",
  },
  DIVD: {
    desc: "Divide D by operand (signed 16/8 → 16-bit quotient in W, remainder in D?)",
    syntax: ["DIVD #val"],
    flags: "C V N Z",
    cycles: "6309: ~25",
    variant: "6309",
  },
  TFM: {
    desc: "Block transfer (TFM+ / TFM- / TFM+R / TFM+W). Uses W as count.",
    syntax: ["TFM+ X+,Y+"],
    flags: "-",
    cycles: "6 + 3*count (chunked in emu)",
    variant: "6309",
    notes: "6309 only. Emulator implements in chunks for UI responsiveness.",
  },
  // Add more as needed (SEX, CLRA, INC, etc. can be expanded)
  SEX: {
    desc: "Sign extend B into A (A = $FF if B negative).",
    syntax: ["SEX"],
    flags: "N Z",
    cycles: "2",
  },
};

export function isMnemonic(token: string): boolean {
  const upper = token.toUpperCase();
  return (MNEMONICS as readonly string[]).includes(upper) || DIRECTIVES.includes(upper as any);
}

export function getInstructionDoc(mnemonic: string): InstructionDoc | undefined {
  const upper = mnemonic.toUpperCase();
  return INSTRUCTION_DOCS[upper];
}

/** Simple scan for defined labels in source (for completion). */
export function scanLabels(source: string): string[] {
  const labels = new Set<string>();
  const re = /^[ \t]*([A-Za-z_][\w]*):/gm;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source))) {
    labels.add(m[1]);
  }
  return Array.from(labels);
}
