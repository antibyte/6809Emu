<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { getInstructionDoc, type InstructionDoc } from "../asm";

  let {
    open,
    mnemonic,
    onClose,
  }: {
    open: boolean;
    mnemonic: string | null;
    onClose: () => void;
  } = $props();

  const doc = $derived(mnemonic ? getInstructionDoc(mnemonic) : undefined);

  function fmtMnem(m: string | null) {
    return m ? m.toUpperCase() : "";
  }

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open && mnemonic}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="backdrop" onclick={onClose} role="presentation">
    <div
      class="modal instruction-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="insn-modal-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="modal-header">
        <div class="title-group">
          <h2 id="insn-modal-title">{fmtMnem(mnemonic)}</h2>
          {#if doc?.variant && doc.variant !== "both"}
            <span class="variant mono">{doc.variant}</span>
          {/if}
        </div>
        <button class="close-btn" onclick={onClose} aria-label={$t("panels.close")}>
          <Icon name="close" size={14} />
        </button>
      </header>

      <div class="modal-body">
        {#if !doc}
          <p class="no-doc">No detailed documentation available for {fmtMnem(mnemonic)}.</p>
          <p class="hint">See the assembler or run the program to test behavior.</p>
        {:else}
          <p class="desc">{doc.desc}</p>

          <h3>Syntax</h3>
          <ul class="syntax">
            {#each doc.syntax as s}
              <li><code>{s}</code></li>
            {/each}
          </ul>

          <div class="meta">
            <div><strong>Flags:</strong> {doc.flags}</div>
            <div><strong>Cycles:</strong> {doc.cycles}</div>
          </div>

          {#if doc.notes}
            <p class="notes">{doc.notes}</p>
          {/if}
        {/if}
      </div>

      <footer class="modal-footer">
        <small>6809 / HD6309 reference • Press Esc to close</small>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 4500;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(4, 8, 12, 0.78);
    backdrop-filter: blur(4px);
  }

  .modal {
    width: min(92vw, 520px);
    background: var(--bg-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-2);
  }

  .title-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-family: var(--font-mono);
  }

  .variant {
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 3px;
    background: var(--accent-soft);
    color: var(--accent);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    padding: 2px;
  }

  .modal-body {
    padding: 12px 14px;
    font-size: 13px;
    line-height: 1.45;
  }

  .desc {
    margin: 0 0 10px;
    color: var(--text);
  }

  h3 {
    font-size: 11px;
    margin: 10px 0 4px;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .syntax {
    margin: 0 0 8px;
    padding-left: 18px;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .meta {
    display: flex;
    gap: 16px;
    margin: 8px 0;
    font-size: 12px;
  }

  .notes {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-dim);
  }

  .no-doc {
    color: var(--text-dim);
    font-style: italic;
  }

  .hint {
    font-size: 11px;
    color: var(--text-faint);
  }

  .modal-footer {
    padding: 6px 14px;
    font-size: 10px;
    color: var(--text-faint);
    border-top: 1px solid var(--border);
    background: var(--bg-0);
  }
</style>
