<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";

  let {
    addresses,
    collapsed = false,
    onToggleCollapse,
    onRemove,
    onClearAll,
    onGoto,
  }: {
    addresses: number[];
    collapsed?: boolean;
    onToggleCollapse?: () => void;
    onRemove: (addr: number) => void;
    onClearAll: () => void;
    onGoto: (addr: number) => void;
  } = $props();

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }
</script>

<div class="panel wp-panel panel-secondary" class:collapsed>
  <div class="panel-header">
    <span class="ph-title">
      <Icon name="watch" size={12} />
      {$t("watchpoints.title")}
      <span class="count">{addresses.length}</span>
    </span>
    <div class="ph-actions">
      {#if addresses.length > 0}
        <button class="clear-btn" onclick={onClearAll}>{$t("watchpoints.clearAll")}</button>
      {/if}
      {#if onToggleCollapse}
        <button class="hdr-btn collapse-btn" onclick={onToggleCollapse} title={collapsed ? $t("panels.expand") : $t("panels.collapse")} aria-label={collapsed ? $t("panels.expand") : $t("panels.collapse")} aria-expanded={!collapsed}>
          <Icon name="chevron-down" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body wp-list">
    {#if addresses.length === 0}
      <div class="empty-line"><Icon name="watch" size={12} /> {$t("empty.watchpoints")}</div>
    {:else}
      {#each addresses as addr}
        <div class="wp-entry">
          <button class="addr mono" onclick={() => onGoto(addr)} title={$t("watchpoints.goto")}>
            {fmtAddr(addr)}
          </button>
          <button class="remove" onclick={() => onRemove(addr)} aria-label={$t("watchpoints.remove")}>
            <Icon name="close" size={11} />
          </button>
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

  .wp-panel.collapsed .panel-body {
    display: none;
  }

  .wp-panel .ph-title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .wp-panel .ph-title :global(.icon) {
    color: var(--amber);
    margin-right: 0;
  }

  .count {
    font-size: 10px;
    color: var(--text-faint);
    font-weight: 500;
  }

  .collapse-btn :global(.icon) {
    transition: transform var(--motion-normal) var(--ease-tactile);
    margin-right: 0;
  }

  .wp-panel.collapsed .collapse-btn :global(.icon) {
    transform: rotate(-90deg);
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

  .wp-entry {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 10px;
    border-bottom: 1px solid var(--border);
  }

  .wp-entry:hover {
    background: var(--bg-hover);
  }

  .addr {
    background: none;
    border: none;
    padding: 0;
    color: var(--amber);
    font-family: var(--font-mono);
    font-size: 11px;
    cursor: pointer;
  }

  .addr:hover {
    color: var(--accent);
  }

  .remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    padding: 0;
    color: var(--text-faint);
    cursor: pointer;
  }

  .remove:hover {
    color: var(--danger);
  }
</style>