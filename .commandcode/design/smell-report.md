# Smell Report — 6809 Emulator

**Score**: 8/10 — FAINT
**Date**: 2026-06-26
**Target**: Full application surface

---

## TL;DR

The 6809 Emulator is a well-architected developer tool with solid interaction patterns and no AI-generated tells. The dominant smell is the **domain default trap** — its green-on-dark terminal aesthetic is the most predictable choice for a retro CPU emulator. Two faint issues detected, no identity failure.

---

## Heuristic Scores

| # | Heuristic | Score | Finding |
|---|-----------|-------|---------|
| 1 | Tech gradient | 1 — CLEAN | No blue-violet gradients. Only a subtle mono-tone gradient on primary buttons mixing 20% accent. |
| 2 | Generic tech hue | 0 — FAINT | `#39ff14` green-on-dark is the terminal/emulator domain default. Appropriate but predictable. |
| 3 | Feature tile grid | 1 — CLEAN | No marketing tiles. The UI is a functional debugger workspace with purpose-driven panels. |
| 4 | Accent rail | 1 — CLEAN | No colored stripe decorations pretending to organize. Division comes from grid layout. |
| 5 | Unearned blur | 0 — FAINT | Toolbar `backdrop-filter: blur(12px)` is the only blur. Provides separation from grid bg but isn't structurally necessary. |
| 6 | Stat monument | 1 — CLEAN | No oversized numbers. Register values are small, functional hex displays. |
| 7 | Icon topper | 1 — CLEAN | Panel headers use plain text. No decorative icons above headings. |
| 8 | Bounce everywhere | 1 — CLEAN | Motion is restrained to 0.1–0.2s `ease` transitions. No elastic or bouncy easing. |
| 9 | Default type | 1 — CLEAN | JetBrains Mono and Space Grotesk are intentional, project-specific choices. Not Inter/Consolas. |
| 10 | Center stack | 1 — CLEAN | Full IDE workspace with resizable split panels. Nothing center-stacked. |

**Score calculation**: 8 / 10 heuristics pass → 8/10

---

## Odor Log

### Domain Default Trap — Green Terminal Aesthetic
**Severity**: Faint
**Location**: All surfaces

The `#39ff14` neon green accent with text-shadow glow on dark background is the most obvious visual lane for a retro CPU emulator. It's functionally appropriate — green phosphor terminals, register highlights, and PC tracking all benefit from the association — but the palette could have made a more specific choice.

This is not a generated-AI tell. It's a domain reflex that was accepted rather than questioned.

### Unearned Blur — Toolbar Backdrop
**Severity**: Faint
**Location**: `.toolbar` in App.svelte

The toolbar uses `background: rgba(22, 29, 38, 0.85)` with `backdrop-filter: blur(12px)`. The grid background pattern is subtle enough that a solid panel background would have achieved the same separation. The blur adds visual complexity without serving a clear functional purpose here.

---

## What's Working

- **Three-theme system** (dark, light, high-contrast) with proper CSS custom property tokens. The high-contrast theme inverts borders to white and removes shadow ambiguity — real accessibility thinking.
- **Resizable split panels** with keyboard support (Arrow keys, Shift for 80px steps, pointer capture). The constraint system (`LAYOUT_LIMITS`) with min/max bounds shows deliberate UX engineering.
- **Svelte 5 runes** used consistently ($state, $derived, $effect). Modern architecture, not a template project.
- **i18n architecture** with English/German locale switching. Every UI string goes through `$t()`.
- **Contextual right-click menus** on disassembly lines with breakpoint, set PC, and run-to operations. Context-appropriate affordance.
- **ACIA serial terminal** with echo demo flow. Handles both running and paused states, with proper terminal state management.
- **Compact layout mode** (<1200px) with tab-based navigation for the bottom panels. Not just responsive — genuinely adapted for smaller viewports.
- **Custom SVG icon system** (7 icons: run, pause, step, reset, import, export, close). Purpose-built, not an icon library dependency.
- **Keyboard shortcut system** with F5 (run/pause), F10 (step), F9 (breakpoint toggle). Proper `registerShortcuts` abstraction.
- **Persistent layout state** in localStorage — splitter positions, trace depth, run speed, theme, memory follow-PC toggle all survive reload.

## Priority Issues

### P2 — No distinctive visual signature beyond "dark + green"
**Location**: Color palette system-wide

The app looks like a competent developer tool but doesn't have a visual identity beyond its genre. If you ran the same layout with a cyan accent, it would look like a different tool. If you ran it with a blue accent, it would look like a standard IDE. The palette doesn't encode anything specific to 6809 emulation.

**Fix**: Consider an accent color or secondary hue tied to the 6809 era — amber (VT100 terminals), or a dual-accent system (green for execution, amber for memory). The `accent-amber` already exists (`#ffb000`) for warnings — it could be elevated.

### P2 — Uniform panel dressing
**Location**: .panel / .panel-header / .panel-body pattern across all components

Every panel uses the identical structure: dark panel body, elevated header with uppercase label, `--radius` borders. This creates visual consistency but also visual flatness — nothing signals which panel is primary vs secondary.

**Fix**: Introduce a visual hierarchy for panels — register panel could have a subtly different border treatment or a small era-appropriate detail (a thin amber top-border line, or a distinct header color).

---

## Recommended Next Modes

- `/design voice` — To build a more distinctive visual lane beyond the terminal default
- `/design recolor` — To evolve the accent strategy from single green to a period-appropriate dual system
- `/design refine` — To push the panel hierarchy and add surface distinction
