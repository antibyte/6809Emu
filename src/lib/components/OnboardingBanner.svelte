<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";

  let {
    onDismiss,
  }: {
    onDismiss: () => void;
  } = $props();
</script>

<div class="onboarding" role="note">
  <p>{$t("onboarding.message")}</p>
  <button class="dismiss" onclick={onDismiss} aria-label={$t("onboarding.dismiss")}>
    <Icon name="close" size={12} />
  </button>
</div>

<style>
  .onboarding {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 7px 14px;
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--text);
  }

  .onboarding p {
    margin: 0;
    line-height: 1.4;
  }

  .dismiss {
    background: none;
    border: none;
    padding: 4px;
    color: var(--text-dim);
    flex-shrink: 0;
    display: inline-flex;
  }

  .dismiss :global(.icon) {
    margin-right: 0;
  }

  .dismiss:hover {
    color: var(--text);
  }
</style>