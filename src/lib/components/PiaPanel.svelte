<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtByte, fmtAddr } from "../format";
  import type { PiaState } from "../types";
  import { getPiaState } from "../api";
  import { setPiaControlLine } from "../machineApi";

  /** MC6821 control-line fields of the PIA state (optional for older backends). */
  type PiaLines = {
    irqa1?: boolean;
    irqa2?: boolean;
    irqb1?: boolean;
    irqb2?: boolean;
    ca1?: boolean;
    ca2?: boolean;
    cb1?: boolean;
    cb2?: boolean;
    ca2_is_output?: boolean;
    cb2_is_output?: boolean;
    ca2_out?: boolean;
    cb2_out?: boolean;
    ca2_strobes?: number;
    cb2_strobes?: number;
  };
  type PiaView = PiaState & PiaLines;
  type Line = "ca1" | "ca2" | "cb1" | "cb2";

  type PortView = {
    port: "a" | "b";
    labelKey: string;
    crKey: string;
    ddr: number;
    or: number;
    ir: number;
    cr: number;
    irq: boolean;
    c1: Line;
    c2: Line;
    c1Level: boolean;
    c2Level: boolean;
    c2IsOutput: boolean;
    c2Out: boolean;
    flag1: boolean;
    flag2: boolean;
    strobes: number;
  };

  // The prop is called `state`; bind it to another local name so it does
  // not shadow the `$state` rune.
  let {
    state: piaState,
    onToggleInput,
    onClose,
  }: {
    state: PiaState | null;
    onToggleInput: (port: "a" | "b", bit: number, on: boolean) => void;
    onClose?: () => void;
  } = $props();

  // State fetched after a control-line change; replaced as soon as the
  // parent hands in a newer state. Raw (not proxied), so `from` keeps the
  // identity of the prop object it was taken from.
  let fresh = $state.raw<{ from: PiaState | null; value: PiaView } | null>(null);
  let lineBusy = $state(false);

  let view: PiaView | null = $derived(
    fresh && fresh.from === piaState ? fresh.value : piaState
  );

  let ports: PortView[] = $derived(
    view
      ? [
          {
            port: "a",
            labelKey: "pia.portA",
            crKey: "pia.cra",
            ddr: view.ddra,
            or: view.ora,
            ir: view.ira,
            cr: view.cra,
            irq: view.irq_a,
            c1: "ca1",
            c2: "ca2",
            c1Level: view.ca1 ?? true,
            c2Level: view.ca2 ?? true,
            c2IsOutput: view.ca2_is_output ?? (view.cra & 0x20) !== 0,
            c2Out: view.ca2_out ?? true,
            flag1: view.irqa1 ?? (view.cra & 0x80) !== 0,
            flag2: view.irqa2 ?? (view.cra & 0x40) !== 0,
            strobes: view.ca2_strobes ?? 0,
          },
          {
            port: "b",
            labelKey: "pia.portB",
            crKey: "pia.crb",
            ddr: view.ddrb,
            or: view.orb,
            ir: view.irb,
            cr: view.crb,
            irq: view.irq_b,
            c1: "cb1",
            c2: "cb2",
            c1Level: view.cb1 ?? true,
            c2Level: view.cb2 ?? true,
            c2IsOutput: view.cb2_is_output ?? (view.crb & 0x20) !== 0,
            c2Out: view.cb2_out ?? true,
            flag1: view.irqb1 ?? (view.crb & 0x80) !== 0,
            flag2: view.irqb2 ?? (view.crb & 0x40) !== 0,
            strobes: view.cb2_strobes ?? 0,
          },
        ]
      : []
  );

  async function toggleLine(line: Line, level: boolean) {
    if (lineBusy) return;
    lineBusy = true;
    const from = piaState;
    try {
      await setPiaControlLine(line, level);
      const next = await getPiaState();
      if (next) fresh = { from, value: next };
    } catch (err) {
      console.error("setPiaControlLine failed", err);
    } finally {
      lineBusy = false;
    }
  }

  function bitAt(value: number, bit: number): boolean {
    return (value >> bit) & 1 ? true : false;
  }

  function isInput(ddr: number, bit: number): boolean {
    return !((ddr >> bit) & 1);
  }

  function lineName(line: Line): string {
    return line.toUpperCase();
  }

  const bits = [7, 6, 5, 4, 3, 2, 1, 0] as const;
</script>

<div class="panel pia-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("pia.title")}
      {#if view}
        <span class="addr mono">{fmtAddr(view.config.base_addr)}</span>
      {/if}
    </span>
    <div class="ph-actions">
      {#if view?.irq_a}
        <span class="irq-badge on" title={$t("pia.irqOutput")}>{$t("pia.irqA")}</span>
      {/if}
      {#if view?.irq_b}
        <span class="irq-badge on" title={$t("pia.irqOutput")}>{$t("pia.irqB")}</span>
      {/if}
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body pia-body">
    {#if !view}
      <div class="empty-line">{$t("pia.title")} —</div>
    {:else}
      {#each ports as p (p.port)}
        <div class="port-group">
          <div class="port-header">
            <span class="port-label">{$t(p.labelKey)}</span>
            <span class="port-value mono">{fmtByte(p.or)}<span class="dim">/</span>{fmtByte(p.ir)}</span>
          </div>

          <div class="bit-grid">
            {#each bits as bit}
              {@const input = isInput(p.ddr, bit)}
              {@const active = input ? bitAt(p.ir, bit) : bitAt(p.or, bit)}
              <button
                class="bit-cell"
                class:input
                class:output={!input}
                class:on={active}
                class:off={!active}
                disabled={!input}
                onclick={() => input && onToggleInput(p.port, bit, !active)}
                title={input ? `${$t("pia.toggleInput")} D${bit}` : `D${bit}: ${$t("pia.output")}`}
                aria-label={input ? `${$t("pia.toggleInput")} D${bit}` : `D${bit}: ${$t("pia.output")}`}
              >
                <span class="bit-num">D{bit}</span>
                <span class="led" class:amber={input} class:green={!input}></span>
                <span class="bit-dir">{input ? "I" : "O"}</span>
              </button>
            {/each}
          </div>

          <div class="ctl-row">
            <span class="meta-label">{$t("pia.controlLines")}</span>
            <button
              class="ctl-line input"
              class:on={p.c1Level}
              disabled={lineBusy}
              aria-pressed={p.c1Level}
              onclick={() => toggleLine(p.c1, !p.c1Level)}
              title={`${$t("pia.toggleLine")} ${lineName(p.c1)}`}
              aria-label={`${$t("pia.toggleLine")} ${lineName(p.c1)}`}
            >
              <span class="led"></span>
              <span class="ctl-name">{lineName(p.c1)}</span>
              <span class="ctl-lvl mono">{p.c1Level ? "H" : "L"}</span>
            </button>
            {#if p.c2IsOutput}
              <span
                class="ctl-line output"
                class:on={p.c2Out}
                title={`${lineName(p.c2)}: ${$t("pia.c2Output")}`}
                aria-label={`${lineName(p.c2)}: ${$t("pia.c2Output")} ${p.c2Out ? "H" : "L"}`}
              >
                <span class="led"></span>
                <span class="ctl-name">{lineName(p.c2)}</span>
                <span class="ctl-lvl mono">{p.c2Out ? "H" : "L"}</span>
                <span class="ctl-dir">O</span>
              </span>
            {:else}
              <button
                class="ctl-line input"
                class:on={p.c2Level}
                disabled={lineBusy}
                aria-pressed={p.c2Level}
                onclick={() => toggleLine(p.c2, !p.c2Level)}
                title={`${$t("pia.toggleLine")} ${lineName(p.c2)}`}
                aria-label={`${$t("pia.toggleLine")} ${lineName(p.c2)}`}
              >
                <span class="led"></span>
                <span class="ctl-name">{lineName(p.c2)}</span>
                <span class="ctl-lvl mono">{p.c2Level ? "H" : "L"}</span>
              </button>
            {/if}
            {#if p.strobes > 0}
              <span class="strobes mono" title={$t("pia.strobes")}>{p.strobes}×</span>
            {/if}
          </div>

          <div class="port-meta">
            <span class="meta-item">
              <span class="meta-label">{$t("pia.ddr")}</span>
              <span class="meta-val mono">{fmtByte(p.ddr)}</span>
            </span>
            <span class="meta-item">
              <span class="meta-label">{$t(p.crKey)}</span>
              <span class="meta-val mono">{fmtByte(p.cr)}</span>
            </span>
            <span class="flag" class:on={p.flag1} title={$t("pia.flag1")}>IRQ1</span>
            <span class="flag" class:on={p.flag2} title={$t("pia.flag2")}>IRQ2</span>
            {#if p.irq}
              <span class="irq-badge on" title={$t("pia.irqOutput")}>{$t("pia.irq")}</span>
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

  /* Control lines CA1/CA2 or CB1/CB2 */
  .ctl-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .ctl-line {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-0);
    color: var(--text-dim);
    font-size: 10px;
    line-height: 1;
    cursor: default;
    transition:
      border-color var(--motion-normal) ease,
      background var(--motion-normal) ease;
  }

  button.ctl-line {
    cursor: pointer;
  }

  button.ctl-line:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--bg-hover);
  }

  button.ctl-line:focus-visible {
    outline: none;
    border-color: var(--accent-dim);
    box-shadow: var(--ring);
  }

  .ctl-line .led {
    width: 9px;
    height: 9px;
  }

  .ctl-line.input.on .led {
    background: var(--amber);
    border-color: var(--amber);
    box-shadow: 0 0 5px color-mix(in srgb, var(--amber) 40%, transparent);
  }

  .ctl-line.output.on .led {
    background: var(--accent);
    border-color: var(--accent-dim);
    box-shadow: 0 0 5px var(--accent-soft);
  }

  .ctl-name {
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--text);
  }

  .ctl-lvl {
    color: var(--text-faint);
  }

  .ctl-dir {
    font-family: var(--font-mono);
    font-size: 8px;
    font-weight: 700;
    color: var(--accent);
  }

  .strobes {
    font-size: 10px;
    color: var(--text-faint);
  }

  .flag {
    font-family: var(--font-mono);
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.04em;
    padding: 1px 4px;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--text-faint);
  }

  .flag.on {
    color: var(--amber);
    border-color: var(--amber);
  }
</style>
