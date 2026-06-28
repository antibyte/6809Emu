<script lang="ts">
  import { toasts, dismissToast } from "../toast";
</script>

<div class="toast-container" aria-live="polite">
  {#each $toasts as toast (toast.id)}
    <div class="toast" class:success={toast.type === "success"} class:error={toast.type === "error"} class:warning={toast.type === "warning"}>
      <span>{toast.message}</span>
      <button class="dismiss" onclick={() => dismissToast(toast.id)} aria-label="Dismiss">×</button>
    </div>
  {/each}
</div>

<style>
  .toast-container {
    position: fixed;
    bottom: 16px;
    right: 16px;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 360px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 13px;
    color: var(--text);
    box-shadow: var(--shadow);
    pointer-events: auto;
    animation: slideIn 0.2s ease;
  }

  .toast.success {
    border-color: var(--accent-dim);
    color: var(--accent);
  }

  .toast.error {
    border-color: rgba(255, 71, 87, 0.5);
    color: var(--danger);
  }

  .toast.warning {
    border-color: rgba(255, 176, 0, 0.5);
    color: var(--accent-amber);
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
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>