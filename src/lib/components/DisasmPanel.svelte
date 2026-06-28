<script lang="ts">
  import type { DisasmLine } from "../types";
  import { t } from "../i18n";

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

  function fmtAddr(a: number) {
    return `$${a.toString(16).toUpperCase().padStart(4, "0")}`;
  }

  function fmtBytes(bytes: number[]) {
    return bytes
      .map((b) => b.toString(16).toUpperCase().padStart(2, "0"))
      .join(" ");
  }

  function openMenu(addr: number, e: MouseEvent) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, addr };
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
  <div class="panel-header">{$t("disasm.title")}</div>
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
    padding: 0;
  }

  .line {
    display: grid;
    grid-template-columns: 24px 56px minmax(72px, 108px) 1fr;
    gap: 8px;
    padding: 4px 12px;
    font-size: 12px;
    align-items: center;
    border-left: 2px solid transparent;
    transition: background 0.1s;
  }

  .line:hover {
    background: rgba(255, 255, 255, 0.02);
  }

  .line.pc {
    background: var(--pc-highlight);
    border-left-color: var(--pc-border);
    border-left-width: 2px;
    position: relative;
  }

  .line.pc::after {
    content: "";
    position: absolute;
    inset: 2px 0 2px auto;
    width: 1px;
    background: linear-gradient(
      to bottom,
      transparent 0%,
      color-mix(in srgb, var(--accent) 20%, transparent) 50%,
      transparent 100%
    );
    pointer-events: none;
  }

  .line.pc .text {
    color: var(--accent);
    text-shadow: 0 0 8px color-mix(in srgb, var(--accent) 40%, transparent);
  }

  .line.has-bp .bp {
    color: var(--danger);
  }

  .bp {
    background: none;
    border: none;
    padding: 0;
    font-size: 10px;
    color: var(--text-dim);
    width: 20px;
    height: 20px;
  }

  .bp:focus-visible {
    outline: 2px solid var(--accent-dim);
    outline-offset: 2px;
  }

  .addr {
    color: var(--text-dim);
  }

  .bytes {
    color: var(--accent-amber);
    opacity: 0.7;
  }

  .text {
    color: var(--text);
  }

  .ctx-menu {
    position: fixed;
    z-index: 2000;
    min-width: 160px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
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
      transform: scale(0.92);
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
    background: rgba(255, 255, 255, 0.05);
  }
</style>