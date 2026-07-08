<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { fade, scale } from "svelte/transition";

  let {
    open,
    onClose,
  }: {
    open: boolean;
    onClose: () => void;
  } = $props();

  let panel: HTMLDivElement | undefined = $state();
  let lastFocused: HTMLElement | null = null;

  function onKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  function onPanelKeydown(e: KeyboardEvent) {
    if (!open || !panel) return;
    if (e.key === "Tab") {
      const focusables = panel.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  $effect(() => {
    if (open) {
      lastFocused = document.activeElement as HTMLElement | null;
      const t = window.setTimeout(() => panel?.focus(), 0);
      return () => {
        window.clearTimeout(t);
        lastFocused?.focus?.();
      };
    }
  });

  const groups = $derived([
    {
      label: $t("shortcuts.category.transport"),
      items: [
        { key: "F5", label: $t("shortcuts.run") },
        { key: "Shift+F5", label: $t("shortcuts.pause") },
        { key: "F10", label: $t("shortcuts.step") },
        { key: "Ctrl+Shift+F5", label: $t("shortcuts.reset") },
      ],
    },
    {
      label: $t("shortcuts.category.debug"),
      items: [
        { key: "F9", label: $t("shortcuts.breakpoint") },
        { key: "Ctrl+Enter", label: $t("shortcuts.assemble") },
      ],
    },
    {
      label: $t("shortcuts.category.ui"),
      items: [
        { key: "?", label: $t("shortcuts.open") },
        { key: "V", label: $t("shortcuts.videoToggle") },
        { key: "T", label: $t("shortcuts.theme") },
        { key: "L", label: $t("shortcuts.locale") },
      ],
    },
  ]);
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="backdrop" transition:fade={{ duration: 120 }} onclick={onClose} role="presentation">
    <div
      class="overlay"
      transition:scale={{ duration: 160, start: 0.96 }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="sc-title"
      tabindex="-1"
      bind:this={panel}
      onkeydown={onPanelKeydown}
      onclick={(e) => e.stopPropagation()}
    >
      <header class="ov-header">
        <h2 id="sc-title"><Icon name="keyboard" size={15} /> {$t("shortcuts.open")}</h2>
        <button class="hdr-btn" onclick={onClose} aria-label={$t("shortcuts.close")} title={$t("shortcuts.close")}>
          <Icon name="close" size={14} />
        </button>
      </header>
      <div class="ov-body">
        {#each groups as group}
          <section class="group">
            <h3>{group.label}</h3>
            <ul>
              {#each group.items as item}
                <li>
                  <span class="label">{item.label}</span>
                  <kbd>{item.key}</kbd>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 4000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(4, 8, 12, 0.66);
    backdrop-filter: blur(6px);
  }

  .overlay {
    width: min(560px, 96vw);
    background: var(--bg-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }

  .ov-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-2);
    border-bottom: 1px solid var(--border);
  }

  .ov-header h2 {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text);
  }

  .ov-header h2 :global(.icon) {
    color: var(--accent);
    margin-right: 0;
  }

  .ov-body {
    padding: 16px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 18px;
  }

  .group h3 {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-faint);
    margin-bottom: 8px;
  }

  .group ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .group li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 12.5px;
    color: var(--text-dim);
  }

  .group li .label {
    color: var(--text);
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 2px 7px;
    background: var(--bg-0);
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: 4px;
    color: var(--text);
    white-space: nowrap;
  }
</style>
