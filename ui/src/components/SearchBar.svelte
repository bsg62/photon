<script lang="ts">
  import { tick } from 'svelte';
  import { library } from '../lib/library.svelte';
  import { mainPage } from '../lib/main-page.svelte';
  import { searchBox } from '../lib/search-box.svelte';
  import { insertTerm } from '../lib/search-help';
  import { canSaveSearch, defaultSearchName, savedSearchFor } from '../lib/searches';
  import Icon from './Icon.svelte';
  import SearchHelp from './SearchHelp.svelte';

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
  let field = $state<HTMLDivElement | undefined>();
  let helpButton = $state<HTMLButtonElement | undefined>();
  const helpId = $props.id();
  /** The panel listing what the box understands (`SearchHelp`), under the box. */
  let help = $state(false);

  /** Escape, from the panel or from its button: the focus goes back to the button, which
   *  is where it was before the panel had any of it. */
  function closeHelp() {
    help = false;
    helpButton?.focus();
  }

  /** A term clicked in the panel goes into the box as though it had been typed there: the
   *  same debounced send, the same leaving of the People page. Then the caret, after it -
   *  a prefix like `camera:` is half a term, and the rest is the user's to type. */
  async function pick(term: string) {
    help = false;
    const next = insertTerm(searchBox.query, term);
    searchBox.query = next;
    mainPage.showGrid();
    searchBox.run(next);
    // The box takes the new text on the next render, and the caret can only go to the end
    // of what is there.
    await tick();
    input?.focus();
    input?.setSelectionRange(next.length, next.length);
  }

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

<!-- A press anywhere but the box and its panel closes the panel, as leaving a menu does. -->
<svelte:window onpointerdown={(e) => help && !field?.contains(e.target as Node) && (help = false)} />

<div class="bar">
  <div class="field" bind:this={field}>
    <Icon name="search" size={14} />
    <input
      class="search"
      class:holding={searchBox.query.trim() !== ''}
      type="search"
      placeholder="Search names, camera, keywords, dates…"
      aria-label="Search photos by file or folder name, camera, lens, keyword or date"
      bind:this={input}
      bind:value={searchBox.query}
      oninput={() => {
        // Typing a search is asking for results, which the People page does not show.
        mainPage.showGrid();
        searchBox.run(searchBox.query);
      }}
      onkeydown={(e) => {
        // A key that confirms or cancels an input method's composition is the composition's.
        if (e.isComposing) return;
        // An empty box with Search not active has nothing to clear: unconditionally clearing
        // here would call setSearchQuery('') regardless, which is a no-op query but still
        // forces the view to All — kicking the user out of Starred with a keystroke that
        // cleared nothing.
        if (e.key === 'Escape') {
          // The panel first, as any Escape closes the nearest thing: the search stays.
          if (help) help = false;
          else if (searchBox.query !== '' || library.info.view === 'search') searchBox.clear();
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
    <!-- What the box understands. Before the bookmark in the tab order and after it on
         screen, so it is always the field's last thing and the panel's terms follow it. -->
    <button
      class="help"
      class:open={help}
      bind:this={helpButton}
      aria-label="What you can search for"
      title="What you can search for"
      aria-expanded={help}
      aria-controls={help ? helpId : undefined}
      onclick={() => (help = !help)}
      onkeydown={(e) => {
        if (e.key === 'Escape' && help) {
          e.stopPropagation();
          closeHelp();
        }
      }}
    >
      <Icon name="circle-help" size={14} />
    </button>
    {#if help}
      <SearchHelp id={helpId} onpick={pick} onclose={closeHelp} />
    {/if}
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
    /* Right padding clears the help button, which overlaps the field's trailing edge. */
    padding: 6px 30px 6px 30px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    color: var(--text);
    font: inherit;
  }
  /* And the bookmark beside it, which is there only while the box holds something. */
  .search.holding { padding-right: 54px; }
  .search::placeholder { color: var(--text-dim); }
  /* The global ring, pulled in to hug the field rather than float 2px off it. */
  .search:focus-visible { outline-offset: 0; }
  .save, .help {
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
  /* The bookmark stands inside the help button, which is always there. */
  .save { right: 28px; }
  .save:hover:not(:disabled), .help:hover, .help.open { color: var(--text); background: var(--hover); }
  /* Saved: the accent marks it, and the cursor says there is nothing more to do here. */
  .save.saved { color: var(--accent); }
  .save:disabled { cursor: default; }
</style>
