<script lang="ts">
  import type { TraceEntry } from "../types";
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fmtAddr } from "../format";

  let {
    entries,
    maxDisplay,
    onMaxDisplayChange,
    onClear,
    onNavigate,
    onClose,
  }: {
    entries: TraceEntry[];
    maxDisplay: number;
    onMaxDisplayChange: (value: number) => void;
    onClear: () => void;
    onNavigate: (addr: number) => void;
    onClose?: () => void;
  } = $props();

  // Newest first; show delta cycles vs the previous (older) entry.
  const reversed = $derived([...entries].reverse());
  const rows = $derived(
    reversed.map((entry, i) => {
      const older = reversed[i + 1];
      const delta = older != null ? entry.cycles - older.cycles : null;
      return { entry, delta, isNew: i === 0 };
    }),
  );
</script>

<div class="panel trace-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("trace.title")}</span>
    <div class="header-controls">
      <label class="depth-label">
        {$t("trace.depth")}
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
      <button class="ghost icon-btn" onclick={onClear} title={$t("trace.clear")} aria-label={$t("trace.clear")}>
        <Icon name="reset" size={12} />
      </button>
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body trace-list">
    {#if rows.length === 0}
      <EmptyState icon="trace" message={$t("trace.empty")} />
    {:else}
      <div class="row header">
        <span class="pc-h">{$t("status.pc")}</span>
        <span class="insn-h">{$t("disasm.title")}</span>
        <span class="cyc-h" title={$t("trace.cycles")}>±{$t("trace.cycles")}</span>
      </div>
      {#each rows as row (row.entry.id)}
        <button class="trace-entry" class:new={row.isNew} onclick={() => onNavigate(row.entry.pc_before)}>
          <span class="pc mono">{fmtAddr(row.entry.pc_before)}</span>
          <span class="insn mono">
            <span class="m">{row.entry.mnemonic}</span>
            {#if row.entry.operands}<span class="op"> {row.entry.operands}</span>{/if}
          </span>
          <span class="cycles mono" title={row.entry.cycles + "c"}>
            {#if row.delta != null}
              <span class="delta">+{row.delta}</span>
            {:else}
              <span class="delta dim">—</span>
            {/if}
          </span>
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
    gap: 6px;
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
    background: var(--bg-0);
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

  .row.header {
    display: grid;
    grid-template-columns: 54px 1fr 52px;
    gap: 8px;
    padding: 4px 12px;
    border-bottom: 1px solid var(--border);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .cyc-h {
    text-align: right;
  }

  .trace-entry {
    display: grid;
    grid-template-columns: 54px 1fr 52px;
    gap: 8px;
    padding: 2px 12px;
    border-bottom: 1px solid var(--border);
    width: 100%;
    background: none;
    border-left: none;
    border-right: none;
    border-top: none;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
    min-height: var(--row-h);
    align-items: center;
  }

  .trace-entry:hover {
    background: var(--bg-hover);
  }

  .trace-entry.new {
    background: var(--accent-soft);
  }

  .trace-entry.new .pc {
    color: var(--accent);
  }

  .pc {
    color: var(--text-dim);
  }

  .insn {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .insn .m {
    color: var(--accent);
    font-weight: 600;
  }

  .insn .op {
    color: var(--text-dim);
  }

  .cycles {
    text-align: right;
    color: var(--amber);
    font-size: 10.5px;
  }

  .delta {
    color: var(--text-faint);
    font-size: 10px;
  }

  .delta.dim {
    opacity: 0.5;
  }
</style>
