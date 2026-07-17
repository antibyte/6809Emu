<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtByte, fmtAddr } from "../format";
  import type { PiaState } from "../types";

  let {
    state,
    onToggleInput,
    onClose,
  }: {
    state: PiaState | null;
    onToggleInput: (port: "a" | "b", bit: number, on: boolean) => void;
    onClose?: () => void;
  } = $props();

  function bitAt(value: number, bit: number): boolean {
    return (value >> bit) & 1 ? true : false;
  }

  function isInput(ddr: number, bit: number): boolean {
    return !((ddr >> bit) & 1);
  }

  const bits = [7, 6, 5, 4, 3, 2, 1, 0] as const;
</script>

<div class="panel pia-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("pia.title")}
      {#if state}
        <span class="addr mono">{fmtAddr(state.config.base_addr)}</span>
      {/if}
    </span>
    <div class="ph-actions">
      {#if state?.irq_a}
        <span class="irq-badge on">{$t("pia.irqA")}</span>
      {/if}
      {#if state?.irq_b}
        <span class="irq-badge on">{$t("pia.irqB")}</span>
      {/if}
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body pia-body">
    {#if !state}
      <div class="empty-line">{$t("pia.title")} —</div>
    {:else}
      {#each [["a", $t("pia.portA"), state.ddra, state.ora, state.ira, state.cra, state.irq_a] as const, ["b", $t("pia.portB"), state.ddrb, state.orb, state.irb, state.crb, state.irq_b] as const] as [port, label, ddr, or_, ir, cr, irq]}
        <div class="port-group">
          <div class="port-header">
            <span class="port-label">{label}</span>
            <span class="port-value mono">{fmtByte(or_)}<span class="dim">/</span>{fmtByte(ir)}</span>
          </div>

          <div class="bit-grid">
            {#each bits as bit}
              {@const input = isInput(ddr, bit)}
              {@const active = input ? bitAt(ir, bit) : bitAt(or_, bit)}
              <button
                class="bit-cell"
                class:input
                class:output={!input}
                class:on={active}
                class:off={!active}
                disabled={!input}
                onclick={() => input && onToggleInput(port, bit, !active)}
                title={input ? `${$t("pia.toggleInput")} D${bit}` : `D${bit}: ${$t("pia.output")}`}
                aria-label={input ? `${$t("pia.toggleInput")} D${bit}` : `D${bit}: ${$t("pia.output")}`}
              >
                <span class="bit-num">D{bit}</span>
                <span class="led" class:amber={input} class:green={!input}></span>
                <span class="bit-dir">{input ? "I" : "O"}</span>
              </button>
            {/each}
          </div>

          <div class="port-meta">
            <span class="meta-item">
              <span class="meta-label">{$t("pia.ddr")}</span>
              <span class="meta-val mono">{fmtByte(ddr)}</span>
            </span>
            <span class="meta-item">
              <span class="meta-label">{$t("pia.cra")}</span>
              <span class="meta-val mono">{fmtByte(cr)}</span>
            </span>
            {#if irq}
              <span class="irq-badge on">{$t("pia.irq")}</span>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .pia-panel {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .addr {
    color: var(--accent);
    font-size: 10.5px;
    font-weight: 500;
  }

  .irq-badge {
    display: inline-flex;
    align-items: center;
    padding: 2px 6px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    border-radius: 3px;
    color: var(--text-faint);
    border: 1px solid var(--border);
    background: var(--bg-2);
  }

  .irq-badge.on {
    color: var(--danger);
    border-color: var(--danger-line);
    background: var(--danger-soft);
    animation: irqPulse 1.2s ease-in-out infinite;
  }

  @keyframes irqPulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .pia-body {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    overflow: auto;
  }

  .port-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .port-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  .port-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-dim);
  }

  .port-value {
    font-size: 11px;
    color: var(--text);
  }

  .port-value .dim {
    color: var(--text-faint);
    margin: 0 1px;
  }

  .bit-grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 3px;
  }

  .bit-cell {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 5px 2px 4px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-0);
    cursor: default;
    min-width: 0;
    transition:
      border-color var(--motion-normal) ease,
      background var(--motion-normal) ease;
  }

  .bit-cell.input {
    cursor: pointer;
  }

  .bit-cell.input:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--bg-hover);
  }

  .bit-cell.input:active:not(:disabled) {
    transform: scale(0.95);
  }

  .bit-cell.input:focus-visible {
    outline: none;
    border-color: var(--accent-dim);
    box-shadow: var(--ring);
  }

  .bit-num {
    font-family: var(--font-mono);
    font-size: 8.5px;
    font-weight: 600;
    color: var(--text-faint);
    letter-spacing: 0.02em;
  }

  .led {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--bg-3);
    border: 1px solid var(--border);
    transition:
      background var(--motion-normal) ease,
      border-color var(--motion-normal) ease,
      box-shadow var(--motion-normal) ease;
  }

  /* Output LED: green */
  .bit-cell.output.on .led {
    background: var(--accent);
    border-color: var(--accent-dim);
    box-shadow: 0 0 6px var(--accent-soft);
  }

  /* Input LED: amber */
  .bit-cell.input.on .led {
    background: var(--amber);
    border-color: var(--amber);
    box-shadow: 0 0 6px color-mix(in srgb, var(--amber) 40%, transparent);
  }

  .bit-dir {
    font-family: var(--font-mono);
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .bit-cell.input .bit-dir {
    color: var(--amber);
  }

  .bit-cell.output .bit-dir {
    color: var(--accent);
  }

  .port-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-top: 4px;
    border-top: 1px solid var(--border);
  }

  .meta-item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .meta-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
  }

  .meta-val {
    font-size: 11px;
    color: var(--text);
  }
</style>
