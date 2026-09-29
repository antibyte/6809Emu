<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtAddr, fmtByte } from "../format";
  import type { AyState } from "../types";

  let {
    state,
    muted,
    onToggleMute,
    onClose,
  }: {
    state: AyState | null;
    muted: boolean;
    onToggleMute: () => void;
    onClose?: () => void;
  } = $props();

  const REG_LABELS = [
    "R0  ToneA F",
    "R1  ToneA C",
    "R2  ToneB F",
    "R3  ToneB C",
    "R4  ToneC F",
    "R5  ToneC C",
    "R6  Noise",
    "R7  Mix/IO",
    "R8  AmpA",
    "R9  AmpB",
    "R10 AmpC",
    "R11 Env F",
    "R12 Env C",
    "R13 Shape",
    "R14 PortA",
    "R15 PortB",
  ] as const;

  function tonePeriod(regs: number[], ch: number): number {
    const fine = regs[ch * 2] & 0xFF;
    const coarse = regs[ch * 2 + 1] & 0x0F;
    return ((coarse << 8) | fine) & 0xFFF;
  }

  function toneHz(regs: number[], ch: number, clock: number): number {
    const period = Math.max(tonePeriod(regs, ch), 1);
    return Math.round(clock / 16 / period);
  }

  function amp(regs: number[], ch: number): number {
    return regs[8 + ch] & 0x0F;
  }

  function envEnabled(regs: number[], ch: number): boolean {
    return (regs[8 + ch] & 0x10) !== 0;
  }

  function mixer(regs: number[]): number {
    return regs[7] & 0x3F;
  }

  function toneOn(regs: number[], ch: number): boolean {
    return (regs[7] & (1 << ch)) === 0;
  }

  function noiseOn(regs: number[], ch: number): boolean {
    return (regs[7] & (1 << (ch + 3))) === 0;
  }

  function portAOutput(regs: number[]): boolean {
    return (regs[7] & 0x40) !== 0;
  }

  function portBOutput(regs: number[]): boolean {
    return (regs[7] & 0x80) !== 0;
  }

  function envPeriod(regs: number[]): number {
    return ((regs[12] << 8) | regs[11]) & 0xFFFF;
  }

  /** Envelope ramp frequency: fE = fCLK / (256 * EP), EP = 0 acts as 1. */
  function envHz(regs: number[], clock: number): string {
    const hz = clock / 256 / Math.max(envPeriod(regs), 1);
    return hz >= 100 ? hz.toFixed(0) : hz.toFixed(2);
  }

  /** Noise frequency: fN = fCLK / (16 * NP), NP = 0 acts as 1. */
  function noiseHz(regs: number[], clock: number): number {
    return Math.round(clock / 16 / Math.max(regs[6] & 0x1F, 1));
  }

  /** Set R13 bits by their datasheet names (bit3 CONT, bit2 ATT, bit1 ALT, bit0 HOLD). */
  function envShapeBits(shape: number): string {
    const names = ["HOLD", "ALT", "ATT", "CONT"];
    const set = names.filter((_, bit) => (shape & (1 << bit)) !== 0).reverse();
    return set.length ? set.join(" ") : "-";
  }

  /** Datasheet envelope drawing; CONT=0 shapes run one ramp, then hold at 0. */
  function envGlyph(shape: number): string {
    switch (shape & 0x0F) {
      case 0x08:
        return "\\\\\\\\";
      case 0x0A:
        return "\\/\\/";
      case 0x0B:
        return "\\‾‾‾";
      case 0x0C:
        return "////";
      case 0x0D:
        return "/‾‾‾";
      case 0x0E:
        return "/\\/\\";
      case 0x04:
      case 0x05:
      case 0x06:
      case 0x07:
      case 0x0F:
        return "/___";
      default:
        return "\\___"; // $00-$03, $09
    }
  }
</script>

<div class="panel ay-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("ay.title")}
      {#if state}
        <span class="addr mono">{fmtAddr(state.config.base_addr)}</span>
      {/if}
    </span>
    <div class="ph-actions">
      <button
        class="hdr-btn"
        class:active={!muted}
        onclick={onToggleMute}
        title={muted ? $t("ay.audioOn") : $t("ay.audioMute")}
        aria-label={muted ? $t("ay.audioOn") : $t("ay.audioMute")}
      >
        <Icon name={muted ? "view" : "registers"} size={13} />
      </button>
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>

  <div class="panel-body ay-body">
    {#if !state}
      <div class="empty-line">{$t("ay.chipOff")}</div>
    {:else}
      <div class="reg-table">
        {#each REG_LABELS as label, i}
          <div class="reg-row" class:active={state.chip_selected !== false && i === state.selected_register}>
            <span class="reg-idx mono">{label}</span>
            <span class="reg-val mono">{fmtByte(state.registers[i] ?? 0)}</span>
          </div>
        {/each}
      </div>

      <div class="ay-summary">
        <div class="summary-row">
          <span class="sum-label">{$t("ay.tonePeriod")}</span>
          <span class="sum-ch mono">{toneHz(state.registers, 0, state.config.chip_clock_hz)} Hz</span>
          <span class="sum-ch mono">{toneHz(state.registers, 1, state.config.chip_clock_hz)} Hz</span>
          <span class="sum-ch mono">{toneHz(state.registers, 2, state.config.chip_clock_hz)} Hz</span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.amplitude")}</span>
          <span class="sum-ch mono">{amp(state.registers, 0)}{envEnabled(state.registers, 0) ? " Env" : ""}</span>
          <span class="sum-ch mono">{amp(state.registers, 1)}{envEnabled(state.registers, 1) ? " Env" : ""}</span>
          <span class="sum-ch mono">{amp(state.registers, 2)}{envEnabled(state.registers, 2) ? " Env" : ""}</span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.tone")}</span>
          <span class="sum-ch" class:on={toneOn(state.registers, 0)}>A</span>
          <span class="sum-ch" class:on={toneOn(state.registers, 1)}>B</span>
          <span class="sum-ch" class:on={toneOn(state.registers, 2)}>C</span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.noise")}</span>
          <span class="sum-ch" class:on={noiseOn(state.registers, 0)}>A</span>
          <span class="sum-ch" class:on={noiseOn(state.registers, 1)}>B</span>
          <span class="sum-ch" class:on={noiseOn(state.registers, 2)}>C</span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.noiseFreq")}</span>
          <span class="sum-ch mono wide">
            NP {state.registers[6] & 0x1F} &middot; {noiseHz(state.registers, state.config.chip_clock_hz)} Hz
          </span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.envelope")}</span>
          <span class="sum-ch mono wide" title={envShapeBits(state.registers[13])}>
            <span class="env-glyph">{envGlyph(state.registers[13])}</span>
            {fmtAddr(envPeriod(state.registers))} &middot; {envHz(state.registers, state.config.chip_clock_hz)} Hz
            &middot; {$t("ay.envLevel")} {state.envelope_level ?? "-"}
          </span>
        </div>
        <div class="summary-row">
          <span class="sum-label">{$t("ay.latch")}</span>
          <span class="sum-ch mono wide" class:off={state.chip_selected === false}>
            {fmtByte(state.address_latch ?? state.selected_register)}
            {#if state.chip_selected === false}
              &middot; {$t("ay.deselected")}
            {:else}
              &rarr; R{state.selected_register}
            {/if}
          </span>
        </div>
      </div>

      <div class="ay-ports">
        <div class="port-line">
          <span class="port-name">{$t("ay.portA")}</span>
          <span class="port-dir">{portAOutput(state.registers) ? "OUT" : "IN"}</span>
          <span class="mono">{fmtByte(portAOutput(state.registers) ? state.registers[14] : state.port_a_in)}</span>
        </div>
        <div class="port-line">
          <span class="port-name">{$t("ay.portB")}</span>
          <span class="port-dir">{portBOutput(state.registers) ? "OUT" : "IN"}</span>
          <span class="mono">{fmtByte(portBOutput(state.registers) ? state.registers[15] : state.port_b_in)}</span>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .ay-panel {
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

  .hdr-btn.active {
    color: var(--accent);
  }

  .ay-body {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow: auto;
  }

  .empty-line {
    color: var(--text-faint);
    font-size: 11px;
    padding: 8px 4px;
  }

  .reg-table {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 2px 8px;
  }

  .reg-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 6px;
    border-radius: var(--radius-sm);
    background: var(--bg-0);
    border: 1px solid transparent;
    font-size: 10.5px;
  }

  .reg-row.active {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .reg-idx {
    color: var(--text-dim);
    font-size: 10px;
  }

  .reg-val {
    color: var(--text);
    font-weight: 600;
  }

  .ay-summary {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .summary-row {
    display: grid;
    grid-template-columns: 70px repeat(3, 1fr);
    gap: 4px;
    align-items: center;
    font-size: 10.5px;
  }

  .summary-row .sum-label {
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 9px;
    font-weight: 600;
  }

  .sum-ch {
    color: var(--text);
    text-align: center;
    padding: 2px 4px;
    border-radius: 3px;
    background: var(--bg-1);
    font-size: 10px;
  }

  .sum-ch.on {
    color: var(--accent);
    font-weight: 700;
    background: var(--accent-soft);
  }

  .sum-ch.wide {
    grid-column: 2 / -1;
    text-align: left;
  }

  .sum-ch.off {
    color: var(--text-faint);
  }

  .env-glyph {
    color: var(--accent);
    font-weight: 700;
    margin-right: 6px;
    letter-spacing: 0.05em;
  }

  .ay-ports {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .port-line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10.5px;
  }

  .port-name {
    color: var(--text-dim);
    min-width: 50px;
  }

  .port-dir {
    font-family: var(--font-mono);
    font-size: 9px;
    font-weight: 700;
    color: var(--accent);
    background: var(--accent-soft);
    padding: 1px 4px;
    border-radius: 3px;
  }

  .ay-ports .mono {
    color: var(--text);
    font-weight: 600;
  }
</style>
