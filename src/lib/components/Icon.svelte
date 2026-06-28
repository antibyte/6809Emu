<script lang="ts">
  let {
    name,
    size = 14,
  }: {
    name:
      | "run"
      | "pause"
      | "step"
      | "reset"
      | "import"
      | "export"
      | "close";
    size?: number;
  } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="currentColor"
  aria-hidden="true"
>
  {#if name === "run"}
    <path d="M4 2.5v11l9-5.5z" />
  {:else if name === "pause"}
    <path d="M4 2h3v12H4zm5 0h3v12H9z" />
  {:else if name === "step"}
    <path d="M3 2.5v11l7-5.5zm8 0v11h2V2.5z" />
  {:else if name === "reset"}
    <path d="M8 2a6 6 0 1 0 5.2 9H10l3-3 3 3h-3.2A8 8 0 1 1 8 0z" />
  {:else if name === "import"}
    <path d="M8 1v9M4 7l4 4 4-4M2 13h12v2H2z" stroke="currentColor" stroke-width="1.2" fill="none" />
  {:else if name === "export"}
    <path d="M8 11V2M4 6l4-4 4 4M2 13h12v2H2z" stroke="currentColor" stroke-width="1.2" fill="none" />
  {:else if name === "close"}
    <path d="M3 3l10 10M13 3L3 13" stroke="currentColor" stroke-width="1.5" fill="none" />
  {/if}
</svg>

<style>
  .icon {
    display: inline-block;
    vertical-align: -2px;
    margin-right: 4px;
    flex-shrink: 0;
  }
</style>