<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fmtAddr } from "../format";
  import type { VideoFrame } from "../types";

  let {
    frame,
    keyboardEnabled = false,
    firmwareLabel = "",
    onGoto,
    onFullscreen,
    onClose,
    onKey,
  }: {
    frame: VideoFrame | null;
    keyboardEnabled?: boolean;
    firmwareLabel?: string;
    onGoto: (addr: number) => void;
    onFullscreen: () => void;
    onClose: () => void;
    onKey?: (code: string, down: boolean) => void;
  } = $props();

  let bodyEl: HTMLDivElement | undefined = $state();
  let focused = $state(false);
  /** Pixel font-size fitted so the full cols×rows grid is visible. */
  let fontPx = $state(12);

  function handleKey(e: KeyboardEvent, down: boolean) {
    if (!keyboardEnabled || !onKey || !focused) return;
    // Keep browser shortcuts with modifiers for the host UI.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    // Ignore auto-repeat for modifiers we don't need; allow for letters/backspace.
    e.preventDefault();
    e.stopPropagation();
    onKey(e.code, down);
  }

  function modeLabel(mode: string): string {
    return mode
      .replace(/([A-Za-z])(\d)/g, "$1 $2")
      .replace(/(\d)x(\d)/gi, "$1×$2");
  }

  const displayRows = $derived(
    frame && frame.rows_text.length > 0
      ? frame.rows_text
      : frame
        ? Array.from({ length: frame.rows }, (_, row) =>
            frame.cells
              .slice(row * frame.cols, (row + 1) * frame.cols)
              .map((cell) => {
                if (cell & 0x80) return "\u00b7";
                if (cell === 0 || cell === 0xff) return " ";
                const code = cell & 0x3f;
                if (code < 0x20) return String.fromCharCode(code + 0x40);
                return String.fromCharCode(code);
              })
              .join("")
          )
        : []
  );

  const visibleRows = $derived.by(() => {
    if (displayRows.length === 0) return [];
    const keepFullGrid =
      frame?.mode === "Text32x16" ||
      frame?.mode === "Semigraphics4" ||
      frame?.mode === "Semigraphics6" ||
      frame?.mode === "Unknown";
    if (keepFullGrid) return displayRows;
    let end = displayRows.length;
    while (end > 0 && displayRows[end - 1].trim() === "") end -= 1;
    return displayRows.slice(0, Math.max(end, 1));
  });

  /** Always honour the machine text grid (32×16), never shrink to content. */
  const displayCols = $derived(frame?.cols ?? 32);
  const displayRowCount = $derived(
    frame?.mode === "Text32x16" || frame?.mode === "Semigraphics4" || frame?.mode === "Semigraphics6"
      ? (frame?.rows ?? 16)
      : visibleRows.length > 0
        ? visibleRows.length
        : (frame?.rows ?? 16)
  );

  /** Pad/truncate each row to exactly displayCols so the grid is stable. */
  const gridRows = $derived.by(() => {
    const cols = displayCols;
    const rows = displayRowCount;
    const src = visibleRows;
    const out: string[] = [];
    for (let r = 0; r < rows; r++) {
      let line = src[r] ?? "";
      if (line.length < cols) line = line.padEnd(cols, " ");
      else if (line.length > cols) line = line.slice(0, cols);
      out.push(line);
    }
    return out;
  });

  const screenText = $derived(gridRows.join("\n"));

  /**
   * Fit the full monospaced grid into the panel.
   * CSS uses `1ch` per column and `line-height` per row — metrics must match.
   */
  function fitFont(width: number, height: number, cols: number, rows: number) {
    if (width < 32 || height < 32 || cols < 1 || rows < 1) return;
    const outerPad = 20; // .crt-body padding + border slack
    const lineRatio = 1.2;
    const charW = 1.0; // matches CSS `1ch` for mono fonts
    const padX = 0.55;
    const padY = 0.45;
    const availW = Math.max(0, width - outerPad);
    const availH = Math.max(0, height - outerPad);
    const byW = availW / (cols * charW + 2 * padX);
    const byH = availH / (rows * lineRatio + 2 * padY);
    // 0.96 safety so borders/scrollbars never clip a column
    const next = Math.floor(Math.min(byW, byH) * 0.96 * 10) / 10;
    fontPx = Math.max(8, Math.min(36, next));
  }

  $effect(() => {
    const el = bodyEl;
    const cols = displayCols;
    const rows = displayRowCount;
    // touch screenText so we remeasure when frame content layout changes
    void screenText;
    if (!el) return;

    const measure = () => {
      const r = el.getBoundingClientRect();
      fitFont(r.width, r.height, cols, rows);
    };
    measure();

    const ro = new ResizeObserver((entries) => {
      const cr = entries[0]?.contentRect;
      if (!cr) return;
      fitFont(cr.width, cr.height, cols, rows);
    });
    ro.observe(el);
    return () => ro.disconnect();
  });
</script>

<div class="panel video-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("machine.videoTitle")}</span>
    <div class="ph-actions">
      {#if frame}
        {#if firmwareLabel}
          <span class="fw mono" title={firmwareLabel}>{firmwareLabel}</span>
        {/if}
        {#if keyboardEnabled}
          <span class="kbd-hint mono" class:on={focused}>{$t("machine.kbdHint")}</span>
        {/if}
        <span class="mode mono">{modeLabel(frame.mode)}</span>
        <span class="dims mono">{displayCols}×{displayRowCount}</span>
        <button class="hdr-btn" onclick={() => onGoto(frame.base_addr)} title={fmtAddr(frame.base_addr)} aria-label={$t("machine.ioGoto")}>
          <Icon name="external" size={13} />
        </button>
        <button class="hdr-btn" onclick={onFullscreen} title={$t("video.fullscreen")} aria-label={$t("video.fullscreen")}>
          <Icon name="expand" size={13} />
        </button>
      {/if}
      <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
        <Icon name="close" size={13} />
      </button>
    </div>
  </div>
  <div
    class="panel-body crt-body"
    class:kbd-focus={focused && keyboardEnabled}
    bind:this={bodyEl}
    tabindex={keyboardEnabled ? 0 : -1}
    role={keyboardEnabled ? "application" : undefined}
    aria-label={keyboardEnabled ? $t("machine.kbdCapture") : undefined}
    onfocus={() => (focused = true)}
    onblur={() => (focused = false)}
    onkeydown={(e) => handleKey(e, true)}
    onkeyup={(e) => handleKey(e, false)}
  >
    {#if !frame}
      <EmptyState icon="video" message={$t("machine.videoEmpty")} size={13} />
    {:else}
      <div class="crt">
        <pre
          class="screen mono"
          style:--cols={displayCols}
          style:--rows={displayRowCount}
          style:font-size="{fontPx}px"
        >{screenText}</pre>
        <div class="scanlines" aria-hidden="true"></div>
        <div class="crt-glow" aria-hidden="true"></div>
      </div>
    {/if}
  </div>
</div>

<style>
  .video-panel {
    height: 100%;
    min-height: 0;
  }

  .video-panel .panel-header .mode {
    color: var(--accent);
    font-size: 10.5px;
  }

  .video-panel .panel-header .dims {
    color: var(--text-faint);
    font-size: 10.5px;
  }

  .video-panel .panel-header .fw {
    color: var(--text-muted);
    font-size: 10px;
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .video-panel .panel-header .kbd-hint {
    color: var(--text-faint);
    font-size: 10px;
  }

  .video-panel .panel-header .kbd-hint.on {
    color: var(--accent);
  }

  .crt-body {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 10px;
    background: var(--crt-bg);
    overflow: hidden;
    outline: none;
    min-height: 0;
    flex: 1;
  }

  .crt-body.kbd-focus {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .crt {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    max-width: 100%;
    max-height: 100%;
  }

  .screen {
    --line-ratio: 1.2;
    --pad-x: 0.55em;
    --pad-y: 0.45em;
    position: relative;
    margin: 0;
    box-sizing: content-box;
    font-family: var(--font-mono), ui-monospace, "Cascadia Mono", "Consolas", monospace;
    font-weight: 500;
    font-variant-ligatures: none;
    font-feature-settings: "liga" 0, "calt" 0;
    letter-spacing: 0;
    line-height: var(--line-ratio);
    color: var(--crt-phosphor);
    background: transparent;
    text-shadow: 0 0 5px var(--crt-glow);
    border: 1px solid var(--crt-border);
    border-radius: 6px;
    padding: var(--pad-y) var(--pad-x);
    white-space: pre;
    /* Exact grid: one `ch` per column — must match fitFont charW */
    width: calc(var(--cols) * 1ch);
    max-width: 100%;
    overflow: hidden;
    z-index: 1;
  }

  .scanlines {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: repeating-linear-gradient(
      to bottom,
      var(--crt-scanline) 0px,
      var(--crt-scanline) 1px,
      transparent 1px,
      transparent 3px
    );
    mix-blend-mode: multiply;
    border-radius: 6px;
    z-index: 2;
  }

  .crt-glow {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: radial-gradient(120% 100% at 50% 50%, transparent 55%, rgba(0, 0, 0, 0.45) 100%);
    border-radius: 6px;
    z-index: 3;
  }
</style>
