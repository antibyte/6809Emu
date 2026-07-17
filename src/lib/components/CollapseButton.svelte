<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";

  let {
    collapsed,
    onclick,
  }: {
    collapsed: boolean;
    onclick: () => void;
  } = $props();

  const label = $derived(collapsed ? $t("panels.expand") : $t("panels.collapse"));
</script>

<button
  class="hdr-btn collapse-btn"
  class:collapsed
  {onclick}
  title={label}
  aria-label={label}
  aria-expanded={!collapsed}
>
  <Icon name="chevron-down" size={13} />
</button>

<style>
  .collapse-btn :global(.icon) {
    transition: transform var(--motion-normal) var(--ease-tactile);
    margin-right: 0;
  }

  .collapse-btn.collapsed :global(.icon) {
    transform: rotate(-90deg);
  }
</style>
