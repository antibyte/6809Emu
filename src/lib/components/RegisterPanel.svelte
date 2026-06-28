<script lang="ts">
  import type { CpuState } from "../types";
  import { t } from "../i18n";

  let {
    cpu,
    onSetRegister,
    onToggleFlag,
  }: {
    cpu: CpuState | null;
    onSetRegister: (register: string, value: number) => void;
    onToggleFlag: (flag: string) => void;
  } = $props();

  let editing: { name: string; bits: 8 | 16; raw: string } | null = $state(null);
  let editInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (editing && editInput) {
      editInput.focus();
      editInput.select();
    }
  });

  const isHd6309 = $derived(cpu?.variant === "hd6309");

  const regs16 = $derived(
    cpu
      ? [
          { label: "D", value: cpu.d },
          ...(isHd6309
            ? [
                { label: "W", value: cpu.w ?? 0 },
                { label: "V", value: cpu.v ?? 0 },
              ]
            : []),
          { label: "X", value: cpu.x },
          { label: "Y", value: cpu.y },
          { label: "U", value: cpu.u },
          { label: "S", value: cpu.s },
          { label: "PC", value: cpu.pc, highlight: true },
        ]
      : []
  );

  const regs8 = $derived(
    cpu
      ? [
          { label: "A", value: cpu.a },
          { label: "B", value: cpu.b },
          { label: "DP", value: cpu.dp },
          ...(isHd6309
            ? [
                { label: "E", value: (cpu.w ?? 0) >> 8 },
                { label: "F", value: (cpu.w ?? 0) & 0xff },
                { label: "MD", value: cpu.mode_reg ?? 0 },
              ]
            : []),
        ]
      : []
  );

  const flags = $derived(
    cpu
      ? [
          { label: "C", on: cpu.flags.c },
          { label: "V", on: cpu.flags.v },
          { label: "Z", on: cpu.flags.z },
          { label: "N", on: cpu.flags.n },
          { label: "I", on: cpu.flags.i },
          { label: "H", on: cpu.flags.h },
          { label: "F", on: cpu.flags.f },
          { label: "E", on: cpu.flags.e },
        ]
      : []
  );

  function fmt8(v: number) {
    return `$${v.toString(16).toUpperCase().padStart(2, "0")}`;
  }

  function fmt16(v: number) {
    return `$${v.toString(16).toUpperCase().padStart(4, "0")}`;
  }

  function startEdit(name: string, value: number, bits: 8 | 16) {
    const raw = bits === 8
      ? value.toString(16).toUpperCase().padStart(2, "0")
      : value.toString(16).toUpperCase().padStart(4, "0");
    editing = { name, bits, raw };
  }

  function commitEdit() {
    if (!editing) return;
    const parsed = parseInt(editing.raw.replace(/^\$/, ""), 16);
    const max = editing.bits === 8 ? 0xff : 0xffff;
    if (!isNaN(parsed) && parsed >= 0 && parsed <= max) {
      onSetRegister(editing.name, parsed);
    }
    editing = null;
  }

  function cancelEdit() {
    editing = null;
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit();
    } else if (e.key === "Escape") {
      cancelEdit();
    }
  }
</script>

<div class="panel register-panel panel-primary">
  <div class="panel-header">{$t("registers.title")}</div>
  <div class="panel-body">
    {#if cpu}
      <div class="reg-grid">
        {#each regs8 as reg}
          <div class="reg-cell">
            <span class="label">{reg.label}</span>
            {#if editing?.name === reg.label}
              <input
                class="value mono edit-input"
                bind:this={editInput}
                bind:value={editing.raw}
                onkeydown={handleEditKeydown}
                onblur={commitEdit}
                size="4"
              />
            {:else}
              <button
                class="value mono reg-btn"
                onclick={() => startEdit(reg.label, reg.value, 8)}
                title={$t("registers.editHint")}
              >{fmt8(reg.value)}</button>
            {/if}
          </div>
        {/each}
      </div>
      <div class="reg-grid wide">
        {#each regs16 as reg}
          <div class="reg-cell" class:highlight={reg.highlight}>
            <span class="label">{reg.label}</span>
            {#if editing?.name === reg.label}
              <input
                class="value mono edit-input"
                bind:this={editInput}
                bind:value={editing.raw}
                onkeydown={handleEditKeydown}
                onblur={commitEdit}
                size="6"
              />
            {:else}
              <button
                class="value mono reg-btn"
                onclick={() => startEdit(reg.label, reg.value, 16)}
                title={$t("registers.editHint")}
              >{fmt16(reg.value)}</button>
            {/if}
          </div>
        {/each}
      </div>
      <div class="flags-section">
        <span class="flags-label">{$t("registers.flags")}</span>
        <div class="flags">
          {#each flags as flag}
            <button
              class="flag"
              class:on={flag.on}
              onclick={() => onToggleFlag(flag.label)}
              aria-pressed={flag.on}
              title={$t("registers.toggleFlag")}
            >{flag.label}</button>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .register-panel {
    min-width: 220px;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .register-panel .panel-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .reg-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
    margin-bottom: 12px;
  }

  .reg-grid.wide {
    grid-template-columns: repeat(2, 1fr);
  }

  .reg-cell {
    background: var(--bg-deep);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .reg-cell.highlight {
    border-color: var(--accent-dim);
    box-shadow: 0 0 8px rgba(57, 255, 20, 0.15);
  }

  .reg-cell.highlight .value {
    color: var(--accent);
    text-shadow: 0 0 8px rgba(57, 255, 20, 0.4);
  }

  .label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-dim);
    letter-spacing: 0.1em;
  }

  .value {
    font-size: 14px;
    color: var(--accent-amber);
  }

  .reg-btn {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 14px;
    color: var(--accent-amber);
  }

  .reg-btn:hover {
    color: var(--accent);
  }

  .edit-input {
    width: 100%;
    padding: 2px 4px;
    font-size: 13px;
  }

  .flags-section {
    margin-top: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }

  .flags-label {
    font-size: 10px;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    display: block;
    margin-bottom: 8px;
  }

  .flags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .flag {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    border-radius: 4px;
    background: var(--flag-off);
    color: var(--text-dim);
    border: 1px solid var(--border);
    transition: all 0.15s;
    padding: 0;
    cursor: pointer;
  }

  .flag.on {
    background: rgba(57, 255, 20, 0.15);
    color: var(--flag-on);
    border-color: var(--accent-dim);
    box-shadow: 0 0 6px rgba(57, 255, 20, 0.2);
  }

  .flag:hover {
    border-color: var(--accent-dim);
  }
</style>