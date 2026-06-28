import { Decoration, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { RangeSetBuilder } from "@codemirror/state";

const MNEMONICS =
  "NOP|SYNC|SWI|RTS|RTI|ABX|MUL|SEX|CWAI|ORCC|ANDCC|DAA|INX|DEX|INY|DEY|LBRA|LBRN|" +
  "LDA|LDB|LDX|LDY|LDU|LDD|LDS|STA|STB|STX|STY|STD|STU|" +
  "ADDA|ADDB|ADDD|SUBA|SUBB|SUBD|CMPA|CMPB|CMPX|CMPY|CMPD|CMPU|CMPS|" +
  "ORA|ORB|ANDA|ANDB|EORA|EORB|ADCA|ADCB|SBCA|SBCB|BITA|BITB|" +
  "BRA|BRN|BNE|BEQ|BCC|BCS|BPL|BMI|BVC|BVS|BGE|BLT|BGT|BLE|BSR|" +
  "JMP|JSR|LEA|PSH|PUL|TFR|EXG|INC|DEC|NEG|COM|LSR|ROR|ASR|ASL|ROL|" +
  "CLR|TST|JMP|ORG|FCB|FDB|RMB|EQU|SET|END";

const mnemonicDeco = Decoration.mark({ class: "cm-asm-mnemonic" });
const commentDeco = Decoration.mark({ class: "cm-asm-comment" });
const numberDeco = Decoration.mark({ class: "cm-asm-number" });
const labelDeco = Decoration.mark({ class: "cm-asm-label" });

const mnemonicRe = new RegExp(`\\b(${MNEMONICS})\\b`, "gi");
const commentRe = /;[^\n]*/g;
const numberRe = /\$[0-9A-Fa-f]+|%[01]+|@[0-7]+|\b\d+\b/g;
const labelRe = /^[ \t]*([A-Za-z_][\w]*):/gm;

function buildDecorations(text: string): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();

  for (const match of text.matchAll(commentRe)) {
    const from = match.index!;
    builder.add(from, from + match[0].length, commentDeco);
  }

  for (const match of text.matchAll(labelRe)) {
    const from = match.index! + match[0].indexOf(match[1]);
    builder.add(from, from + match[1].length, labelDeco);
  }

  for (const match of text.matchAll(numberRe)) {
    const from = match.index!;
    if (!isInsideComment(text, from)) {
      builder.add(from, from + match[0].length, numberDeco);
    }
  }

  for (const match of text.matchAll(mnemonicRe)) {
    const from = match.index!;
    if (!isInsideComment(text, from)) {
      builder.add(from, from + match[0].length, mnemonicDeco);
    }
  }

  return builder.finish();
}

function isInsideComment(text: string, pos: number): boolean {
  const lineStart = text.lastIndexOf("\n", pos - 1) + 1;
  const semi = text.indexOf(";", lineStart);
  return semi !== -1 && semi < pos;
}

const asmHighlightPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: import("@codemirror/view").EditorView) {
      this.decorations = buildDecorations(view.state.doc.toString());
    }

    update(update: ViewUpdate) {
      if (update.docChanged) {
        this.decorations = buildDecorations(update.state.doc.toString());
      }
    }
  },
  { decorations: (v) => v.decorations }
);

export function asmHighlightExtension() {
  return [asmHighlightPlugin];
}