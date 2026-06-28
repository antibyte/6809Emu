import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { asmEditorTheme } from "./asm-theme";
import { asmHighlightExtension } from "./asm-highlight";

export interface AsmEditorOptions {
  parent: HTMLElement;
  doc: string;
  onChange: (value: string) => void;
}

export function createAsmEditor(options: AsmEditorOptions): EditorView {
  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    history(),
    keymap.of([...defaultKeymap, ...historyKeymap]),
    asmEditorTheme,
    ...asmHighlightExtension(),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        options.onChange(update.state.doc.toString());
      }
    }),
    EditorView.theme({
      ".cm-asm-mnemonic": { color: "var(--accent)" },
      ".cm-asm-comment": { color: "var(--text-dim)", fontStyle: "italic" },
      ".cm-asm-number": { color: "var(--accent-amber)" },
      ".cm-asm-label": { color: "var(--text)" },
    }),
  ];

  const state = EditorState.create({
    doc: options.doc,
    extensions,
  });

  return new EditorView({ state, parent: options.parent });
}

export function setAsmEditorDoc(view: EditorView, doc: string) {
  if (view.state.doc.toString() !== doc) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: doc },
    });
  }
}