<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import type { AciaTerminalState } from "../types";

  let {
    terminal,
    baseAddr,
    onSend,
    onClose,
  }: {
    terminal: AciaTerminalState | null;
    baseAddr: number;
    onSend: (text: string) => void;
    onClose?: () => void;
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

<div class="panel terminal-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("acia.title")}<span class="base mono">{fmtAddr(baseAddr)}</span></span>
    <div class="ph-actions">
      {#if terminal}
        <span class="status mono" class:on={terminal.rdrf} title={$t("acia.rdrf")}>RDRF</span>
        <span class="status mono" class:on={terminal.tdre} title={$t("acia.tdre")}>TDRE</span>
        <span class="status mono" class:on={terminal.irq} title={$t("acia.irq")}>IRQ</span>
      {/if}
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body">
    <div class="crt">
      <pre class="output mono">{terminal?.tx_text ?? ""}</pre>
      <div class="scanlines" aria-hidden="true"></div>
      <div class="crt-glow" aria-hidden="true"></div>
    </div>
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

  .ph-title {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .base {
    color: var(--accent);
    font-size: 10.5px;
    font-weight: 500;
  }

  .ph-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .status {
    font-size: 9.5px;
    padding: 2px 6px;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--text-faint);
    letter-spacing: 0.04em;
  }

  .status.on {
    color: var(--accent);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .panel-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    flex: 1;
    padding: 8px;
  }

  .crt {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--crt-bg);
    border: 1px solid var(--crt-border);
    border-radius: 4px;
    overflow: hidden;
    box-shadow: inset 0 0 32px rgba(0, 0, 0, 0.45);
  }

  .output {
    position: relative;
    margin: 0;
    height: 100%;
    overflow: auto;
    font-size: 12px;
    line-height: 1.45;
    color: var(--crt-phosphor);
    text-shadow: 0 0 5px var(--crt-glow);
    padding: 10px 12px;
    white-space: pre-wrap;
    word-break: break-all;
    z-index: 1;
  }

  .scanlines {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: repeating-linear-gradient(
      to bottom,
      var(--crt-scanline) 0px,
      var(--crt-scanline) 1px,
      transparent 1px,
      transparent 3px
    );
    mix-blend-mode: multiply;
    z-index: 2;
  }

  .crt-glow {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: radial-gradient(120% 100% at 50% 50%, transparent 55%, rgba(0, 0, 0, 0.5) 100%);
    z-index: 3;
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
    background: var(--bg-0);
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
