<script lang="ts">
  import { fade } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  let {
    label,
    buttonClass = "",
    buttonLabel,
    icon = undefined,
    active = false,
    align = "left",
    children,
  }: {
    label: string;
    buttonClass?: string;
    buttonLabel: string;
    icon?: IconName;
    active?: boolean;
    align?: "left" | "right";
    children: import("svelte").Snippet;
  } = $props();

  let open = $state(false);
  let trigger: HTMLButtonElement | undefined = $state();
  let popover: HTMLDivElement | undefined = $state();

  function toggle() {
    open = !open;
  }

  function onWindowPointerDown(e: MouseEvent) {
    if (!open) return;
    const target = e.target as Node;
    if (trigger?.contains(target)) return;
    if (popover?.contains(target)) return;
    open = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      open = false;
      trigger?.focus();
    }
  }

  function close() {
    open = false;
  }

  function onPopoverClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    if (el.closest("button.menu-item:not(:disabled)")) close();
    else if (el.closest("label.menu-item.check:not(.disabled)")) close();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onKeydown} />

<button
  class="popover-trigger {buttonClass}"
  class:active={open || active}
  onclick={toggle}
  aria-label={label}
  title={label}
  aria-haspopup="true"
  aria-expanded={open}
  bind:this={trigger}
>
  {#if icon}
    <Icon name={icon} size={14} />
  {/if}
  {#if buttonLabel}
    <span class="lbl">{buttonLabel}</span>
  {/if}
</button>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="popover"
    class:align-right={align === "right"}
    bind:this={popover}
    transition:fade={{ duration: 100 }}
    role="menu"
    tabindex="-1"
    onclick={onPopoverClick}
  >
    {@render children()}
  </div>
{/if}

<style>
  .popover-trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-dim);
    border-radius: var(--radius-sm);
  }

  .popover-trigger:hover {
    background: var(--bg-hover);
    color: var(--text);
    border-color: var(--border);
  }

  .popover-trigger.active {
    color: var(--accent);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .popover-trigger .lbl {
    white-space: nowrap;
  }

  .popover {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 2500;
    min-width: 220px;
    background: var(--bg-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-pop);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .popover.align-right {
    left: auto;
    right: 0;
  }

  .popover :global(.menu-item) {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    font-size: 12.5px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }

  .popover :global(.menu-item:hover:not(:disabled)) {
    background: var(--bg-hover);
  }

  .popover :global(.menu-item:disabled) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .popover :global(.menu-sep) {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }

  .popover :global(.menu-label) {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
    padding: 4px 10px 2px;
  }

  .popover :global(.field) {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    font-size: 12px;
  }

  .popover :global(.field .lk) {
    flex: 1;
    color: var(--text-dim);
  }

  .popover :global(.field input),
  .popover :global(.field select) {
    width: 130px;
    font-size: 12px;
    padding: 4px 8px;
  }

  @media (max-width: 480px) {
    .popover {
      min-width: min(280px, calc(100vw - 24px));
    }
    .popover :global(.field) {
      flex-direction: column;
      align-items: stretch;
      gap: 3px;
    }
    .popover :global(.field .lk) {
      font-size: 10px;
    }
    .popover :global(.field input),
    .popover :global(.field select) {
      width: 100%;
    }
  }
</style>
