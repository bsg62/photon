<script lang="ts">
  import { library } from '../lib/library.svelte';
  import Icon from './Icon.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each library.errors as toast (toast.id)}
    <div class="toast" role="alert">
      <span>{toast.message}</span>
      <button onclick={() => library.dismissError(toast.id)} aria-label="Dismiss"><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: var(--s-4); bottom: 40px; display: flex; flex-direction: column; gap: var(--s-2); z-index: 30; }
  .toast {
    display: flex;
    gap: var(--s-3);
    align-items: center;
    max-width: 420px;
    padding: 10px var(--s-3);
    background: var(--raised);
    border-left: 3px solid var(--danger);
    border-radius: var(--r-2);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .toast button {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
  }
  .toast button:hover { color: var(--text); background: var(--hover); }
</style>
