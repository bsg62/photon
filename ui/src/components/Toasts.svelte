<script lang="ts">
  import { library } from '../lib/library.svelte';
  import Icon from './Icon.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each library.toasts as toast (toast.id)}
    <!-- `alert` interrupts a screen reader, `status` waits its turn: a failure is worth the
         interruption and a report of something that worked is not. -->
    <div class="toast" class:done={toast.kind === 'done'} role={toast.kind === 'error' ? 'alert' : 'status'}>
      <span>{toast.message}</span>
      <button onclick={() => library.dismissToast(toast.id)} aria-label="Dismiss"><Icon name="x" size={14} /></button>
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
  .toast.done { border-left-color: var(--accent); }
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
