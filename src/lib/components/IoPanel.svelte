<script lang="ts">
  import { t } from "../i18n";
  import CollapseButton from "./CollapseButton.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { fmtAddr, fmtByte, toHex } from "../format";
  import type { IoRegister, MachineKind } from "../types";

  let {
    kind,
    registers,
    collapsed = false,
    onToggleCollapse,
    onGoto,
    onWrite,
  }: {
    kind: MachineKind;
    registers: IoRegister[];
    collapsed?: boolean;
    onToggleCollapse?: () => void;
    onGoto: (addr: number) => void;
    onWrite: (addr: number, value: number) => void;
  } = $props();

  let editing: { address: number; raw: string } | null = $state(null);
  let editInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (editing && editInput) {
      editInput.focus();
      editInput.select();
    }
  });

  function startEdit(reg: IoRegister) {
    editing = { address: reg.address, raw: toHex(reg.value, 2) };
  }

  function commitEdit() {
    if (!editing) return;
    const parsed = parseInt(editing.raw.replace(/[^0-9A-Fa-f]/g, ""), 16);
    if (!Number.isNaN(parsed) && parsed >= 0 && parsed <= 0xff) {
      onWrite(editing.address, parsed);
    }
    editing = null;
  }

  function cancelEdit() {
    editing = null;
  }

  const kindLabel = $derived(
    kind === "coco2"
      ? $t("machine.coco2")
      : kind === "dragon32"
        ? $t("machine.dragon32")
        : $t("machine.bare")
  );
</script>

<div class="panel io-panel panel-secondary" class:collapsed>
  <div class="panel-header">
    <span class="ph-title">
      {$t("machine.ioTitle")}
      <span class="kind">{kindLabel}</span>
    </span>
    {#if onToggleCollapse}
      <CollapseButton {collapsed} onclick={onToggleCollapse} />
    {/if}
  </div>
  <div class="panel-body io-list">
    {#if registers.length === 0}
      <EmptyState icon="io" message={$t("machine.ioEmpty")} />
    {:else}
      {#each registers as reg}
        <div class="io-entry">
          <button class="addr mono" onclick={() => onGoto(reg.address)} title={$t("machine.ioGoto")}>
            {fmtAddr(reg.address)}
          </button>
          <span class="name">{reg.name}</span>
          {#if editing?.address === reg.address}
            <input
              bind:this={editInput}
              class="value-edit mono"
              bind:value={editing.raw}
              onkeydown={(e) => {
                if (e.key === "Enter") commitEdit();
                if (e.key === "Escape") cancelEdit();
              }}
              onblur={commitEdit}
            />
          {:else}
            <button
              class="value mono"
              onclick={() => startEdit(reg)}
              title={$t("machine.ioEdit")}
            >
              {fmtByte(reg.value)}
            </button>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .io-panel {
    height: 100%;
    min-height: 0;
  }

  .io-panel.collapsed .panel-body {
    display: none;
  }

  .io-panel .ph-title {
    display: inline-flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .kind {
    font-size: 10px;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 500;
  }

  .io-list {
    padding: 0;
    font-size: 11px;
    max-height: none;
    overflow: auto;
    min-height: 0;
  }

  .io-entry {
    display: grid;
    grid-template-columns: 64px 1fr 48px;
    gap: 6px;
    padding: 4px 10px;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }

  .io-entry:hover {
    background: var(--accent-soft);
  }

  .addr {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    text-align: left;
    font-size: 11px;
  }

  .addr:hover {
    text-decoration: underline;
  }

  .name {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .value {
    background: none;
    border: none;
    text-align: right;
    color: var(--text);
    cursor: pointer;
    padding: 0;
    font-size: 11px;
    width: 100%;
  }

  .value:hover {
    color: var(--accent);
  }

  .value-edit {
    background: var(--bg-deep);
    border: 1px solid var(--accent);
    color: var(--text);
    text-align: right;
    font-size: 11px;
    padding: 1px 4px;
    border-radius: 2px;
  }
</style>