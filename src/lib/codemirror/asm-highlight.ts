import { Decoration, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { RangeSetBuilder } from "@codemirror/state";
import { MNEMONICS, DIRECTIVES } from "../asm";

const ALL_KEYWORDS = [...MNEMONICS, ...DIRECTIVES];

const mnemonicDeco = Decoration.mark({ class: "cm-asm-mnemonic" });
const commentDeco = Decoration.mark({ class: "cm-asm-comment" });
const numberDeco = Decoration.mark({ class: "cm-asm-number" });
const labelDeco = Decoration.mark({ class: "cm-asm-label" });

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// Longer tokens first so e.g. TFM+ wins over TFM in alternation.
const sortedKeywords = [...ALL_KEYWORDS].sort((a, b) => b.length - a.length);
const mnemonicRe = new RegExp(
  `\\b(${sortedKeywords.map(escapeRegex).join("|")})\\b`,
  "gi",
);
const commentRe = /;[^\n]*/g;
const numberRe = /\$[0-9A-Fa-f]+|%[01]+|@[0-7]+|\b\d+\b/g;
const labelRe = /^[ \t]*([A-Za-z_][\w]*):/gm;

interface HighlightSpan {
  from: number;
  to: number;
  deco: Decoration;
}

function buildDecorations(text: string): DecorationSet {
  const spans: HighlightSpan[] = [];

  for (const match of text.matchAll(commentRe)) {
    const from = match.index!;
    spans.push({ from, to: from + match[0].length, deco: commentDeco });
  }

  for (const match of text.matchAll(labelRe)) {
    const from = match.index! + match[0].indexOf(match[1]);
    spans.push({ from, to: from + match[1].length, deco: labelDeco });
  }

  for (const match of text.matchAll(numberRe)) {
    const from = match.index!;
    if (!isInsideComment(text, from)) {
      spans.push({ from, to: from + match[0].length, deco: numberDeco });
    }
  }

  for (const match of text.matchAll(mnemonicRe)) {
    const from = match.index!;
    if (!isInsideComment(text, from)) {
      spans.push({ from, to: from + match[0].length, deco: mnemonicDeco });
    }
  }

  spans.sort((a, b) => a.from - b.from || a.to - b.to);

  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to, deco } of spans) {
    builder.add(from, to, deco);
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