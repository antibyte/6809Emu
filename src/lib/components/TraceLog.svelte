<script lang="ts">
  import type { TraceEntry } from "../types";
  import { t } from "../i18n";

  let {
    entries,
    maxDisplay,
    onMaxDisplayChange,
    onClear,
    onNavigate,
  }: {
    entries: TraceEntry[];
    maxDisplay: number;
    onMaxDisplayChange: (value: number) => void;
    onClear: () => void;
    onNavigate: (addr: number) => void;
  } = $props();

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }
</script>

<div class="panel trace-panel panel-primary">
  <div class="panel-header">
    <span>{$t("trace.title")}</span>
    <div class="header-controls">
      <label class="depth-label">
        {$t("trace.depth")}:
        <select
          value={maxDisplay}
          onchange={(e) => onMaxDisplayChange(parseInt((e.target as HTMLSelectElement).value, 10))}
        >
          <option value={30}>30</option>
          <option value={50}>50</option>
          <option value={100}>100</option>
          <option value={200}>200</option>
        </select>
      </label>
      <button onclick={onClear}>{$t("trace.clear")}</button>
    </div>
  </div>
  <div class="panel-body trace-list">
    {#if entries.length === 0}
      <div class="empty">{$t("trace.empty")}</div>
    {:else}
      {#each [...entries].reverse() as entry}
        <button class="trace-entry" onclick={() => onNavigate(entry.pc_before)}>
          <span class="pc mono">{fmtAddr(entry.pc_before)}</span>
          <span class="insn mono">
            {entry.mnemonic}
            {#if entry.operands}
              {entry.operands}
            {/if}
          </span>
          <span class="cycles">{entry.cycles}c</span>
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .trace-panel {
    height: 100%;
    min-height: 0;
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: 10px;
    text-transform: none;
    letter-spacing: 0;
  }

  .depth-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-weight: 400;
    color: var(--text-dim);
  }

  .depth-label select {
    padding: 2px 4px;
    font-size: 10px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 4px;
  }

  .trace-list {
    padding: 0;
    font-size: 11px;
    overflow: auto;
    min-height: 0;
  }

  .empty {
    padding: 16px;
    color: var(--text-dim);
    text-align: center;
    font-size: 12px;
  }

  .trace-entry {
    display: grid;
    grid-template-columns: 64px 1fr 40px;
    gap: 8px;
    padding: 3px 12px;
    border-bottom: 1px solid rgba(36, 48, 64, 0.5);
    width: 100%;
    background: none;
    border-left: none;
    border-right: none;
    border-top: none;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
  }

  .trace-entry:hover {
    background: rgba(255, 255, 255, 0.04);
  }

  .pc {
    color: var(--text-dim);
  }

  .insn {
    color: var(--text);
  }

  .cycles {
    color: var(--accent-amber);
    text-align: right;
    opacity: 0.7;
  }
</style>