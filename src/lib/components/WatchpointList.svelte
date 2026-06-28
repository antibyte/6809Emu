<script lang="ts">
  import { t } from "../i18n";

  let {
    addresses,
    onRemove,
    onClearAll,
    onGoto,
  }: {
    addresses: number[];
    onRemove: (addr: number) => void;
    onClearAll: () => void;
    onGoto: (addr: number) => void;
  } = $props();

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }
</script>

<div class="panel wp-panel panel-secondary">
  <div class="panel-header">
    <span>{$t("watchpoints.title")} ({addresses.length})</span>
    {#if addresses.length > 0}
      <button class="clear-btn" onclick={onClearAll}>{$t("watchpoints.clearAll")}</button>
    {/if}
  </div>
  <div class="panel-body wp-list">
    {#if addresses.length === 0}
      <div class="empty">{$t("watchpoints.empty")}</div>
    {:else}
      {#each addresses as addr}
        <div class="wp-entry">
          <button class="addr mono" onclick={() => onGoto(addr)} title={$t("watchpoints.goto")}>
            {fmtAddr(addr)}
          </button>
          <button
            class="remove"
            onclick={() => onRemove(addr)}
            aria-label={$t("watchpoints.remove")}
          >×</button>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .wp-panel {
    height: 100%;
    min-height: 0;
  }

  .clear-btn {
    padding: 2px 8px;
    font-size: 10px;
    text-transform: none;
    letter-spacing: 0;
  }

  .wp-list {
    padding: 0;
    font-size: 11px;
    overflow: auto;
    min-height: 0;
  }

  .empty {
    padding: 10px;
    color: var(--text-dim);
    text-align: center;
    font-size: 11px;
  }

  .wp-entry {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 10px;
    border-bottom: 1px solid rgba(36, 48, 64, 0.5);
  }

  .addr {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent-amber);
    font-family: var(--font-mono);
    font-size: 11px;
    cursor: pointer;
  }

  .addr:hover {
    color: var(--accent);
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