<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fade, scale } from "svelte/transition";
  import type { UpdateOffer } from "../updater";

  let {
    open,
    offer,
    installing = false,
    progress = null as number | null,
    onInstall,
    onLater,
    onSkip,
    onClose,
  }: {
    open: boolean;
    offer: UpdateOffer | null;
    installing?: boolean;
    progress?: number | null;
    onInstall: () => void;
    onLater: () => void;
    onSkip: () => void;
    onClose: () => void;
  } = $props();

  let panel: HTMLDivElement | undefined = $state();
  let lastFocused: HTMLElement | null = null;

  function onKeydown(e: KeyboardEvent) {
    if (!open || installing) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  function onPanelKeydown(e: KeyboardEvent) {
    if (!open || !panel) return;
    if (e.key === "Tab") {
      const focusables = panel.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  $effect(() => {
    if (open) {
      lastFocused = document.activeElement as HTMLElement | null;
      const id = window.setTimeout(() => panel?.focus(), 0);
      return () => {
        window.clearTimeout(id);
        lastFocused?.focus?.();
      };
    }
  });

  const progressLabel = $derived(
    progress == null ? $t("update.downloading") : `${$t("update.downloading")} ${progress}%`,
  );
</script>

<svelte:window onkeydown={onKeydown} />

{#if open && offer}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="backdrop"
    transition:fade={{ duration: 120 }}
    onclick={() => { if (!installing) onClose(); }}
    role="presentation"
  >
    <div
      class="overlay"
      transition:scale={{ duration: 160, start: 0.96 }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="upd-title"
      tabindex="-1"
      bind:this={panel}
      onkeydown={onPanelKeydown}
      onclick={(e) => e.stopPropagation()}
    >
      <header class="ov-header">
        <h2 id="upd-title">
          <Icon name="external" size={15} />
          {$t("update.available")}
        </h2>
        <button
          class="hdr-btn"
          onclick={onClose}
          disabled={installing}
          aria-label={$t("shortcuts.close")}
          title={$t("shortcuts.close")}
        >
          <Icon name="close" size={14} />
        </button>
      </header>

      <div class="ov-body">
        <div class="versions">
          <div class="ver-block">
            <span class="ver-label">{$t("update.current")}</span>
            <span class="ver-value mono">v{offer.currentVersion}</span>
          </div>
          <Icon name="arrow-right" size={14} />
          <div class="ver-block accent">
            <span class="ver-label">{$t("update.latest")}</span>
            <span class="ver-value mono">v{offer.version}</span>
          </div>
        </div>

        {#if offer.body}
          <section class="notes">
            <h3>{$t("update.notes")}</h3>
            <pre class="notes-body">{offer.body}</pre>
          </section>
        {/if}

        {#if installing}
          <div class="progress-wrap" aria-live="polite">
            <div class="progress-label">{progressLabel}</div>
            <div class="progress-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress ?? undefined}>
              <div
                class="progress-fill"
                class:indeterminate={progress == null}
                style={progress != null ? `width: ${progress}%` : undefined}
              ></div>
            </div>
          </div>
        {/if}
      </div>

      <footer class="ov-footer">
        <button class="ghost" onclick={onSkip} disabled={installing}>
          {$t("update.skip")}
        </button>
        <div class="spacer"></div>
        <button class="ghost" onclick={onLater} disabled={installing}>
          {$t("update.later")}
        </button>
        <button class="primary" onclick={onInstall} disabled={installing}>
          {#if installing}
            {$t("update.downloading")}
          {:else}
            <Icon name="check" size={13} />
            {$t("update.install")}
          {/if}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 4100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(4, 8, 12, 0.66);
    backdrop-filter: blur(6px);
  }

  .overlay {
    width: min(480px, 96vw);
    background: var(--bg-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    outline: none;
  }

  .ov-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-2);
    border-bottom: 1px solid var(--border);
  }

  .ov-header h2 {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text);
  }

  .ov-header h2 :global(.icon) {
    color: var(--accent);
  }

  .hdr-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
  }

  .hdr-btn:hover:not(:disabled) {
    background: var(--bg-3);
    color: var(--text);
  }

  .hdr-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .ov-body {
    padding: 18px 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .versions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
  }

  .ver-block {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 10px 16px;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-width: 110px;
  }

  .ver-block.accent {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .ver-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
  }

  .ver-value {
    font-size: 15px;
    font-weight: 600;
    color: var(--text);
  }

  .ver-block.accent .ver-value {
    color: var(--accent);
  }

  .notes h3 {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
    margin-bottom: 6px;
  }

  .notes-body {
    margin: 0;
    max-height: 160px;
    overflow: auto;
    padding: 10px 12px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--text-dim);
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .progress-label {
    font-size: 11.5px;
    color: var(--text-dim);
  }

  .progress-bar {
    height: 6px;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: 99px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 99px;
    transition: width 0.15s ease;
  }

  .progress-fill.indeterminate {
    width: 40%;
    animation: indeterminate 1.2s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(350%); }
  }

  .ov-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg-2);
  }

  .spacer {
    flex: 1;
  }

  .ov-footer button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    padding: 6px 12px;
    border-radius: 6px;
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--bg-1);
    color: var(--text);
  }

  .ov-footer button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .ov-footer button.ghost {
    background: transparent;
    border-color: transparent;
    color: var(--text-dim);
  }

  .ov-footer button.ghost:hover:not(:disabled) {
    color: var(--text);
    background: var(--bg-3);
  }

  .ov-footer button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent, #0a0e12);
    font-weight: 600;
  }

  .ov-footer button.primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }
</style>
