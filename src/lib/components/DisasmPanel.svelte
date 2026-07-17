<script lang="ts">
  import type { DisasmLine } from "../types";
  import { t } from "../i18n";
  import { fmtAddr, fmtBytes } from "../format";

  let {
    lines,
    pc,
    running = false,
    onToggleBreakpoint,
    onSetPc,
    onRunTo,
    breakpoints,
  }: {
    lines: DisasmLine[];
    pc: number;
    running?: boolean;
    breakpoints: Set<number>;
    onToggleBreakpoint: (addr: number) => void;
    onSetPc: (addr: number) => void;
    onRunTo: (addr: number) => void;
  } = $props();

  let scrollContainer: HTMLDivElement | undefined = $state();
  let menu: { x: number; y: number; addr: number } | null = $state(null);

  function openMenu(addr: number, e: MouseEvent) {
    e.preventDefault();
    const margin = 8;
    const w = 180;
    const h = 120;
    const x = Math.min(e.clientX, window.innerWidth - w - margin);
    const y = Math.min(e.clientY, window.innerHeight - h - margin);
    menu = { x: Math.max(margin, x), y: Math.max(margin, y), addr };
  }

  function closeMenu() {
    menu = null;
  }

  function handleAction(action: "bp" | "pc" | "run") {
    if (!menu) return;
    const addr = menu.addr;
    closeMenu();
    if (action === "bp") onToggleBreakpoint(addr);
    else if (action === "pc") onSetPc(addr);
    else onRunTo(addr);
  }

  $effect(() => {
    pc;
    if (!scrollContainer) return;
    const el = scrollContainer.querySelector(".line.pc");
    el?.scrollIntoView({ block: "nearest", behavior: running ? "auto" : "smooth" });
  });
</script>

<svelte:window onclick={closeMenu} />

<div class="panel disasm-panel panel-primary">
  <div class="panel-header">
    <span class="ph-title"><span class="accent-dot"></span>{$t("disasm.title")}</span>
  </div>
  <div class="panel-body lines" bind:this={scrollContainer}>
    {#each lines as line}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="line"
        class:pc={line.address === pc}
        class:has-bp={breakpoints.has(line.address)}
        oncontextmenu={(e) => openMenu(line.address, e)}
      >
        <button
          class="bp"
          onclick={() => onToggleBreakpoint(line.address)}
          title={$t("disasm.breakpoint")}
          aria-label={$t("disasm.breakpoint")}
          aria-pressed={breakpoints.has(line.address)}
        >
          {breakpoints.has(line.address) ? "●" : "○"}
        </button>
        <span class="addr mono">{fmtAddr(line.address)}</span>
        <span class="bytes mono">{fmtBytes(line.bytes)}</span>
        <span class="text mono">{line.text}</span>
      </div>
    {/each}
  </div>
</div>

{#if menu}
  <div
    class="ctx-menu"
    style="left: {menu.x}px; top: {menu.y}px;"
    role="menu"
  >
    <button role="menuitem" onclick={() => handleAction("bp")}>
      {$t("disasm.ctxBreakpoint")}
    </button>
    <button role="menuitem" onclick={() => handleAction("pc")}>
      {$t("disasm.ctxSetPc")}
    </button>
    <button role="menuitem" onclick={() => handleAction("run")} disabled={running}>
      {$t("disasm.ctxRunTo")}
    </button>
  </div>
{/if}

<style>
  .disasm-panel {
    flex: 1;
    min-width: 0;
    height: 100%;
    min-height: 0;
  }

  .lines {
    padding: 4px 0;
  }

  .line {
    display: grid;
    grid-template-columns: 22px 54px minmax(64px, 104px) 1fr;
    gap: 8px;
    padding: 1px 12px;
    font-size: 12px;
    align-items: center;
    border-left: 2px solid transparent;
    min-height: var(--row-h);
    transition: background var(--motion-fast) ease;
  }

  .line:hover {
    background: var(--bg-hover);
  }

  .line.pc {
    background: var(--pc-highlight);
    border-left-color: var(--pc-border);
  }

  .line.pc .text {
    color: var(--accent);
    font-weight: 600;
  }

  .line.pc .addr {
    color: var(--accent);
  }

  .line.has-bp .bp {
    color: var(--danger);
  }

  .bp {
    background: none;
    border: none;
    padding: 0;
    font-size: 11px;
    color: var(--text-faint);
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .bp:hover {
    color: var(--danger);
  }

  .bp:focus-visible {
    outline: 2px solid var(--accent-dim);
    outline-offset: 2px;
  }

  .addr {
    color: var(--text-faint);
  }

  .bytes {
    color: var(--amber);
    opacity: 0.65;
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .text {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ctx-menu {
    position: fixed;
    z-index: 3000;
    min-width: 180px;
    background: var(--bg-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-pop);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    transform-origin: top left;
    animation: ctxIn var(--motion-normal) var(--ease-tactile);
  }

  @keyframes ctxIn {
    from {
      opacity: 0;
      transform: scale(0.94);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .ctx-menu button {
    text-align: left;
    padding: 6px 10px;
    font-size: 12px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text);
  }

  .ctx-menu button:hover:not(:disabled) {
    background: var(--bg-hover);
  }
</style>