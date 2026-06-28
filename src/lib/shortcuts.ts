export interface Shortcut {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  handler: () => void;
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable;
}

export function registerShortcuts(shortcuts: Shortcut[]): () => void {
  function onKeyDown(e: KeyboardEvent) {
    if (isEditableTarget(e.target)) return;

    for (const s of shortcuts) {
      const ctrl = e.ctrlKey || e.metaKey;
      if (
        e.key === s.key &&
        !!s.ctrl === ctrl &&
        !!s.shift === e.shiftKey &&
        !!s.alt === e.altKey
      ) {
        e.preventDefault();
        s.handler();
        return;
      }
    }
  }

  window.addEventListener("keydown", onKeyDown);
  return () => window.removeEventListener("keydown", onKeyDown);
}