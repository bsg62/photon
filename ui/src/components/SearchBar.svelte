<script lang="ts">
  import { library } from '../lib/library.svelte';
  import { mainPage } from '../lib/main-page.svelte';
  import { searchBox } from '../lib/search-box.svelte';
  import { canSaveSearch, defaultSearchName, savedSearchFor } from '../lib/searches';
  import Icon from './Icon.svelte';

  // This component only renders the machine in `search-box.svelte.ts`; the folder tree
  // drives the same instance, which is why none of that state lives here.

  let {
    onleave,
  }: {
    /** The keyboard is done with the box - Enter, or Escape on a box with nothing to clear -
     *  and wants the photos: App knows which of its pages is showing them. */
    onleave: () => void;
  } = $props();

  let input = $state<HTMLInputElement | undefined>();

  /** Ctrl+F and `/` (App): the caret in the box, with what it holds selected, so typing
   *  replaces the last search and an arrow key keeps it to refine. */
  export function focus() {
    input?.focus();
    input?.select();
  }

  /** The saved search the box already holds, if any. Drives both the filled bookmark and
   *  the refusal to save the same query twice. */
  const already = $derived(savedSearchFor(library.searches, searchBox.query));
  const canSave = $derived(canSaveSearch(library.searches, searchBox.query));

  /** Saving names the search after the query itself; the sidebar's right-click menu is
   *  where it gets a friendlier name. Nothing is offered to save an empty box. */
  function save() {
    const query = searchBox.query;
    if (!canSaveSearch(library.searches, query)) return;
    library.saveSearch(defaultSearchName(query), query).catch(library.reportError);
  }
</script>

<div class="bar">
  <div class="field">
    <Icon name="search" size={14} />
    <input
      class="search"
      type="search"
      placeholder="Search names, camera, keywords, dates…"
      aria-label="Search photos by file or folder name, camera, lens, keyword or date"
      title="Every word must match: a name, a folder, a camera or lens, a keyword, 50mm, f/1.8, iso400, or a date like 2024-06. Use OR to widen, &quot;quotes&quot; for a phrase, camera:, lens:, tag:, person:, album: or folder: for one field, from:2019-06 or to:2020 for a date range, on:07-14 for that day in any year, is:starred, is:edited or is:video for what a photo is, has:gps for photos that record where they were taken, has:face or faces:2+ for photos with people in them, has:tag, has:caption, has:album or has:person for photos with any, is:duplicate, is:portrait, is:landscape or is:square, size:&gt;10mb, iso:&gt;=1600, aperture:&lt;2, focal:&gt;100 or mp:&lt;2 to compare a number, and a leading - to exclude: -tag:family, or -has:tag for the untagged."
      bind:this={input}
      bind:value={searchBox.query}
      oninput={() => {
        // Typing a search is asking for results, which the People page does not show.
        mainPage.showGrid();
        searchBox.run(searchBox.query);
      }}
      onkeydown={(e) => {
        // An empty box with Search not active has nothing to clear: unconditionally clearing
        // here would call setSearchQuery('') regardless, which is a no-op query but still
        // forces the view to All — kicking the user out of Starred with a keystroke that
        // cleared nothing.
        if (e.key === 'Escape') {
          if (searchBox.query !== '' || library.info.view === 'search') searchBox.clear();
          // Nothing to clear: the second Escape of two, or a box opened by mistake. The way
          // out of it without the mouse, since Tab from here walks the whole top bar first.
          else onleave();
          return;
        }
        // The search is already on its way - it runs as it is typed - so Enter only moves on
        // to its results, where the arrow keys are.
        if (e.key === 'Enter') {
          e.preventDefault();
          onleave();
        }
      }}
    />
    {#if searchBox.query.trim() !== ''}
      <!-- Filled and inert once saved: removing a saved search is done from the sidebar,
           which asks first, so there is no one-click undo of a thing the sidebar guards. -->
      <button
        class="save"
        class:saved={already !== undefined}
        disabled={!canSave}
        aria-label={already ? `Saved as “${already.name}”` : 'Save this search'}
        title={already ? `Saved as “${already.name}”` : 'Save this search'}
        onclick={save}
      >
        <Icon name="bookmark" size={14} filled={already !== undefined} />
      </button>
    {/if}
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
  /* The leading icon sits over the input's left padding; clicks pass through to the input.
     Scoped to a direct child so the bookmark button's own icon, which is nested inside the
     button, keeps its place in the flow instead of being dragged to the left edge too. */
  .field > :global(svg) { position: absolute; left: 9px; pointer-events: none; }
  .search {
    width: 100%;
    box-sizing: border-box;
    /* Right padding clears the bookmark button, which overlaps the field's trailing edge. */
    padding: 6px 30px 6px 30px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    color: var(--text);
    font: inherit;
  }
  .search::placeholder { color: var(--text-dim); }
  /* The global ring, pulled in to hug the field rather than float 2px off it. */
  .search:focus-visible { outline-offset: 0; }
  .save {
    position: absolute;
    right: 4px;
    display: flex;
    padding: var(--s-1);
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
  }
  .save:hover:not(:disabled) { color: var(--text); background: var(--hover); }
  /* Saved: the accent marks it, and the cursor says there is nothing more to do here. */
  .save.saved { color: var(--accent); }
  .save:disabled { cursor: default; }
</style>
