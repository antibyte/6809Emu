import { EditorState, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { Compartment } from "@codemirror/state";
import { asmEditorTheme, asmHighlighting, darkModeExtension } from "./asm-theme";
import { asmHighlightExtension } from "./asm-highlight";
import type { Theme } from "../theme";

export interface AsmEditorOptions {
  parent: HTMLElement;
  doc: string;
  onChange: (value: string) => void;
  onAssemble?: () => void;
  theme?: Theme;
}

export interface AsmEditorHandle {
  view: EditorView;
  setTheme: (theme: Theme) => void;
}

export function createAsmEditor(options: AsmEditorOptions): AsmEditorHandle {
  const themeCompartment = new Compartment();
  const initialTheme = options.theme ?? "dark";

  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    history(),
    keymap.of([...defaultKeymap, ...historyKeymap]),
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
    EditorView.theme({
      ".cm-asm-mnemonic": { color: "var(--accent)", fontWeight: "600" },
      ".cm-asm-comment": { color: "var(--text-faint)", fontStyle: "italic" },
      ".cm-asm-number": { color: "var(--amber)" },
      ".cm-asm-label": { color: "var(--text)" },
    }),
    themeCompartment.of(darkModeExtension(initialTheme)),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        options.onChange(update.state.doc.toString());
      }
    }),
  );

  const state = EditorState.create({
    doc: options.doc,
    extensions,
  });

  const view = new EditorView({ state, parent: options.parent });

  return {
    view,
    setTheme(theme: Theme) {
      view.dispatch({ effects: themeCompartment.reconfigure(darkModeExtension(theme)) });
    },
  };
}

export function setAsmEditorDoc(view: EditorView, doc: string) {
  if (view.state.doc.toString() !== doc) {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: doc },
    });
  }
}
