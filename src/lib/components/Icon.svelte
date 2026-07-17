<script lang="ts">
  import type { IconName } from "../icons";

  let {
    name,
    size = 14,
  }: {
    name: IconName;
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
  {#if name === "run" || name === "play"}
    <path d="M4 2.5v11l9-5.5z" />
  {:else if name === "pause"}
    <path d="M4 2h3v12H4zm5 0h3v12H9z" />
  {:else if name === "step"}
    <path d="M3 2.5v11l7-5.5zm8 0v11h2V2.5z" />
  {:else if name === "reset"}
    <path d="M8 2a6 6 0 1 0 5.2 9H10l3-3 3 3h-3.2A8 8 0 1 1 8 0z" />
  {:else if name === "import" || name === "file"}
    <path d="M8 1v9M4 7l4 4 4-4M2 13h12v2H2z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "export"}
    <path d="M8 11V2M4 6l4-4 4 4M2 13h12v2H2z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "save"}
    <path d="M3 2h8l3 3v9H3V2zm2 0v4h6V2H5zm1 6h4v5H6V8z" fill-rule="evenodd" />
  {:else if name === "folder-open"}
    <path d="M1 3h5l2 2h7v9H1V3zm1 2v8h12l-2-4 2-4H2z" fill-rule="evenodd" />
  {:else if name === "settings"}
    <path d="M8 5.5A2.5 2.5 0 1 0 8 10.5 2.5 2.5 0 0 0 8 5.5zm0 1.5a1 1 0 1 1 0 2 1 1 0 0 1 0-2z" fill-rule="evenodd" />
    <path d="M8 1l.7 1.8 1.9-.4.4 1.9 1.8.7-.9 1.7.9 1.7-1.8.7-.4 1.9-1.9-.4L8 15l-.7-1.8-1.9.4-.4-1.9-1.8-.7.9-1.7-.9-1.7 1.8-.7.4-1.9 1.9.4L8 1z" fill="none" stroke="currentColor" stroke-width="0.7" />
  {:else if name === "close"}
    <path d="M3 3l10 10M13 3L3 13" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" />
  {:else if name === "cpu" || name === "chip"}
    <rect x="4" y="4" width="8" height="8" rx="1" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="6.5" y="6.5" width="3" height="3" rx="0.5" />
    <path d="M6 2v2M10 2v2M6 12v2M10 12v2M2 6h2M2 10h2M12 6h2M12 10h2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  {:else if name === "memory"}
    <rect x="1.5" y="4" width="13" height="8" rx="1" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M4 12v2M7 12v2M10 12v2M4 2v2M7 2v2M10 2v2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
    <path d="M5 7h2M9 7h2M5 9h2M9 9h2" stroke="currentColor" stroke-width="0.9" stroke-linecap="round" />
  {:else if name === "list"}
    <path d="M3 3h10v1.5H3zM3 7h10v1.5H3zM3 11h10v1.5H3z" />
    <circle cx="1.5" cy="3.75" r="0.8" />
    <circle cx="1.5" cy="7.75" r="0.8" />
    <circle cx="1.5" cy="11.75" r="0.8" />
  {:else if name === "terminal"}
    <rect x="1" y="2" width="14" height="12" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M3.5 6l2.5 2-2.5 2" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M7.5 10h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  {:else if name === "video"}
    <rect x="1" y="3" width="11" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M12 6l3-2v8l-3-2z" fill="currentColor" />
  {:else if name === "registers"}
    <rect x="2" y="2" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="9" y="2" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="2" y="9" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="9" y="9" width="5" height="5" rx="0.8" />
  {:else if name === "disasm"}
    <path d="M2 3h12M2 6h8M2 9h10M2 12h6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
    <circle cx="13.5" cy="6" r="1" />
  {:else if name === "code"}
    <path d="M5 4L1.5 8 5 12M11 4l3.5 4L11 12M9 3l-2 10" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "trace"}
    <path d="M1.5 13l3-5 3 3 4-8 3 4" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    <circle cx="4.5" cy="8" r="1" />
    <circle cx="11.5" cy="3" r="1" />
  {:else if name === "flag"}
    <path d="M3 1.5v13" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
    <path d="M3 2h8l-1.5 2.5L11 7H3z" />
  {:else if name === "breakpoint"}
    <circle cx="8" cy="8" r="4.5" fill="none" stroke="currentColor" stroke-width="1.3" />
    <circle cx="8" cy="8" r="2.5" />
  {:else if name === "watch"}
    <path d="M8 2.5L4.5 5 4 9l4 4.5L12 9l-.5-4L8 2.5z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
    <circle cx="8" cy="8" r="1.4" />
  {:else if name === "io"}
    <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M5 5v6M8 5v6M11 5v6" stroke="currentColor" stroke-width="1" stroke-linecap="round" />
  {:else if name === "theme"}
    <path d="M8 1.5A6.5 6.5 0 1 0 14.5 8c0-.3 0-.6-.05-.9A4 4 0 0 1 8.9 1.55 6.5 6.5 0 0 0 8 1.5z" />
    <path d="M10.5 3a2 2 0 0 0 2 2" fill="none" stroke="currentColor" stroke-width="0.8" />
  {:else if name === "globe"}
    <circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M1.5 8h13M8 1.5c2 2 2 11 0 13M8 1.5c-2 2-2 11 0 13" fill="none" stroke="currentColor" stroke-width="1" />
  {:else if name === "keyboard"}
    <rect x="1" y="4" width="14" height="8" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M3 7h1M5.5 7h1M8 7h1M10.5 7h1M13 7h0M4 9h8" stroke="currentColor" stroke-width="1" stroke-linecap="round" />
  {:else if name === "chevron-down"}
    <path d="M3 6l5 5 5-5" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "arrow-right"}
    <path d="M3 8h9M9 4l4 4-4 4" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "external"}
    <path d="M7 2H2v12h12V9" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    <path d="M9 2h5v5M14 2L8 8" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "minus"}
    <path d="M3 8h10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
  {:else if name === "plus"}
    <path d="M3 8h10M8 3v10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
  {:else if name === "collapse"}
    <path d="M2 6h4V2M14 10h-4v4M2 10h4v4M14 6h-4V2" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "expand"}
    <path d="M2 2h4M2 2v4M14 2h-4M14 2v4M2 14h4M2 14v-4M14 14h-4M14 14v-4" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "check"}
    <path d="M3 8.5l3 3 7-7" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  {:else if name === "view"}
    <rect x="2" y="2" width="5" height="5" rx="0.8" />
    <rect x="9" y="2" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="2" y="9" width="5" height="5" rx="0.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <rect x="9" y="9" width="5" height="5" rx="0.8" />
  {:else if name === "clear"}
    <path d="M3 6h10M5 6v6a1 1 0 001 1h4a1 1 0 001-1V6M8 3v3M6 3h4" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round" stroke-linejoin="round" />
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
