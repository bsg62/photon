<script lang="ts">
  import { untrack } from 'svelte';
  import { library } from '../lib/library.svelte';
  import { searchBox } from '../lib/search-box.svelte';
  import Icon from './Icon.svelte';

  // This component only renders the machine in `search-box.svelte.ts`; the folder tree
  // drives the same instance, which is why none of that state lives here.
  $effect(() => {
    const backend = library.info.searchQuery;
    // Registers `outstanding` as a dependency. `syncFromBackend` declines every echo until
    // the count reaches zero, so the effect has to re-run when it gets there — and the read
    // below is untracked, so this is the only place that dependency can be established.
    void searchBox.outstanding;
    // `syncFromBackend` reads `query`, and an untracked call keeps it out of this effect's
    // dependencies. A direct dependency would re-run the effect on every keystroke (which
    // writes `query` through `bind:value`) — and since the effect can also write `query`,
    // that write would re-trigger the effect it happened inside. It would settle rather than
    // loop (the second run sees backend === query and stops), but there is no reason to pay
    // for it.
    untrack(() => searchBox.syncFromBackend(backend));
  });
</script>

<div class="bar">
  <div class="field">
    <Icon name="search" size={14} />
    <input
      class="search"
      type="search"
      placeholder="Search names, camera, keywords, dates…"
      aria-label="Search photos by file or folder name, camera, lens, keyword or date"
      title="Every word must match: a name, a folder, a camera or lens, a keyword, 50mm, f/1.8, iso400, or a date like 2024-06. Use OR to widen, &quot;quotes&quot; for a phrase, camera: or lens: for one field."
      bind:value={searchBox.query}
      oninput={() => searchBox.run(searchBox.query)}
      onkeydown={(e) => {
        // An empty box with Search not active has nothing to clear: unconditionally clearing
        // here would call setSearchQuery('') regardless, which is a no-op query but still
        // forces the view to All — kicking the user out of Starred with a keystroke that
        // cleared nothing.
        if (e.key === 'Escape' && (searchBox.query !== '' || library.info.view === 'search')) searchBox.clear();
      }}
    />
  </div>
</div>

<style>
  /* Background and border belong to the top bar in App, which also holds the settings gear. */
  .bar { display: flex; flex: 1; min-width: 0; padding: var(--s-2); }
  .field {
    position: relative;
    display: flex;
    align-items: center;
    width: 320px;
    max-width: 100%;
    color: var(--text-dim);
  }
  /* The icon sits over the input's left padding; clicks pass through to the input. */
  .field :global(svg) { position: absolute; left: 9px; pointer-events: none; }
  .search {
    width: 100%;
    box-sizing: border-box;
    padding: 6px var(--s-2) 6px 30px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    color: var(--text);
    font: inherit;
  }
  .search::placeholder { color: var(--text-dim); }
  /* The global ring, pulled in to hug the field rather than float 2px off it. */
  .search:focus-visible { outline-offset: 0; }
</style>
