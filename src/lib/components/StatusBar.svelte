<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtAddr } from "../format";

  let {
    running,
    halted,
    busy,
    pc,
    cycles,
    pending,
    trap,
    cpuLabel,
    machineLabel,
    speedIndex,
    speedPresets,
    onSpeedChange,
    onOpenShortcuts,
  }: {
    running: boolean;
    halted: boolean;
    busy: boolean;
    pc: number;
    cycles: number;
    pending: string | null;
    trap: string | null;
    cpuLabel: string;
    machineLabel: string;
    speedIndex: number;
    speedPresets: { label: string }[];
    onSpeedChange: (index: number) => void;
    onOpenShortcuts: () => void;
  } = $props();

  const stateLabel = $derived(running ? $t("statusbar.running") : halted ? $t("statusbar.halted") : $t("statusbar.ready"));
  const stateClass = $derived(running ? "on" : halted ? "danger" : "");
  const pcHex = $derived(fmtAddr(pc));
</script>

<footer class="status-bar">
  <span class="pill {stateClass}" title={stateLabel}>
    <span class="dot"></span>
    {stateLabel}
  </span>

  <span class="seg mono" title={$t("status.pc")}>
    <span class="k">PC</span>
    <span class="v accent">{pcHex}</span>
  </span>

  <span class="seg mono" title={$t("status.cycles")}>
    <span class="k">{$t("status.cycles")}</span>
    <span class="v">{cycles.toLocaleString()}</span>
  </span>

  {#if pending}
    <span class="pill warn" title={$t("interrupts.pending")}>
      <span class="dot"></span>
      {pending}
    </span>
  {/if}

  {#if trap}
    <span class="pill danger" title={$t("status.trap")}>
      <span class="dot"></span>
      {$t("status.trap")}: {trap}
    </span>
  {/if}

  <span class="spacer"></span>

  <span class="seg narrow-optional" title={$t("cpu.label")}>
    <Icon name="cpu" size={12} />
    <span class="v">{cpuLabel}</span>
  </span>

  <span class="seg narrow-optional" title={$t("machine.label")}>
    <Icon name="chip" size={12} />
    <span class="v">{machineLabel}</span>
  </span>

  <span class="seg speed" title={$t("speed.label")}>
    <span class="k">{$t("speed.label")}</span>
    <select
      value={speedIndex}
      onchange={(e) => onSpeedChange(parseInt((e.target as HTMLSelectElement).value, 10))}
    >
      {#each speedPresets as preset, i}
        <option value={i}>{preset.label}</option>
      {/each}
    </select>
  </span>

  <button class="hdr-btn shortcuts-btn" onclick={onOpenShortcuts} title={$t("shortcuts.open")} aria-label={$t("shortcuts.open")}>
    <Icon name="keyboard" size={14} />
  </button>
</footer>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--status-h);
    padding: 0 12px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 11.5px;
    color: var(--text-dim);
    flex-shrink: 0;
    overflow: hidden;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    color: var(--text-dim);
  }

  .seg .k {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .seg .v {
    color: var(--text);
  }

  .seg.mono .v {
    font-family: var(--font-mono);
  }

  .pill .dot {
    animation: none;
  }

  .pill.on .dot {
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .spacer {
    flex: 1;
    min-width: 0;
  }

  .speed select {
    width: auto;
    min-width: 56px;
    padding: 2px 18px 2px 6px;
    font-size: 11px;
    font-family: var(--font-ui);
    color: var(--text);
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: 4px;
  }

  .shortcuts-btn {
    width: 22px;
    height: 22px;
  }

  @media (max-width: 760px) {
    .seg .k {
      display: none;
    }
    .seg.speed .k {
      display: inline;
    }
  }

  @media (max-width: 520px) {
    .narrow-optional {
      display: none;
    }
    .status-bar {
      gap: 6px;
      padding: 0 8px;
    }
  }

  @media (max-width: 400px) {
    .status-bar {
      gap: 4px;
      padding: 0 6px;
    }
  }
</style>
