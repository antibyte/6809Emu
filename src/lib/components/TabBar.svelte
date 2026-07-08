<script lang="ts">
  let {
    tabs,
    active,
    onSelect,
  }: {
    tabs: { id: string; label: string }[];
    active: string;
    onSelect: (id: string) => void;
  } = $props();
</script>

<div class="tab-bar" role="tablist">
  {#each tabs as tab}
    <button
      class="tab"
      class:active={active === tab.id}
      role="tab"
      aria-selected={active === tab.id}
      onclick={() => onSelect(tab.id)}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tab-bar {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow-x: auto;
    flex-shrink: 0;
  }

  .tab {
    padding: 5px 12px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--text-dim);
    white-space: nowrap;
    transition:
      background var(--motion-normal) ease,
      border-color var(--motion-normal) ease,
      color var(--motion-normal) ease;
  }

  .tab:hover {
    color: var(--text);
    background: var(--bg-hover);
  }

  .tab.active {
    background: var(--bg-2);
    border-color: var(--border);
    color: var(--accent);
  }

</style>