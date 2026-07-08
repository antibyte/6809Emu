<script lang="ts">
  import { toasts, dismissToast } from "../toast";
  import Icon from "./Icon.svelte";

  const iconFor = (type: string) =>
    type === "success" ? "run" : type === "error" ? "close" : type === "warning" ? "flag" : "terminal";
</script>

<div class="toast-container" aria-live="polite">
  {#each $toasts as toast (toast.id)}
    <div class="toast" class:success={toast.type === "success"} class:error={toast.type === "error"} class:warning={toast.type === "warning"} class:info={toast.type === "info"}>
      <span class="ic"><Icon name={iconFor(toast.type)} size={13} /></span>
      <span class="msg">{toast.message}</span>
      <button class="dismiss" onclick={() => dismissToast(toast.id)} aria-label="Dismiss">×</button>
    </div>
  {/each}
</div>

<style>
  .toast-container {
    position: fixed;
    bottom: calc(var(--status-h) + 12px);
    right: 12px;
    z-index: 3000;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 360px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-left: 3px solid var(--text-dim);
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    color: var(--text);
    box-shadow: var(--shadow-pop);
    pointer-events: auto;
    animation: slideIn var(--motion-normal) var(--ease-tactile);
  }

  .ic {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--text-dim);
  }

  .msg {
    flex: 1;
    line-height: 1.35;
  }

  .toast.success {
    border-left-color: var(--accent);
  }
  .toast.success .ic {
    color: var(--accent);
  }

  .toast.error {
    border-left-color: var(--danger);
  }
  .toast.error .ic {
    color: var(--danger);
  }

  .toast.warning {
    border-left-color: var(--amber);
  }
  .toast.warning .ic {
    color: var(--amber);
  }

  .toast.info {
    border-left-color: var(--info);
  }
  .toast.info .ic {
    color: var(--info);
  }

  .dismiss {
    background: none;
    border: none;
    padding: 0 4px;
    font-size: 18px;
    line-height: 1;
    color: var(--text-dim);
    flex-shrink: 0;
  }

  .dismiss:hover {
    color: var(--text);
  }

  @keyframes slideIn {
    from {
      opacity: 0;
      transform: translateX(12px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }
</style>
