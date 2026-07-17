import { EditorState, StateField, StateEffect, type Extension, RangeSetBuilder } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  gutter,
  GutterMarker,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { Compartment } from "@codemirror/state";
import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
import { asmEditorTheme, asmHighlighting, darkModeExtension } from "./asm-theme";
import { asmHighlightExtension } from "./asm-highlight";
import { MNEMONICS, DIRECTIVES, REGISTERS, scanLabels, isMnemonic } from "../asm";
import type { Theme } from "../theme";

export interface AsmEditorOptions {
  parent: HTMLElement;
  doc: string;
  onChange: (value: string) => void;
  onAssemble?: () => void;
  onHelpMnemonic?: (mnemonic: string) => void;
  /** Called when the user clicks the breakpoint gutter on a source line. */
  onToggleBreakpointLine?: (line: number) => void;
  /** Returns whether the given 1-based source line maps to an address. */
  hasAddress?: (line: number) => boolean;
  theme?: Theme;
}

export interface AsmEditorHandle {
  view: EditorView;
  setTheme: (theme: Theme) => void;
  /** Reactive handlers kept up to date by the Svelte wrapper. */
  handlers: {
    onToggleBreakpointLine?: (line: number) => void;
    hasAddress?: (line: number) => boolean;
  };
}

/** State effect used to update the set of breakpoint lines from outside. */
const setBreakpointLines = StateEffect.define<Set<number>>();

/** State field holding the set of 1-based source line numbers with a breakpoint. */
const breakpointLineField = StateField.define<Set<number>>({
  create() {
    return new Set();
  },
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setBreakpointLines)) {
        return effect.value;
      }
    }
    return value;
  },
});

class BreakpointMarker extends GutterMarker {
  toDOM() {
    const dot = document.createElement("span");
    dot.className = "cm-bp-dot";
    dot.textContent = "\u25CF";
    return dot;
  }
}

class BreakpointEmptyMarker extends GutterMarker {
  toDOM() {
    const dot = document.createElement("span");
    dot.className = "cm-bp-dot cm-bp-empty";
    dot.textContent = "\u25CB";
    return dot;
  }
}

const breakpointGutter = (handle: { handlers: AsmEditorHandle["handlers"] }) =>
  gutter({
    class: "cm-breakpoint-gutter",
    markers(view) {
      const builder = new RangeSetBuilder<GutterMarker>();
      const bps = view.state.field(breakpointLineField);
      const hasAddress = handle.handlers.hasAddress;
      const lines = view.state.doc.lines;
      for (let i = 1; i <= lines; i++) {
        const line = view.state.doc.line(i);
        const srcLine = line.number;
        let marker: GutterMarker | null = null;
        if (bps.has(srcLine)) {
          marker = new BreakpointMarker();
        } else if (hasAddress && hasAddress(srcLine)) {
          marker = new BreakpointEmptyMarker();
        }
        if (marker) {
          builder.add(line.from, line.from, marker);
        }
      }
      return builder.finish();
    },
    domEventHandlers: {
      mousedown(view, line) {
        const srcLine = view.state.doc.lineAt(line.from).number;
        const hasAddress = handle.handlers.hasAddress;
        if (hasAddress && !hasAddress(srcLine)) return false;
        handle.handlers.onToggleBreakpointLine?.(srcLine);
        return true;
      },
    },
  });

export function updateBreakpoints(view: EditorView, lines: Set<number>) {
  view.dispatch({ effects: setBreakpointLines.of(new Set(lines)) });
}

function asmCompletions(context: CompletionContext) {
  const word = context.matchBefore(/\w+/);
  if (!word) return null;

  const line = context.state.doc.lineAt(context.pos);
  const before = line.text.slice(0, word.from - line.from).trim().toUpperCase();
  const isStart = before === "" || /^[A-Z_][\w]*:$/.test(before);

  let options: any[] = [];

  if (isStart) {
    // Offer mnemonics + directives at start of instruction
    options = [
      ...MNEMONICS.map((m) => ({ label: m, type: "keyword", info: "6809/6309 mnemonic" })),
      ...DIRECTIVES.map((d) => ({ label: d, type: "keyword", info: "assembler directive" })),
    ];
  } else {
    // After mnemonic: registers, common literals
    options = REGISTERS.map((r) => ({ label: r, type: "variable", info: "register" }));
    // Could add more like # $ , etc but keep simple
  }

  // Always offer labels found in current doc
  const labels = scanLabels(context.state.doc.toString());
  options = [
    ...options,
    ...labels.map((l) => ({ label: l, type: "constant", info: "label" })),
  ];

  return {
    from: word.from,
    options,
    validFor: /^\w*$/,
  };
}

export function createAsmEditor(options: AsmEditorOptions): AsmEditorHandle {
  const themeCompartment = new Compartment();
  const initialTheme = options.theme ?? "dark";

  const handle: AsmEditorHandle = {
    view: undefined as unknown as EditorView,
    setTheme: () => {},
    handlers: {
      onToggleBreakpointLine: options.onToggleBreakpointLine,
      hasAddress: options.hasAddress,
    },
  };

  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    history(),
    keymap.of([...defaultKeymap, ...historyKeymap]),
    breakpointLineField,
    breakpointGutter(handle),
  ];

  if (options.onAssemble) {
    extensions.push(
      keymap.of([
        {
          key: "Ctrl-Enter",
          mac: "Cmd-Enter",
          run: () => {
            options.onAssemble?.();
            return true;
          },
        },
      ]),
    );
  }

  extensions.push(
    asmEditorTheme,
    ...asmHighlightExtension(),
    autocompletion({ override: [asmCompletions] }),
    EditorView.theme({
      ".cm-asm-mnemonic": { color: "var(--accent)", fontWeight: "600" },
      ".cm-asm-comment": { color: "var(--text-faint)", fontStyle: "italic" },
      ".cm-asm-number": { color: "var(--amber)" },
      ".cm-asm-label": { color: "var(--text)" },
      ".cm-breakpoint-gutter": { width: "1.2em" },
      ".cm-breakpoint-gutter .cm-bp-dot": {
        color: "var(--danger)",
        cursor: "pointer",
        fontSize: "11px",
        lineHeight: "1",
      },
      ".cm-breakpoint-gutter .cm-bp-empty": {
        color: "var(--text-faint)",
        opacity: "0.35",
      },
      ".cm-breakpoint-gutter .cm-bp-empty:hover": {
        color: "var(--danger)",
        opacity: "1",
      },
    }),
    themeCompartment.of(darkModeExtension(initialTheme)),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        options.onChange(update.state.doc.toString());
      }
    }),
  );

  if (options.onHelpMnemonic) {
    extensions.push(
      keymap.of([
        {
          key: "F1",
          run: (view) => {
            const sel = view.state.selection.main.head;
            const word = view.state.wordAt(sel);
            if (word) {
              const token = view.state.doc.sliceString(word.from, word.to).toUpperCase().trim();
              if (isMnemonic(token)) {
                options.onHelpMnemonic!(token);
                return true;
              }
            }
            return false;
          },
        },
      ]),
    );
  }

  const state = EditorState.create({
    doc: options.doc,
    extensions,
  });

  const view = new EditorView({ state, parent: options.parent });
  handle.view = view;
  handle.setTheme = (theme: Theme) => {
    view.dispatch({ effects: themeCompartment.reconfigure(darkModeExtension(theme)) });
  };

  return handle;
}

export function setAsmEditorDoc(view: EditorView, doc: string) {
  if (view.state.doc.toString() !== doc) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: doc },
    });
  }
}
