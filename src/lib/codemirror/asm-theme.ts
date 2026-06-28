import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";

export const asmEditorTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--bg-deep)",
      color: "var(--text)",
      height: "100%",
    },
    ".cm-scroller": {
      fontFamily: "var(--font-mono)",
      fontSize: "13px",
      lineHeight: "1.6",
    },
    ".cm-gutters": {
      backgroundColor: "var(--bg-elevated)",
      color: "var(--text-dim)",
      borderRight: "1px solid var(--border)",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "rgba(57, 255, 20, 0.06)",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(57, 255, 20, 0.04)",
    },
    ".cm-cursor": {
      borderLeftColor: "var(--accent)",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      backgroundColor: "rgba(57, 255, 20, 0.15) !important",
    },
    ".cm-lintRange-error": {
      backgroundImage: "none",
      borderBottom: "2px wavy var(--danger)",
    },
    ".cm-diagnostic-error": {
      color: "var(--danger)",
    },
  },
  { dark: true }
);

const asmHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--accent)" },
  { tag: tags.comment, color: "var(--text-dim)", fontStyle: "italic" },
  { tag: tags.number, color: "var(--accent-amber)" },
  { tag: tags.string, color: "var(--accent-amber)" },
  { tag: tags.labelName, color: "var(--text)" },
]);

export const asmHighlighting = syntaxHighlighting(asmHighlight);