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
  // MC6809 inherent / control
  "NOP", "SYNC", "SWI", "SWI2", "SWI3", "RTS", "RTI", "ABX", "MUL", "SEX", "CWAI", "ORCC", "ANDCC", "DAA",
  "INX", "DEX", "INY", "DEY",
  // Loads / stores
  "LDA", "LDB", "LDX", "LDY", "LDU", "LDD", "LDS",
  "STA", "STB", "STX", "STY", "STD", "STU", "STS",
  // 8/16-bit arithmetic and logic
  "ADDA", "ADDB", "ADDD", "SUBA", "SUBB", "SUBD",
  "CMPA", "CMPB", "CMPX", "CMPY", "CMPD", "CMPU", "CMPS",
  "ORA", "ORB", "ANDA", "ANDB", "EORA", "EORB",
  "ADCA", "ADCB", "SBCA", "SBCB", "BITA", "BITB",
  // Branches
  "BRA", "BRN", "BHI", "BLS", "BHS", "BLO", "BCC", "BCS", "BNE", "BEQ", "BVC", "BVS",
  "BPL", "BMI", "BGE", "BLT", "BGT", "BLE", "BSR",
  "LBRA", "LBRN", "LBHI", "LBLS", "LBHS", "LBLO", "LBCC", "LBCS", "LBNE", "LBEQ", "LBVC", "LBVS",
  "LBPL", "LBMI", "LBGE", "LBLT", "LBGT", "LBLE", "LBSR",
  "JMP", "JSR",
  "LEAX", "LEAY", "LEAS", "LEAU",
  "PSHS", "PULS", "PSHU", "PULU",
  "TFR", "EXG",
  // Read-modify-write (memory, A, B)
  "NEG", "COM", "LSR", "ROR", "ASR", "ASL", "LSL", "ROL", "DEC", "INC", "TST", "CLR",
  "NEGA", "COMA", "LSRA", "RORA", "ASRA", "ASLA", "LSLA", "ROLA", "DECA", "INCA", "TSTA", "CLRA",
  "NEGB", "COMB", "LSRB", "RORB", "ASRB", "ASLB", "LSLB", "ROLB", "DECB", "INCB", "TSTB", "CLRB",
  // HD6309 extensions
  "SEXW", "PSHSW", "PULSW", "PSHUW", "PULUW",
  "AIM", "OIM", "EIM", "TIM",
  "NEGD", "COMD", "LSRD", "RORD", "ASRD", "ASLD", "LSLD", "ROLD", "DECD", "INCD", "TSTD", "CLRD",
  "COMW", "LSRW", "RORW", "ROLW", "DECW", "INCW", "TSTW", "CLRW",
  "COME", "DECE", "INCE", "TSTE", "CLRE",
  "COMF", "DECF", "INCF", "TSTF", "CLRF",
  "SUBW", "CMPW", "SBCD", "ANDD", "BITD", "LDW", "STW", "EORD", "ADCD", "ORD", "ADDW",
  "SUBE", "CMPE", "LDE", "STE", "ADDE",
  "SUBF", "CMPF", "LDF", "STF", "ADDF",
  "LDQ", "STQ", "MULD", "DIVD", "DIVQ",
  "LDMD", "BITMD",
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
    cycles: "6809: 2 | 6309 native: 1",
    notes: "Useful for timing or alignment.",
  },
  LDA: {
    desc: "Load accumulator A from memory or immediate.",
    syntax: ["LDA #imm", "LDA <dp", "LDA addr", "LDA 5,X", "LDA ,Y++", "LDA [label,PCR]"],
    flags: "N Z V=0",
    cycles: "2 (#) / 4 (dir) / 4+ (idx) / 5 (ext)",
    variant: "both",
    notes: "Indexed extras: ,R +0; n5,R / n8,R / A,R / B,R +1; ,R+ +2; ,R++ +3; n16,R / D,R +4; n16,PCR +5; indirect +3.",
  },
  STA: {
    desc: "Store accumulator A to memory.",
    syntax: ["STA <dp", "STA addr", "STA addr,X"],
    flags: "N Z V=0",
    cycles: "4 (dir) / 4+ (idx) / 5 (ext)",
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
    flags: "-",
    cycles: "3 (taken or not); LBEQ 5 / 6 taken",
  },
  JSR: {
    desc: "Jump to subroutine (pushes return address).",
    syntax: ["JSR <dp", "JSR addr", "JSR addr,X"],
    flags: "-",
    cycles: "7 (dir) / 7+ (idx) / 8 (ext)",
  },
  RTS: {
    desc: "Return from subroutine (pulls PC).",
    syntax: ["RTS"],
    flags: "-",
    cycles: "5 | 6309 native: 4",
  },
  SWI: {
    desc: "Software interrupt: stacks the entire state, sets I and F, vectors via $FFFA.",
    syntax: ["SWI", "SWI2", "SWI3"],
    flags: "E=1, I=F=1 (SWI only; SWI2/SWI3 leave I and F unchanged)",
    cycles: "SWI 19, SWI2/SWI3 20 | 6309 native +2",
    notes: "SWI2 vectors via $FFF4, SWI3 via $FFF2.",
  },
  CWAI: {
    desc: "AND the mask into CC, stack the entire state, wait for an interrupt the new masks allow.",
    syntax: ["CWAI #$EF"],
    flags: "as masked",
    cycles: "20 (up to the vector fetch)",
  },
  SYNC: {
    desc: "Wait until an interrupt line is asserted. A masked interrupt resumes with the next instruction.",
    syntax: ["SYNC"],
    flags: "-",
    cycles: ">= 4",
  },
  LDX: {
    desc: "Load index register X.",
    syntax: ["LDX #imm", "LDX addr"],
    flags: "N Z V=0",
    cycles: "3 (#) / 5 (dir) / 5+ (idx) / 6 (ext)",
  },
  LEAX: {
    desc: "Load effective address into X (no memory access for simple offsets).",
    syntax: ["LEAX 5,X", "LEAX ,Y++", "LEAX label,PCR"],
    flags: "Z (LEAX/LEAY only; LEAS/LEAU affect no flags)",
    cycles: "4+",
    variant: "both",
  },
  PSHS: {
    desc: "Push registers onto hardware stack S.",
    syntax: ["PSHS A", "PSHS A,B,X,CC"],
    flags: "-",
    cycles: "5 + 1 per byte pushed",
  },
  TFR: {
    desc: "Transfer register to register.",
    syntax: ["TFR A,B", "TFR X,Y", "TFR A,CC"],
    flags: "- (TFR r,CC loads CC)",
    cycles: "6 | 6309 native: 4",
    notes: "Mixed sizes on the 6809: 8→16 gives $FF:r (A/B) or r:r (CC/DP); 16→8 takes the low byte.",
  },
  ORCC: {
    desc: "OR immediate value into condition code register (set flags).",
    syntax: ["ORCC #$50"],
    flags: "as specified",
    cycles: "3",
  },
  MUL: {
    desc: "Multiply A × B → D (unsigned).",
    syntax: ["MUL"],
    flags: "Z C (C = bit 7 of B); N, V unchanged",
    cycles: "11 | 6309 native: 10",
  },
  DAA: {
    desc: "Decimal adjust A after an ADD/ADC of BCD values.",
    syntax: ["DAA"],
    flags: "N Z C (H unchanged)",
    cycles: "2",
  },
  SEX: {
    desc: "Sign extend B into A (A = $FF if B negative).",
    syntax: ["SEX"],
    flags: "N Z",
    cycles: "2 | 6309 native: 1",
  },
  MULD: {
    desc: "Q = D × operand (signed 16 × 16 → 32 bit, 6309).",
    syntax: ["MULD #$1234", "MULD <dp", "MULD ,X", "MULD addr"],
    flags: "N (bit 31) Z (upper 16 bits); V, C unchanged",
    cycles: "28 (#) / 30 (dir) / 30+ (idx) / 31 (ext); +2 if an operand is negative",
    variant: "6309",
  },
  DIVD: {
    desc: "Signed D ÷ 8-bit operand → B = quotient, A = remainder (6309).",
    syntax: ["DIVD #9", "DIVD <dp", "DIVD ,X", "DIVD addr"],
    flags: "N Z V C (C = quotient odd)",
    cycles: "25 (#) / 27 (dir) / 27+ (idx) / 28 (ext); varies with signs and overflow",
    variant: "6309",
    notes: "Division by zero traps through $FFF0 and sets MD bit 7.",
  },
  DIVQ: {
    desc: "Signed Q ÷ 16-bit operand → W = quotient, D = remainder (6309).",
    syntax: ["DIVQ #1000", "DIVQ <dp", "DIVQ ,X", "DIVQ addr"],
    flags: "N Z V C (C = quotient odd)",
    cycles: "34 (#) / 36 (dir) / 36+ (idx) / 37 (ext); varies with signs and overflow",
    variant: "6309",
    notes: "Division by zero traps through $FFF0 and sets MD bit 7.",
  },
  TFM: {
    desc: "Block transfer of W bytes between D, X, Y, U, S (6309).",
    syntax: ["TFM X+,Y+", "TFM X-,Y-", "TFM X+,Y", "TFM X,Y+"],
    flags: "Z = (W == 0)",
    cycles: "6 + 3 per byte",
    variant: "6309",
    notes: "Interruptible: the stacked PC is the TFM itself, RTI resumes with the remaining count in W.",
  },
  AIM: {
    desc: "AND immediate with memory (OIM: OR, EIM: EOR, TIM: test only) (6309).",
    syntax: ["AIM #$F0,<$20", "OIM #$01,$1234", "EIM #$FF,5,X", "TIM #$80,<$20"],
    flags: "N Z V=0 (C unchanged)",
    cycles: "6 (dir) / 7+ (idx) / 7 (ext); TIM 4 / 5+ / 5",
    variant: "6309",
  },
  ADDR: {
    desc: "Register-to-register: dst = dst + src (ADCR, SUBR, SBCR, ANDR, ORR, EORR, CMPR alike) (6309).",
    syntax: ["ADDR X,Y", "SUBR A,B", "CMPR W,D"],
    flags: "as the corresponding memory instruction",
    cycles: "4",
    variant: "6309",
  },
  BAND: {
    desc: "Bit operation between a direct-page memory bit and a register bit (BIAND, BOR, BIOR, BEOR, BIEOR, LDBT, STBT alike) (6309).",
    syntax: ["BAND A,srcbit,dstbit,<$40", "LDBT CC,7,0,<$40", "STBT B,0,7,<$40"],
    flags: "- (except when the register is CC)",
    cycles: "7 | STBT 8",
    variant: "6309",
    notes: "For BAND..LDBT the source bit is in memory and the result goes into the register bit; STBT stores a register bit into memory.",
  },
  LDMD: {
    desc: "Load the mode register: bit 0 = native mode, bit 1 = FIRQ saves the entire state (6309).",
    syntax: ["LDMD #$01"],
    flags: "-",
    cycles: "5",
    variant: "6309",
  },
  BITMD: {
    desc: "Test and clear the MD error flags (bit 7 divide by zero, bit 6 illegal instruction) (6309).",
    syntax: ["BITMD #$C0"],
    flags: "Z",
    cycles: "4",
    variant: "6309",
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
