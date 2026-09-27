<script lang="ts">
  import type { SortKey } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import Icon from './Icon.svelte';

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
  <span class="picker">
    <select
      aria-label="Sort by"
      value={sort.key}
      onchange={(e) => library.setSort({ key: e.currentTarget.value as SortKey, reverse: sort.reverse })}
    >
      {#each KEYS as option (option.value)}
        <option value={option.value}>{option.label}</option>
      {/each}
    </select>
    <span class="chevron"><Icon name="chevron-down" size={12} /></span>
  </span>
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
  .sort { display: inline-flex; flex: 0 0 auto; gap: 2px; padding: 2px; border-radius: var(--r-3); background: var(--field); }
  /* `appearance: none` and a chevron of our own because the webviews draw a native select in
     the platform's control theme - a GTK combo box under WebKitGTK, a Windows control under
     WebView2 - rather than from `color-scheme` and the background set here: a light button
     in the dark top bar. The screenshots' Chromium on Linux themes it correctly, so it never
     showed there; only the running app on Linux and Windows did. */
  .picker { position: relative; display: inline-flex; align-items: center; }
  .chevron { position: absolute; right: 8px; display: inline-flex; pointer-events: none; }
  select {
    appearance: none;
    padding: 4px 26px 4px 8px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  /* The open list, which `appearance: none` does not reach. WebView2 honours these; where a
     platform draws its own menu (WebKitGTK's GTK popup, macOS) it follows the platform's
     theme instead. */
  option { background: var(--surface); color: var(--text); }
  button {
    display: inline-flex;
    align-items: center;
    padding: 4px 8px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: inherit;
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  select:hover, button:hover { background: var(--hover); }
  /* Spelled with the hover state so the pressed toggle keeps its accent under the pointer, as
     the size control's chosen segment does. */
  button.checked, button.checked:hover { background: var(--accent); color: var(--on-accent); }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
