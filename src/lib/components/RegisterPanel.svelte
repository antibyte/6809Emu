<script lang="ts">
  import type { CpuState } from "../types";
  import { t } from "../i18n";
  import CollapseButton from "./CollapseButton.svelte";
  import { fmtAddr, fmtByte, toHex } from "../format";

  let {
    cpu,
    collapsed = false,
    onToggleCollapse,
    onSetRegister,
    onToggleFlag,
  }: {
    cpu: CpuState | null;
    collapsed?: boolean;
    onToggleCollapse?: () => void;
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

  function startEdit(name: string, value: number, bits: 8 | 16) {
    const raw = toHex(value, bits === 8 ? 2 : 4);
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

  // Change-highlighting: flash registers whose value changed.
  let changed = $state<Set<string>>(new Set());
  let prevValues: Record<string, number> = {};
  let clearTimers: Record<string, ReturnType<typeof setTimeout>> = {};

  $effect(() => {
    if (!cpu) return;
    const cur: Record<string, number> = {
      A: cpu.a, B: cpu.b, DP: cpu.dp, D: cpu.d, X: cpu.x, Y: cpu.y, U: cpu.u, S: cpu.s, PC: cpu.pc,
    };
    if (isHd6309) {
      cur.W = cpu.w ?? 0; cur.V = cpu.v ?? 0; cur.E = (cpu.w ?? 0) >> 8; cur.F = (cpu.w ?? 0) & 0xff; cur.MD = cpu.mode_reg ?? 0;
    }
    let dirty = false;
    const next = new Set(changed);
    for (const [k, v] of Object.entries(cur)) {
      if (k in prevValues && prevValues[k] !== v) {
        next.add(k);
        dirty = true;
        if (clearTimers[k]) clearTimeout(clearTimers[k]);
        clearTimers[k] = setTimeout(() => {
          changed = new Set([...changed].filter((c) => c !== k));
          delete clearTimers[k];
        }, 600);
      }
      prevValues[k] = v;
    }
    if (dirty) changed = next;
  });

  $effect(() => () => {
    for (const t of Object.values(clearTimers)) clearTimeout(t);
  });

  function isChanged(label: string): boolean {
    return changed.has(label);
  }
</script>

<div class="panel register-panel panel-primary" class:collapsed>
  <div class="panel-header">
    <span class="ph-title">
      <span class="accent-dot"></span>
      {$t("registers.title")}
    </span>
    {#if onToggleCollapse}
      <CollapseButton {collapsed} onclick={onToggleCollapse} />
    {/if}
  </div>
  <div class="panel-body">
    {#if cpu}
      <div class="reg-grid">
        {#each regs8 as reg}
          <div class="reg-cell" class:highlight={reg.highlight} class:changed={isChanged(reg.label)}>
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
                title={`${$t("registers.editHint")} · ${$t("registers.dec")}: ${reg.value}`}
              >{fmtByte(reg.value)}</button>
            {/if}
          </div>
        {/each}
      </div>
      <div class="reg-grid wide">
        {#each regs16 as reg}
          <div class="reg-cell" class:highlight={reg.highlight} class:changed={isChanged(reg.label)}>
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
                title={`${$t("registers.editHint")} · ${$t("registers.dec")}: ${reg.value}`}
              >{fmtAddr(reg.value)}</button>
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

  .register-panel.collapsed .panel-body {
    display: none;
  }

  .register-panel .panel-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .reg-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    margin-bottom: 8px;
  }

  .reg-grid.wide {
    grid-template-columns: repeat(2, 1fr);
  }

  .reg-cell {
    background: var(--bg-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .reg-cell.highlight {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .reg-cell.highlight .value {
    color: var(--accent);
  }

  .reg-cell.changed {
    border-color: var(--changed);
    animation: regFlash var(--motion-slow) var(--ease-exit);
  }

  .reg-cell.changed .value {
    color: var(--changed);
  }

  @keyframes regFlash {
    0% { background: var(--changed-soft); }
    100% { background: var(--bg-0); }
  }

  .label {
    font-size: 9.5px;
    font-weight: 600;
    color: var(--text-faint);
    letter-spacing: 0.1em;
  }

  .value {
    font-size: 13px;
    color: var(--amber);
  }

  .reg-btn {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 13px;
    color: var(--amber);
  }

  .reg-btn:hover {
    color: var(--accent);
  }

  .edit-input {
    width: 100%;
    padding: 1px 4px;
    font-size: 12px;
  }

  .flags-section {
    margin-top: 6px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .flags-label {
    font-size: 9.5px;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    display: block;
    margin-bottom: 6px;
  }

  .flags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .flag {
    width: 24px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 600;
    border-radius: 4px;
    background: var(--flag-off);
    color: var(--text-faint);
    border: 1px solid var(--border);
    transition: background var(--motion-normal) ease, color var(--motion-normal) ease, border-color var(--motion-normal) ease;
    padding: 0;
    cursor: pointer;
  }

  .flag.on {
    background: var(--accent-soft);
    color: var(--flag-on);
    border-color: var(--accent-line);
  }

  .flag:hover {
    border-color: var(--accent-dim);
  }
</style>