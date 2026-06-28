<script lang="ts">
  import { t } from "../i18n";

  let {
    entries,
    onRemove,
    onClearAll,
    onGoto,
  }: {
    entries: { address: number; text: string }[];
    onRemove: (addr: number) => void;
    onClearAll: () => void;
    onGoto: (addr: number) => void;
  } = $props();

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }
</script>

<div class="panel bp-panel panel-secondary">
  <div class="panel-header">
    <span>{$t("breakpoints.title")} ({entries.length})</span>
    {#if entries.length > 0}
      <button class="clear-btn" onclick={onClearAll}>{$t("breakpoints.clearAll")}</button>
    {/if}
  </div>
  <div class="panel-body bp-list">
    {#if entries.length === 0}
      <div class="empty">{$t("breakpoints.empty")}</div>
    {:else}
      {#each entries as entry}
        <div class="bp-entry">
          <button class="addr mono" onclick={() => onGoto(entry.address)} title={$t("breakpoints.goto")}>
            {fmtAddr(entry.address)}
          </button>
          <span class="text mono">{entry.text || "-"}</span>
          <button
            class="remove"
            onclick={() => onRemove(entry.address)}
            aria-label={$t("breakpoints.remove")}
          >×</button>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .bp-panel {
    height: 100%;
    min-height: 0;
  }

  .clear-btn {
    padding: 2px 8px;
    font-size: 10px;
    text-transform: none;
    letter-spacing: 0;
  }

  .bp-list {
    padding: 0;
    font-size: 11px;
    overflow: auto;
    min-height: 0;
  }

  .empty {
    padding: 12px;
    color: var(--text-dim);
    text-align: center;
    font-size: 11px;
  }

  .bp-entry {
    display: grid;
    grid-template-columns: 64px 1fr 24px;
    gap: 6px;
    padding: 4px 10px;
    align-items: center;
    border-bottom: 1px solid rgba(36, 48, 64, 0.5);
  }

  .bp-entry:hover {
    background: rgba(255, 255, 255, 0.02);
  }

  .addr {
    background: none;
    border: none;
    padding: 0;
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }

  .addr:hover {
    color: var(--accent);
  }

  .text {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .remove {
    background: none;
    border: none;
    padding: 0;
    font-size: 16px;
    line-height: 1;
    color: var(--text-dim);
    cursor: pointer;
  }

  .remove:hover {
    color: var(--danger);
  }
</style>