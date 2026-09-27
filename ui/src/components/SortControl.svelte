<script lang="ts">
  import type { SortKey } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import Icon from './Icon.svelte';
  import Select from './Select.svelte';

  const KEYS: { value: SortKey; label: string }[] = [
    { value: 'date', label: 'Date taken' },
    { value: 'modified', label: 'Date modified' },
    { value: 'name', label: 'Name' },
    { value: 'size', label: 'Size' },
  ];

  const sort = $derived(library.sort);
</script>

<!-- Holds no sort of its own: `library.sort` is the change in flight, else the grid's, so a
     quick second change builds on the first and a refused one puts the control back. -->
<div class="sort" role="group" aria-label="Sort">
  <Select label="Sort by" options={KEYS} value={sort.key} onchange={(key) => library.setSort({ key, reverse: sort.reverse })} />
  <button
    aria-label="Reverse order"
    title="Reverse order"
    aria-pressed={sort.reverse}
    class:checked={sort.reverse}
    onclick={() => library.setSort({ key: sort.key, reverse: !sort.reverse })}
  >
    <Icon name="arrow-down-up" size={14} />
  </button>
</div>

<style>
  /* `flex: 0 0 auto` for the same reason as the size control beside it: the search bar is
     the top bar's one child meant to give way. */
  .sort { display: inline-flex; flex: 0 0 auto; align-items: center; gap: var(--s-1); }
  /* The gear's ghost button (App.svelte), so the two icon buttons in the bar are one kind. */
  button {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--r-3);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  button:hover { color: var(--text); background: var(--hover); }
  /* Spelled with the hover state so the pressed toggle keeps its accent under the pointer, as
     the size control's chosen segment does. */
  button.checked, button.checked:hover { background: var(--accent); color: var(--on-accent); }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
