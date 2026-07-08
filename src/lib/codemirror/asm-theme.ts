import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import type { Theme } from "../theme";

/**
 * Base editor chrome, expressed entirely through CSS variables so it
 * follows the active app theme automatically. The `dark` base class
 * is toggled separately via `darkModeExtension` so CM's built-in
 * selection/caret defaults also adapt.
 */
export const asmEditorTheme = EditorView.theme({
  "&": {
    backgroundColor: "var(--bg-0)",
    color: "var(--text)",
    height: "100%",
  },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    fontSize: "13px",
    lineHeight: "1.55",
  },
  ".cm-gutters": {
    backgroundColor: "transparent",
    color: "var(--text-faint)",
    borderRight: "1px solid var(--border)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "var(--accent-soft)",
    color: "var(--text-dim)",
  },
  ".cm-activeLine": {
    backgroundColor: "color-mix(in srgb, var(--bg-2) 60%, transparent)",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--accent)",
    borderLeftWidth: "2px",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "var(--accent-soft) !important",
  },
  ".cm-lintRange-error": {
    backgroundImage: "none",
    borderBottom: "2px wavy var(--danger)",
  },
  ".cm-diagnostic-error": {
    color: "var(--danger)",
  },
});

const asmHighlight = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--accent)" },
  { tag: tags.comment, color: "var(--text-faint)", fontStyle: "italic" },
  { tag: tags.number, color: "var(--amber)" },
  { tag: tags.string, color: "var(--amber)" },
  { tag: tags.labelName, color: "var(--text)" },
  { tag: tags.variableName, color: "var(--text)" },
  { tag: tags.operator, color: "var(--text-dim)" },
]);

export const asmHighlighting = syntaxHighlighting(asmHighlight);

/**
 * Empty theme spec whose only job is to set CodeMirror's dark/light
 * base class. Reconfigure the holding compartment on theme change.
 */
export function darkModeExtension(theme: Theme) {
  return EditorView.theme({}, { dark: theme !== "light" });
}
