<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import type { VideoFrame } from "../types";

  let {
    frame,
    onGoto,
    onFullscreen,
    onClose,
  }: {
    frame: VideoFrame | null;
    onGoto: (addr: number) => void;
    onFullscreen: () => void;
    onClose: () => void;
  } = $props();

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }

  function modeLabel(mode: string): string {
    return mode.replace(/([a-z])([0-9])/g, "$1 $2").replace(/x/g, "×");
  }

  const displayRows = $derived(
    frame && frame.rows_text.length > 0
      ? frame.rows_text
      : frame
        ? Array.from({ length: frame.rows }, (_, row) =>
            frame.cells
              .slice(row * frame.cols, (row + 1) * frame.cols)
              .map((cell) => {
                if (cell === 0 || cell === 0xff) return " ";
                const code = cell & 0x7f;
                return code >= 0x20 && code < 0x7f
                  ? String.fromCharCode(code)
                  : "\u00b7";
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

  const displayCols = $derived(
    visibleRows.length > 0
      ? Math.max(...visibleRows.map((row) => row.length), frame?.cols ?? 0)
      : (frame?.cols ?? 32)
  );
  const displayRowCount = $derived(visibleRows.length);
  const screenText = $derived(visibleRows.join("\n"));
</script>

<div class="panel video-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("machine.videoTitle")}</span>
    <div class="ph-actions">
      {#if frame}
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
  <div class="panel-body crt-body">
    {#if !frame}
      <div class="empty-line"><Icon name="video" size={13} /> {$t("machine.videoEmpty")}</div>
    {:else}
      <div class="crt">
        <pre class="screen mono" style:--cols={displayCols} style:--rows={displayRowCount}>{screenText}</pre>
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

  .crt-body {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 10px;
    background: var(--crt-bg);
    overflow: hidden;
  }

  .crt {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .screen {
    --line-ratio: 1.35;
    margin: 0;
    font-size: min(
      20px,
      calc((100% - 1.5rem) / var(--cols)),
      calc((100% - 1.5rem) / var(--rows) / var(--line-ratio))
    );
    line-height: var(--line-ratio);
    color: var(--crt-phosphor);
    background: transparent;
    text-shadow: 0 0 6px var(--crt-glow);
    white-space: pre;
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
    z-index: 2;
  }

  .crt-glow {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: radial-gradient(120% 100% at 50% 50%, transparent 55%, rgba(0, 0, 0, 0.45) 100%);
    z-index: 3;
  }

  .empty-line {
    color: var(--text-faint);
  }
</style>
