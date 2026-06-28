<script lang="ts">
  import { t } from "../i18n";
  import type { AciaTerminalState } from "../types";

  let {
    terminal,
    baseAddr,
    onSend,
  }: {
    terminal: AciaTerminalState | null;
    baseAddr: number;
    onSend: (text: string) => void;
  } = $props();

  let inputText = $state("");

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }

  function handleSend() {
    if (!inputText) return;
    onSend(inputText);
    inputText = "";
  }

  function handleInputKeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      handleSend();
    }
  }
</script>

<div class="panel terminal-panel panel-secondary">
  <div class="panel-header">
    <div class="title-group">
      <span>{$t("acia.title")}</span>
      <span class="base mono">{fmtAddr(baseAddr)}</span>
    </div>
    {#if terminal}
      <div class="status-row">
        <span class="status mono" class:on={terminal.rdrf} title={$t("acia.rdrf")}>RDRF</span>
        <span class="status mono" class:on={terminal.tdre} title={$t("acia.tdre")}>TDRE</span>
        <span class="status mono" class:on={terminal.irq} title={$t("acia.irq")}>IRQ</span>
      </div>
    {/if}
  </div>
  <div class="panel-body">
    <pre class="output mono">{terminal?.tx_text ?? ""}</pre>
    <div class="input-row">
      <input
        class="mono"
        type="text"
        bind:value={inputText}
        placeholder={$t("acia.inputPlaceholder")}
        onkeydown={handleInputKeydown}
        aria-label={$t("acia.inputLabel")}
      />
      <button onclick={handleSend} disabled={!inputText}>{$t("acia.send")}</button>
    </div>
  </div>
</div>

<style>
  .terminal-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .title-group {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .base {
    color: var(--accent);
    font-size: 11px;
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--text-dim);
    opacity: 0.5;
  }

  .status.on {
    opacity: 1;
    color: var(--accent);
    border-color: rgba(57, 255, 20, 0.35);
    background: rgba(57, 255, 20, 0.08);
  }

  .panel-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    flex: 1;
  }

  .output {
    margin: 0;
    flex: 1;
    min-height: 0;
    overflow: auto;
    font-size: 12px;
    line-height: 1.45;
    color: #93c5fd;
    background: #0a1018;
    border: 1px solid rgba(96, 165, 250, 0.2);
    border-radius: 4px;
    padding: 10px 12px;
    box-shadow: inset 0 0 32px rgba(0, 0, 0, 0.35);
    white-space: pre-wrap;
    word-break: break-all;
  }

  .input-row {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }

  .input-row input {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    background: var(--bg-deep);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    font-size: 12px;
  }

  .input-row input:focus {
    outline: none;
    border-color: var(--accent-dim);
  }

  .input-row button {
    flex-shrink: 0;
  }
</style>
