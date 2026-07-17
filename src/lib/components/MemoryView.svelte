<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { toHex } from "../format";

  let {
    address,
    bytes,
    followPc = $bindable(false),
    watchpoints,
    onGoto,
    onEdit,
    onToggleWatchpoint,
    onClose,
  }: {
    address: number;
    bytes: number[];
    followPc?: boolean;
    watchpoints: Set<number>;
    onGoto: (addr: number) => void;
    onEdit: (addr: number, value: number) => void;
    onToggleWatchpoint: (addr: number) => void;
    onClose?: () => void;
  } = $props();

  let gotoInput = $state("0100");
  let gridEl: HTMLDivElement | undefined = $state();
  let colsPerRow = $state(16);

  let changedAddrs = $state<Set<number>>(new Set());
  let prevBytes: Record<number, number> = {};
  const clearTimers = new Map<number, ReturnType<typeof setTimeout>>();

  $effect(() => {
    gotoInput = toHex(address, 4);
  });

  $effect(() => {
    address;
    prevBytes = {};
    changedAddrs = new Set();
    for (const t of clearTimers.values()) clearTimeout(t);
    clearTimers.clear();
  });

  $effect(() => {
    address;
    bytes;
    let dirty = false;
    const next = new Set(changedAddrs);
    for (let i = 0; i < bytes.length; i++) {
      const addr = address + i;
      const v = bytes[i];
      if (addr in prevBytes && prevBytes[addr] !== v) {
        next.add(addr);
        dirty = true;
        const prev = clearTimers.get(addr);
        if (prev) clearTimeout(prev);
        clearTimers.set(
          addr,
          setTimeout(() => {
            changedAddrs = new Set([...changedAddrs].filter((a) => a !== addr));
            clearTimers.delete(addr);
          }, 600),
        );
      }
      prevBytes[addr] = v;
    }
    if (dirty) changedAddrs = next;
  });

  $effect(() => {
    if (!gridEl) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 640;
      colsPerRow = w < 560 ? 8 : 16;
    });
    ro.observe(gridEl);
    return () => ro.disconnect();
  });

  $effect(() => () => {
    for (const t of clearTimers.values()) clearTimeout(t);
    clearTimers.clear();
  });

  function handleGoto() {
    const parsed = parseInt(gotoInput, 16);
    if (!isNaN(parsed) && parsed >= 0 && parsed <= 0xffff) {
      onGoto(parsed);
    }
  }

  function handleEdit(addr: number, event: Event) {
    const input = event.target as HTMLInputElement;
    const val = parseInt(input.value, 16);
    if (!isNaN(val) && val >= 0 && val <= 0xff) {
      onEdit(addr, val);
    }
  }

  function handleContextMenu(addr: number, e: MouseEvent) {
    e.preventDefault();
    onToggleWatchpoint(addr);
  }

  function isChanged(addr: number): boolean {
    return changedAddrs.has(addr);
  }

  const rows = $derived.by(() => {
    const cols = colsPerRow;
    const result: { addr: number; cells: number[]; ascii: string }[] = [];
    for (let i = 0; i < bytes.length; i += cols) {
      const cells = bytes.slice(i, i + cols);
      const ascii = cells
        .map((b) => {
          const c = b & 0x7f;
          return c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : "·";
        })
        .join("");
      result.push({ addr: address + i, cells, ascii });
    }
    return result;
  });

  const colIndices = $derived(Array.from({ length: colsPerRow }, (_, i) => i));
  const bytesGridStyle = $derived(`grid-template-columns: repeat(${colsPerRow}, 1fr)`);
</script>

<div class="panel memory-panel panel-primary">
  <div class="panel-header">
    <div class="header-left">
      <span class="ph-title"><span class="accent-dot"></span>{$t("memory.title")}</span>
      <span class="export-hint">{$t("memory.exportHint")}</span>
    </div>
    <div class="header-right">
      <label class="follow-toggle">
        <input type="checkbox" bind:checked={followPc} />
        {$t("memory.followPc")}
      </label>
      <div class="goto">
        <label for="goto" class="goto-label">{$t("memory.goto")}:</label>
        <input id="goto" bind:value={gotoInput} class="mono" size="6" />
        <button class="goto-btn" onclick={handleGoto} aria-label={$t("memory.goto")}>
          <Icon name="arrow-right" size={12} />
        </button>
      </div>
      {#if onClose}
        <button class="hdr-btn" onclick={onClose} title={$t("panels.close")} aria-label={$t("panels.close")}>
          <Icon name="close" size={13} />
        </button>
      {/if}
    </div>
  </div>
  <div class="panel-body hex-grid" bind:this={gridEl}>
    <div class="row header">
      <span class="addr-col">{$t("memory.address")}</span>
      <span class="bytes-hdr" style={bytesGridStyle}>
        {#each colIndices as i}
          <span class="col-hdr">{i.toString(16).toUpperCase()}</span>
        {/each}
      </span>
      <span class="ascii-hdr">{$t("memory.ascii")}</span>
    </div>
    {#each rows as row}
      <div class="row">
        <span class="addr-col mono">{toHex(row.addr, 4)}</span>
        <span class="bytes" style={bytesGridStyle}>
          {#each row.cells as byte, i}
            {@const cellAddr = row.addr + i}
            <input
              class="cell mono"
              class:has-wp={watchpoints.has(cellAddr)}
              class:changed={isChanged(cellAddr)}
              value={toHex(byte, 2)}
              onchange={(e) => handleEdit(cellAddr, e)}
              oncontextmenu={(e) => handleContextMenu(cellAddr, e)}
              title={$t("memory.watchpointHint")}
              maxlength="2"
              spellcheck="false"
            />
          {/each}
        </span>
        <span class="ascii mono">{row.ascii}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .memory-panel {
    height: 100%;
    min-height: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .header-left .ph-title {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .export-hint {
    font-size: 10px;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-faint);
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .follow-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-dim);
    cursor: pointer;
  }

  .follow-toggle input {
    accent-color: var(--accent);
  }

  .goto {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    text-transform: none;
    letter-spacing: 0;
  }

  .goto-label {
    color: var(--text-faint);
  }

  .goto input {
    width: 58px;
    padding: 3px 6px;
    font-size: 11px;
  }

  .goto-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px 6px;
    font-size: 11px;
  }

  .goto-btn :global(.icon) {
    margin-right: 0;
  }

  .hex-grid {
    padding: 6px 8px;
    overflow: auto;
    font-size: 12px;
  }

  .row {
    display: grid;
    grid-template-columns: 54px 1fr 96px;
    gap: 8px;
    align-items: center;
    height: var(--row-h);
  }

  .row.header {
    height: auto;
    margin-bottom: 4px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border);
  }

  .bytes-hdr,
  .bytes {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .addr-col {
    color: var(--text-faint);
    font-size: 11px;
  }

  .col-hdr {
    font-size: 10px;
    color: var(--text-faint);
    text-align: center;
  }

  .ascii-hdr {
    font-size: 10px;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .cell {
    width: 100%;
    min-width: 0;
    padding: 1px 0;
    text-align: center;
    font-size: 11px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    transition:
      background var(--motion-fast) ease,
      border-color var(--motion-fast) ease,
      color var(--motion-fast) ease;
  }

  .cell.has-wp {
    border-color: var(--amber);
    background: color-mix(in srgb, var(--amber) 12%, transparent);
    color: var(--amber);
  }

  .cell.changed {
    border-color: var(--changed);
    background: var(--changed-soft);
    color: var(--changed);
  }

  .cell:hover {
    border-color: var(--border);
  }

  .cell:focus {
    border-color: var(--accent-dim);
    color: var(--accent);
    background: var(--accent-soft);
  }

  .ascii {
    color: var(--text-dim);
    font-size: 11px;
    letter-spacing: 1px;
    white-space: pre;
    overflow: hidden;
    text-overflow: clip;
  }

  @media (max-width: 520px) {
    .row {
      grid-template-columns: 48px 1fr 64px;
    }
    .export-hint {
      display: none;
    }
    .ascii {
      letter-spacing: 0.5px;
    }
  }
</style>
