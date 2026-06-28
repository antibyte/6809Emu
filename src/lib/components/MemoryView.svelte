<script lang="ts">
  import { t } from "../i18n";

  let {
    address,
    bytes,
    followPc = $bindable(false),
    watchpoints,
    onGoto,
    onEdit,
    onToggleWatchpoint,
  }: {
    address: number;
    bytes: number[];
    followPc?: boolean;
    watchpoints: Set<number>;
    onGoto: (addr: number) => void;
    onEdit: (addr: number, value: number) => void;
    onToggleWatchpoint: (addr: number) => void;
  } = $props();

  let gotoInput = $state("0100");

  $effect(() => {
    gotoInput = address.toString(16).toUpperCase().padStart(4, "0");
  });

  function fmtAddr(a: number) {
    return a.toString(16).toUpperCase().padStart(4, "0");
  }

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

  const rows = $derived.by(() => {
    const result: { addr: number; cells: number[] }[] = [];
    for (let i = 0; i < bytes.length; i += 16) {
      result.push({
        addr: address + i,
        cells: bytes.slice(i, i + 16),
      });
    }
    return result;
  });
</script>

<div class="panel memory-panel panel-primary">
  <div class="panel-header">
    <div class="header-left">
      <span>{$t("memory.title")}</span>
      <span class="export-hint">{$t("memory.exportHint")} · {$t("memory.watchpointHint")}</span>
    </div>
    <div class="header-right">
      <label class="follow-toggle">
        <input type="checkbox" bind:checked={followPc} />
        {$t("memory.followPc")}
      </label>
      <div class="goto">
        <label for="goto">{$t("memory.goto")}:</label>
        <input id="goto" bind:value={gotoInput} class="mono" size="6" />
        <button onclick={handleGoto}>→</button>
      </div>
    </div>
  </div>
  <div class="panel-body hex-grid">
    <div class="row header">
      <span class="addr-col">{$t("memory.address")}</span>
      {#each Array(16) as _, i}
        <span class="col-hdr">{i.toString(16).toUpperCase()}</span>
      {/each}
    </div>
    {#each rows as row}
      <div class="row">
        <span class="addr-col mono">{fmtAddr(row.addr)}</span>
        {#each row.cells as byte, i}
          {@const cellAddr = row.addr + i}
          <input
            class="cell mono"
            class:has-wp={watchpoints.has(cellAddr)}
            value={byte.toString(16).toUpperCase().padStart(2, "0")}
            onchange={(e) => handleEdit(cellAddr, e)}
            oncontextmenu={(e) => handleContextMenu(cellAddr, e)}
            title={$t("memory.watchpointHint")}
            maxlength="2"
          />
        {/each}
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
    flex-direction: column;
    gap: 2px;
  }

  .export-hint {
    font-size: 10px;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-dim);
    opacity: 0.8;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 12px;
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
    gap: 6px;
    font-size: 11px;
    text-transform: none;
    letter-spacing: 0;
  }

  .goto input {
    width: 70px;
    padding: 4px 6px;
    font-size: 11px;
  }

  .goto button {
    padding: 4px 8px;
    font-size: 11px;
  }

  .hex-grid {
    padding: 8px;
    overflow-x: auto;
  }

  .row {
    display: grid;
    grid-template-columns: 56px repeat(16, 28px);
    gap: 2px;
    margin-bottom: 2px;
  }

  .row.header {
    margin-bottom: 6px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border);
  }

  .addr-col {
    color: var(--text-dim);
    font-size: 11px;
    padding: 2px 0;
  }

  .col-hdr {
    font-size: 10px;
    color: var(--text-dim);
    text-align: center;
  }

  .cell {
    width: 28px;
    padding: 2px;
    text-align: center;
    font-size: 11px;
    border: 1px solid transparent;
    background: var(--bg-deep);
  }

  .cell.has-wp {
    border-color: var(--accent-amber);
    box-shadow: 0 0 4px rgba(255, 176, 0, 0.25);
  }

  .cell:hover {
    border-color: var(--border);
  }

  .cell:focus {
    border-color: var(--accent-dim);
    color: var(--accent);
  }
</style>