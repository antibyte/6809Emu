<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import type { VideoFrame } from "../types";

  let {
    open,
    frame,
    onClose,
    onGoto,
  }: {
    open: boolean;
    frame: VideoFrame | null;
    onClose: () => void;
    onGoto: (addr: number) => void;
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
    if (keepFullGrid) {
      return displayRows;
    }
    let end = displayRows.length;
    while (end > 0 && displayRows[end - 1].trim() === "") {
      end -= 1;
    }
    return displayRows.slice(0, Math.max(end, 1));
  });

  const displayCols = $derived(
    visibleRows.length > 0
      ? Math.max(...visibleRows.map((row) => row.length), frame?.cols ?? 0)
      : (frame?.cols ?? 32)
  );

  const displayRowCount = $derived(visibleRows.length);
  const screenText = $derived(visibleRows.join("\n"));

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop" onclick={onClose} role="presentation">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="video-modal-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="modal-header">
        <div class="title-group">
          <h2 id="video-modal-title">{$t("machine.videoTitle")}</h2>
          {#if frame}
            <span class="mode mono">{modeLabel(frame.mode)}</span>
            <span class="dims mono">{displayCols}×{displayRowCount}</span>
          {/if}
        </div>
        <div class="actions">
          {#if frame}
            <button
              class="base mono"
              onclick={() => onGoto(frame.base_addr)}
              title={$t("machine.ioGoto")}
            >
              {fmtAddr(frame.base_addr)}
            </button>
          {/if}
          <button class="close-btn" onclick={onClose} aria-label={$t("machine.videoClose")}>
            <Icon name="close" size={14} />
          </button>
        </div>
      </header>

      <div class="modal-body">
        {#if !frame}
          <div class="empty">{$t("machine.videoEmpty")}</div>
        {:else}
          <pre
            class="screen mono"
            style:--cols={displayCols}
            style:--rows={displayRowCount}
          >{screenText}</pre>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(4, 8, 12, 0.72);
    backdrop-filter: blur(6px);
    animation: backdropIn var(--motion-normal) ease;
  }

  @keyframes backdropIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    width: fit-content;
    max-width: min(96vw, 900px);
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border-glow);
    border-radius: calc(var(--radius) + 2px);
    box-shadow:
      0 24px 64px rgba(0, 0, 0, 0.55),
      0 0 0 1px rgba(57, 255, 20, 0.08);
    overflow: hidden;
    transform-origin: center;
    animation: modalIn var(--motion-slow) var(--ease-tactile);
  }

  @keyframes modalIn {
    from {
      opacity: 0;
      transform: scale(0.92) translateY(8px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
  }

  .title-group {
    display: flex;
    align-items: baseline;
    gap: 12px;
    min-width: 0;
  }

  .title-group h2 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .mode {
    color: var(--accent);
    font-size: 12px;
  }

  .dims {
    color: var(--text-dim);
    font-size: 11px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .base {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
    padding: 4px 8px;
    border-radius: 4px;
  }

  .base:hover {
    background: rgba(57, 255, 20, 0.08);
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    background: none;
    border: 1px solid var(--border);
    color: var(--text-dim);
  }

  .close-btn:hover {
    color: var(--text);
    border-color: var(--accent-dim);
  }

  .close-btn :global(.icon) {
    margin-right: 0;
  }

  .modal-body {
    display: flex;
    justify-content: center;
    padding: 14px;
    background: #060a10;
  }

  .empty {
    padding: 32px 24px;
    color: var(--text-dim);
    text-align: center;
    font-size: 13px;
  }

  .screen {
    --line-ratio: 1.35;
    margin: 0;
    font-size: min(
      24px,
      calc((min(90vw, 720px) - 2.5rem) / var(--cols)),
      calc((min(70vh, 520px) - 2.5rem) / var(--rows) / var(--line-ratio))
    );
    line-height: var(--line-ratio);
    color: #6ee7b7;
    background: #0a1018;
    border: 1px solid rgba(57, 255, 20, 0.2);
    border-radius: 6px;
    padding: 1.1em 1.25em;
    box-shadow: inset 0 0 40px rgba(0, 0, 0, 0.35);
    white-space: pre;
    width: calc(var(--cols) * 1ch + 2.5em);
    max-width: calc(100vw - 4rem);
    overflow: hidden;
  }
</style>