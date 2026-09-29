<script lang="ts">
  import { untrack } from "svelte";
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtAddr } from "../format";
  import type { AciaTerminalState } from "../types";

  let {
    terminal,
    baseAddr,
    capsLockDefault = false,
    onSend,
    onClear,
    onClose,
  }: {
    terminal: AciaTerminalState | null;
    baseAddr: number;
    capsLockDefault?: boolean;
    onSend: (text: string) => void;
    onClear?: () => void;
    onClose?: () => void;
  } = $props();

  // Initial value only; the user toggles caps lock afterwards.
  let capsLock = $state(untrack(() => capsLockDefault));
  let inputText = $state("");
  let focused = $state(false);
  let composing = $state(false);
  let ghost: HTMLTextAreaElement | undefined = $state();
  let outputEl: HTMLPreElement | undefined = $state();
  let crtEl: HTMLDivElement | undefined = $state();

  $effect(() => {
    capsLock = capsLockDefault;
  });

  function focusGhost() {
    ghost?.focus({ preventScroll: true });
  }

  // `terminal.tx_text` arrives already rendered by the ACIA terminal buffer
  // (BS erases, CR returns to column 0, LF = new line, bytes >= $80 decoded as
  // Latin-1), so it is shown verbatim.

  let stickToBottom = true;

  function applyCaps(text: string): string {
    if (!capsLock) return text;
    // Upper-case per character, but only where the result stays one Latin-1 char.
    return Array.from(text, (ch) => {
      const up = ch.toUpperCase();
      return up.length === 1 && (up.codePointAt(0) ?? 0) <= 0xff ? up : ch;
    }).join("");
  }

  /** The serial line carries Latin-1 bytes: line breaks become CR, characters
   *  above U+00FF are sent as "?". */
  function toSerial(text: string): string {
    return Array.from(text.replace(/\r\n?|\n/g, "\r"), (ch) =>
      (ch.codePointAt(0) ?? 0) <= 0xff ? ch : "?"
    ).join("");
  }

  function sendRaw(text: string) {
    if (!text) return;
    stickToBottom = true;
    onSend(toSerial(applyCaps(text)));
  }

  function handleGhostKeydown(event: KeyboardEvent) {
    if (composing) return;
    if (event.ctrlKey && (event.key === "c" || event.key === "C")) {
      event.preventDefault();
      sendRaw("\x03");
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      inputText = "";
      sendRaw("\r");
      return;
    }
    if (event.key === "Backspace") {
      event.preventDefault();
      inputText = "";
      sendRaw("\x08");
      return;
    }
    if (event.key === "Tab" && !event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      sendRaw("\t");
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      sendRaw("\x1b");
    }
  }

  function handleGhostInput() {
    if (composing) return;
    const raw = inputText;
    inputText = "";
    sendRaw(raw);
  }

  function handleCompositionEnd() {
    composing = false;
    const raw = inputText;
    inputText = "";
    sendRaw(raw);
  }

  function handleCrtPointerUp() {
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && crtEl?.contains(sel.anchorNode)) {
      return;
    }
    focusGhost();
  }

  function handleOutputScroll() {
    const el = outputEl;
    if (!el) return;
    // Follow new output only while the view is at (or near) the bottom.
    stickToBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  function scrollToEnd() {
    const el = outputEl;
    if (!el || !stickToBottom) return;
    el.scrollTop = el.scrollHeight;
  }

  let lastTextLength = 0;

  $effect(() => {
    const length = terminal?.tx_text.length ?? 0;
    if (length < lastTextLength) stickToBottom = true; // cleared
    lastTextLength = length;
    queueMicrotask(scrollToEnd);
  });
</script>

<div class="panel terminal-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("acia.title")}<span class="base mono">{fmtAddr(baseAddr)}</span></span>
    <div class="ph-actions">
      <label class="caps-toggle status mono" class:on={capsLock} title={$t("acia.capsHint")}>
        <input type="checkbox" bind:checked={capsLock} aria-label={$t("acia.caps")} />
        {$t("acia.caps")}
      </label>
      {#if terminal}
        <span class="status mono" class:on={terminal.rdrf} title={$t("acia.rdrf")}>RDRF</span>
        <span class="status mono" class:on={terminal.tdre} title={$t("acia.tdre")}>TDRE</span>
        <span class="status mono" class:on={terminal.irq} title={$t("acia.irq")}>IRQ</span>
      {/if}
      {#if onClear}
        <button class="hdr-btn" onclick={onClear} title={$t("acia.clear")} aria-label={$t("acia.clear")}>
          <Icon name="clear" size={13} />
        </button>
      {/if}
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body">
    <!-- The hidden textarea inside is the keyboard target; a click anywhere on the screen focuses it. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="crt"
      class:focused
      bind:this={crtEl}
      onclick={handleCrtPointerUp}
    >
      <pre class="output mono" bind:this={outputEl} onscroll={handleOutputScroll}><span class="tx">{terminal?.tx_text ?? ""}</span><span class="cursor" class:blink={focused} aria-hidden="true"> </span></pre>
      <textarea
        class="ghost"
        bind:this={ghost}
        bind:value={inputText}
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        rows="1"
        aria-label={$t("acia.inputLabel")}
        onkeydown={handleGhostKeydown}
        oninput={handleGhostInput}
        oncompositionstart={() => (composing = true)}
        oncompositionend={handleCompositionEnd}
        onfocus={() => (focused = true)}
        onblur={() => (focused = false)}
      ></textarea>
      <div class="scanlines" aria-hidden="true"></div>
      <div class="crt-glow" aria-hidden="true"></div>
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

  .caps-toggle {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
    user-select: none;
  }

  .caps-toggle input {
    margin: 0;
    accent-color: var(--accent);
  }

  .panel-body {
    display: flex;
    flex-direction: column;
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
    cursor: text;
  }

  .crt.focused {
    box-shadow:
      inset 0 0 32px rgba(0, 0, 0, 0.45),
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
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

  .cursor {
    display: inline-block;
    min-width: 0.55em;
    background: color-mix(in srgb, var(--crt-phosphor) 38%, transparent);
    color: var(--crt-phosphor);
    text-shadow: none;
    border-radius: 1px;
  }

  .cursor.blink {
    background: var(--crt-phosphor);
    color: var(--crt-bg);
    animation: cursor-blink 1.05s step-end infinite;
  }

  @keyframes cursor-blink {
    50% {
      background: transparent;
      color: var(--crt-phosphor);
      text-shadow: 0 0 5px var(--crt-glow);
    }
  }

  .ghost {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    border: 0;
    opacity: 0;
    overflow: hidden;
    resize: none;
    clip: rect(0, 0, 0, 0);
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
</style>
