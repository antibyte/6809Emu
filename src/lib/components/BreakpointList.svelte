<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import CollapseButton from "./CollapseButton.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fmtAddr } from "../format";

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
        <CollapseButton {collapsed} onclick={onToggleCollapse} />
      {/if}
    </div>
  </div>
  <div class="panel-body bp-list">
    {#if entries.length === 0}
      <EmptyState icon="breakpoint" message={$t("empty.breakpoints")} />
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