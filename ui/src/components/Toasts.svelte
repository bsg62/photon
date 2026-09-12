<script lang="ts">
  import { library } from '../lib/library.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each library.errors as toast (toast.id)}
    <div class="toast" role="alert">
      <span>{toast.message}</span>
      <button onclick={() => library.dismissError(toast.id)} aria-label="Dismiss">✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 40px; display: flex; flex-direction: column; gap: 8px; z-index: 30; }
  .toast {
    display: flex;
    gap: 12px;
    align-items: center;
    max-width: 420px;
    padding: 10px 12px;
    background: var(--panel-2);
    border-left: 3px solid var(--danger);
    border-radius: 4px;
    box-shadow: 0 4px 16px #0006;
  }
  .toast button { border: 0; background: none; color: var(--muted); cursor: pointer; }
</style>
