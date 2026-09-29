<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fmtAddr, fmtByte } from "../format";
  import type { SpeechState } from "../types";

  let {
    speech,
    muted,
    onToggleMute,
    onSay,
    onClose,
    onGreetingChange,
  }: {
    speech: SpeechState | null;
    muted: boolean;
    onToggleMute: () => void;
    onSay: (text: string) => void | Promise<void>;
    onClose?: () => void;
    /** Optional: toggle the CTS256 "O.K." greeting after reset. */
    onGreetingChange?: (on: boolean) => void | Promise<void>;
  } = $props();

  // SP0256-AL2 allophone mnemonics, indexed by allophone address.
  // prettier-ignore
  const ALLOPHONES = [
    "PA1", "PA2", "PA3", "PA4", "PA5", "OY",  "AY",  "EH",
    "KK3", "PP",  "JH",  "NN1", "IH",  "TT2", "RR1", "AX",
    "MM",  "TT1", "DH1", "IY",  "EY",  "DD1", "UW1", "AO",
    "AA",  "YY2", "AE",  "HH1", "BB1", "TH",  "UH",  "UW2",
    "AW",  "DD2", "GG3", "VV",  "GG1", "SH",  "ZH",  "RR2",
    "FF",  "KK2", "KK1", "ZZ",  "NG",  "LL",  "WW",  "XR",
    "WH",  "YY1", "CH",  "ER1", "ER2", "OW",  "DH2", "SS",
    "NN2", "HH2", "OR",  "AR",  "YR",  "GG2", "EL",  "BB2",
  ];

  function alloName(code: number): string {
    return ALLOPHONES[code & 0x3f] ?? "?";
  }

  let sayText = $state("");
  let saying = $state(false);

  const ctsPhase = $derived(
    !speech ? "idle" : speech.cts_booting ? "booting" : speech.cts_busy ? "busy" : "idle"
  );
  const recent = $derived((speech?.cts_recent ?? []).map(alloName).join(" "));
  const crOnly = $derived(speech?.cts_cr_only ?? true);

  async function submitSay() {
    const text = sayText.trim();
    if (!text || saying) return;
    saying = true;
    try {
      await onSay(text);
    } finally {
      saying = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      submitSay();
    }
  }
</script>

<div class="panel speech-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("speech.title")}
      {#if speech}
        <span class="addr mono">{fmtAddr(speech.config.base_addr)}</span>
      {/if}
    </span>
    <div class="ph-actions">
      <button
        class="hdr-btn"
        class:active={!muted}
        onclick={onToggleMute}
        title={muted ? $t("speech.audioOn") : $t("speech.audioMute")}
        aria-label={muted ? $t("speech.audioOn") : $t("speech.audioMute")}
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

  <div class="panel-body speech-body">
    {#if !speech}
      <div class="empty-line">{$t("speech.chipOff")}</div>
    {:else}
      <div class="status-grid">
        <div class="status-cell" class:on={speech.lrq_ready}>
          <span class="sc-label">{$t("speech.lrq")}</span>
          <span class="sc-val">{speech.lrq_ready ? $t("speech.ready") : $t("speech.busy")}</span>
        </div>
        <div class="status-cell" class:on={speech.standby}>
          <span class="sc-label">{$t("speech.sby")}</span>
          <span class="sc-val">{speech.standby ? $t("speech.idle") : $t("speech.speaking")}</span>
        </div>
        <div class="status-cell" class:on={speech.speaking}>
          <span class="sc-label">{$t("speech.output")}</span>
          <span class="sc-val">{speech.speaking ? $t("speech.speaking") : $t("speech.idle")}</span>
        </div>
        <div class="status-cell">
          <span class="sc-label">{$t("speech.lastAllophone")}</span>
          <span class="sc-val mono">
            {fmtByte(speech.last_allophone)}
            <span class="allo">{alloName(speech.last_allophone)}</span>
          </span>
        </div>
      </div>

      {#if speech.cts_enabled}
        <div class="cts-line">
          <span class="cts-name">{$t("speech.cts")}</span>
          <span class="cts-dir" class:busy={ctsPhase === "busy"} class:booting={ctsPhase === "booting"}>
            {ctsPhase === "booting"
              ? $t("speech.booting")
              : ctsPhase === "busy"
                ? $t("speech.busy")
                : $t("speech.idle")}
          </span>
          <span class="mono" title={$t("speech.ctsLast")}>
            {fmtByte(speech.cts_last_allophone)}
            <span class="allo">{alloName(speech.cts_last_allophone)}</span>
          </span>
        </div>

        <div class="cts-meta">
          <span class="meta-mode">{crOnly ? $t("speech.modeCr") : $t("speech.modeAny")}</span>
          <span>
            <span class="meta-k">{$t("speech.inputPending")}</span>
            <span class="mono">{speech.cts_input_pending ?? 0}</span>
          </span>
          <span>
            <span class="meta-k">{$t("speech.outputPending")}</span>
            <span class="mono">{speech.cts_output_pending ?? 0}</span>
          </span>
        </div>

        {#if speech.cts_buffer_full}
          <div class="warn-line">{$t("speech.bufferFull")}</div>
        {/if}
        {#if speech.cts_fifo_full}
          <div class="warn-line">{$t("speech.fifoFull")}</div>
        {/if}

        <div class="recent">
          <span class="sc-label">{$t("speech.recent")}</span>
          <span class="recent-list mono">{recent || "—"}</span>
        </div>

        <div class="say-box">
          <input
            class="say-input mono"
            type="text"
            bind:value={sayText}
            onkeydown={onKeydown}
            placeholder={$t("speech.sayPlaceholder")}
            aria-label={$t("speech.sayPlaceholder")}
          />
          <button
            class="say-btn"
            onclick={submitSay}
            disabled={saying || sayText.trim().length === 0}
          >
            {$t("speech.sayButton")}
          </button>
        </div>
        <div class="hint">{crOnly ? $t("speech.crHint") : $t("speech.anyHint")}</div>
        {#if onGreetingChange}
          <label class="greet">
            <input
              type="checkbox"
              checked={speech.cts_greeting ?? true}
              onchange={(e) => void onGreetingChange?.((e.target as HTMLInputElement).checked)}
            />
            <span>{$t("speech.greeting")}</span>
          </label>
        {/if}
      {:else}
        <div class="hint">{$t("speech.ctsOff")}</div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .speech-panel {
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

  .speech-body {
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

  .status-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }

  .status-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 5px 7px;
    border-radius: var(--radius-sm);
    background: var(--bg-0);
    border: 1px solid transparent;
  }

  .status-cell.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .sc-label {
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 9px;
    font-weight: 600;
  }

  .sc-val {
    color: var(--text);
    font-size: 11px;
    font-weight: 600;
  }

  .allo {
    color: var(--text-dim);
    font-weight: 500;
    margin-left: 4px;
  }

  .cts-line {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10.5px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .cts-name {
    color: var(--text-dim);
    min-width: 44px;
  }

  .cts-dir {
    font-family: var(--font-mono);
    font-size: 9px;
    font-weight: 700;
    color: var(--accent);
    background: var(--accent-soft);
    padding: 1px 5px;
    border-radius: 3px;
  }

  .cts-dir.busy,
  .cts-dir.booting {
    color: var(--warn, #d98a3d);
    background: color-mix(in srgb, var(--warn, #d98a3d) 18%, transparent);
  }

  .cts-line .mono {
    color: var(--text);
    font-weight: 600;
  }

  .cts-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    font-size: 10px;
    color: var(--text-dim);
  }

  .meta-mode {
    color: var(--text-faint);
  }

  .meta-k {
    color: var(--text-faint);
    margin-right: 4px;
  }

  .warn-line {
    font-size: 10px;
    color: var(--warn, #d98a3d);
  }

  .recent {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 5px 7px;
    border-radius: var(--radius-sm);
    background: var(--bg-0);
  }

  .recent-list {
    color: var(--text);
    font-size: 10.5px;
    line-height: 1.5;
    word-break: break-word;
  }

  .say-box {
    display: flex;
    gap: 6px;
  }

  .say-input {
    flex: 1;
    min-width: 0;
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 5px 7px;
    font-size: 11px;
  }

  .say-input:focus {
    outline: none;
    border-color: var(--accent-line);
  }

  .say-btn {
    background: var(--accent-soft);
    color: var(--accent);
    border: 1px solid var(--accent-line);
    border-radius: var(--radius-sm);
    padding: 5px 12px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }

  .say-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .hint {
    color: var(--text-faint);
    font-size: 9.5px;
    line-height: 1.4;
  }

  .greet {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    color: var(--text-dim);
    cursor: pointer;
  }
</style>
