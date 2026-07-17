<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "../icons";

  let {
    icon,
    message,
    size = 12,
  }: {
    icon: IconName;
    message: string;
    size?: number;
  } = $props();
</script>

<div class="empty-line">
  <Icon name={icon} {size} />
  {message}
</div>
