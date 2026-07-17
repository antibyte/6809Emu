import { writable, type Writable } from "svelte/store";

/**
 * Central layout state for the workspace shell.
 * Owns panel visibility, persistent sizes, collapsed sidebar
 * sections and which dock panels are open. Everything is mirrored
 * to localStorage so the user's arrangement survives reloads.
 */

export type PanelId =
  | "registers"
  | "io"
  | "breakpoints"
  | "watchpoints"
  | "disasm"
  | "asm"
  | "memory"
  | "trace"
  | "terminal"
  | "video"
  | "pia"
  | "ay";

export type SidebarSection =
  | "registers"
  | "io"
  | "breakpoints"
  | "watchpoints"
  | "pia"
  | "ay";

export interface LayoutSizes {
  mainPct: number;
  sidebarPx: number;
  disasmPct: number;
  /** bottom row: memory / trace / terminal share, in percent (0..100) */
  bottom: Record<"memory" | "trace" | "terminal", number>;
  /** sidebar vertical shares for the four sections, in percent */
  sidebarRows: Record<SidebarSection, number>;
  /** video dock width in px when docked right */
  videoPx: number;
}

export interface LayoutState {
  visible: Record<PanelId, boolean>;
  collapsed: Record<SidebarSection, boolean>;
  videoDocked: boolean;
  sizes: LayoutSizes;
}

const STORAGE_KEY = "layoutState.v3";

const DEFAULT: LayoutState = {
  visible: {
    registers: true,
    io: true,
    breakpoints: true,
    watchpoints: true,
    disasm: true,
    asm: true,
    memory: true,
    trace: true,
    terminal: true,
    video: false,
    pia: false,
    ay: false,
  },
  collapsed: {
    registers: false,
    io: false,
    breakpoints: false,
    watchpoints: false,
    pia: false,
    ay: false,
  },
  videoDocked: true,
  sizes: {
    mainPct: 62,
    sidebarPx: 290,
    disasmPct: 52,
    bottom: { memory: 40, trace: 34, terminal: 26 },
    sidebarRows: { registers: 34, io: 22, breakpoints: 22, watchpoints: 22, pia: 28, ay: 28 },
    videoPx: 360,
  },
};

export function defaultLayoutState(): LayoutState {
  return structuredClone(DEFAULT);
}

function clamp(v: number, min: number, max: number) {
  return Math.min(max, Math.max(min, v));
}

function sanitize(raw: Partial<LayoutState> | null): LayoutState {
  if (!raw) return structuredClone(DEFAULT);
  const s = structuredClone(DEFAULT);
  if (raw.visible) {
    for (const k of Object.keys(s.visible) as PanelId[]) {
      if (typeof raw.visible[k] === "boolean") s.visible[k] = raw.visible[k]!;
    }
  }
  if (raw.collapsed) {
    for (const k of Object.keys(s.collapsed) as SidebarSection[]) {
      if (typeof raw.collapsed[k] === "boolean") s.collapsed[k] = raw.collapsed[k]!;
    }
  }
  if (typeof raw.videoDocked === "boolean") s.videoDocked = raw.videoDocked;
  if (raw.sizes) {
    const sz = raw.sizes;
    if (typeof sz.mainPct === "number") s.sizes.mainPct = clamp(sz.mainPct, 38, 78);
    if (typeof sz.sidebarPx === "number") s.sizes.sidebarPx = clamp(sz.sidebarPx, 240, 460);
    if (typeof sz.disasmPct === "number") s.sizes.disasmPct = clamp(sz.disasmPct, 30, 70);
    if (sz.bottom) {
      for (const k of ["memory", "trace", "terminal"] as const) {
        if (typeof sz.bottom[k] === "number") s.sizes.bottom[k] = clamp(sz.bottom[k], 6, 80);
      }
    }
    if (sz.sidebarRows) {
      for (const k of ["registers", "io", "breakpoints", "watchpoints", "pia", "ay"] as const) {
        if (typeof sz.sidebarRows[k] === "number") s.sizes.sidebarRows[k] = clamp(sz.sidebarRows[k], 6, 80);
      }
    }
    if (typeof sz.videoPx === "number") s.sizes.videoPx = clamp(sz.videoPx, 240, 640);
  }
  return s;
}

function migrateFromV2(raw: Record<string, unknown>): Partial<LayoutState> {
  const r = structuredClone(raw) as Partial<LayoutState> & {
    collapsed?: Record<string, boolean>;
    sizes?: { sidebarRows?: Record<string, number> };
  };
  const collapsed = r.collapsed as Record<string, boolean> | undefined;
  if (collapsed && typeof collapsed.debug === "boolean") {
    collapsed.breakpoints = collapsed.debug;
    collapsed.watchpoints = collapsed.debug;
    delete collapsed.debug;
  }
  const rows = r.sizes?.sidebarRows as Record<string, number> | undefined;
  if (rows && typeof rows.debug === "number") {
    const half = rows.debug / 2;
    rows.breakpoints = half;
    rows.watchpoints = half;
    delete rows.debug;
  }
  return r;
}

function load(): LayoutState {
  if (typeof localStorage === "undefined") return structuredClone(DEFAULT);
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return sanitize(JSON.parse(raw));

    const legacy = localStorage.getItem("layoutState.v2");
    if (legacy) {
      const migrated = migrateFromV2(JSON.parse(legacy));
      const state = sanitize(migrated);
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
      return state;
    }
    return structuredClone(DEFAULT);
  } catch {
    return structuredClone(DEFAULT);
  }
}

export const layout: Writable<LayoutState> = writable<LayoutState>(load());

layout.subscribe((state) => {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    /* ignore quota errors */
  }
});

export function togglePanel(id: PanelId) {
  layout.update((s) => ({ ...s, visible: { ...s.visible, [id]: !s.visible[id] } }));
}

export function setPanel(id: PanelId, value: boolean) {
  layout.update((s) => ({ ...s, visible: { ...s.visible, [id]: value } }));
}

export function toggleCollapsed(section: SidebarSection) {
  layout.update((s) => ({ ...s, collapsed: { ...s.collapsed, [section]: !s.collapsed[section] } }));
}

export function setVideoDocked(docked: boolean) {
  layout.update((s) => ({ ...s, videoDocked: docked }));
}

export function patchSizes(patch: Partial<LayoutSizes>) {
  layout.update((s) => ({ ...s, sizes: { ...s.sizes, ...patch } }));
}

/**
 * Redistribute size between two adjacent items of a percentage group
 * (e.g. bottom panels or sidebar rows) based on a pixel delta.
 * `totalPx` is the current pixel size of the whole group.
 */
export function redistribute(
  group: "bottom" | "sidebarRows",
  aKey: string,
  bKey: string,
  deltaPct: number,
  min = 8,
  max = 80,
) {
  layout.update((s) => {
    const map = { ...s.sizes[group] } as Record<string, number>;
    const a = map[aKey] ?? 0;
    const b = map[bKey] ?? 0;
    let na = a + deltaPct;
    let nb = b - deltaPct;
    if (na < min) {
      nb -= min - na;
      na = min;
    }
    if (nb < min) {
      na -= min - nb;
      nb = min;
    }
    if (na > max) {
      nb += na - max;
      na = max;
    }
    map[aKey] = na;
    map[bKey] = nb;
    return {
      ...s,
      sizes: { ...s.sizes, [group]: map } as LayoutSizes,
    };
  });
}

/** Convenience: read a snapshot without subscribing. */
export function getLayout(): LayoutState {
  let value: LayoutState = structuredClone(DEFAULT);
  const unsub = layout.subscribe((v) => (value = v));
  unsub();
  return value;
}
