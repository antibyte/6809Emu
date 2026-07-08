<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";

  let {
    entries,
    collapsed = false,
    onToggleCollapse,
    onRemove,
    onClearAll,
    onGoto,
  }: {
    entries: { address: number; text: string }[];
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

<div class="panel bp-panel panel-secondary" class:collapsed>
  <div class="panel-header">
    <span class="ph-title">
      <Icon name="breakpoint" size={12} />
      {$t("breakpoints.title")}
      <span class="count">{entries.length}</span>
    </span>
    <div class="ph-actions">
      {#if entries.length > 0}
        <button class="clear-btn" onclick={onClearAll}>{$t("breakpoints.clearAll")}</button>
      {/if}
      {#if onToggleCollapse}
        <button class="hdr-btn collapse-btn" onclick={onToggleCollapse} title={collapsed ? $t("panels.expand") : $t("panels.collapse")} aria-label={collapsed ? $t("panels.expand") : $t("panels.collapse")} aria-expanded={!collapsed}>
          <Icon name="chevron-down" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body bp-list">
    {#if entries.length === 0}
      <div class="empty-line"><Icon name="breakpoint" size={12} /> {$t("empty.breakpoints")}</div>
    {:else}
      {#each entries as entry}
        <div class="bp-entry">
          <button class="addr mono" onclick={() => onGoto(entry.address)} title={$t("breakpoints.goto")}>
            {fmtAddr(entry.address)}
          </button>
          <span class="text mono">{entry.text || "—"}</span>
          <button class="remove" onclick={() => onRemove(entry.address)} aria-label={$t("breakpoints.remove")}>
            <Icon name="close" size={11} />
          </button>
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

  .bp-panel.collapsed .panel-body {
    display: none;
  }

  .bp-panel .ph-title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .bp-panel .ph-title :global(.icon) {
    color: var(--danger);
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

  .bp-panel.collapsed .collapse-btn :global(.icon) {
    transform: rotate(-90deg);
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

  .bp-entry {
    display: grid;
    grid-template-columns: 60px 1fr 20px;
    gap: 6px;
    padding: 3px 10px;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }

  .bp-entry:hover {
    background: var(--bg-hover);
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