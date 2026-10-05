<script lang="ts">
  import { SEARCH_HELP } from '../lib/search-help';

  /** What the search box understands, as a panel under it (`SearchBar` opens and places it).
   *  Not a modal: the box stays usable beside it, and nothing behind is inert, so it is not
   *  one of App's overlays - it closes on Escape, on a click elsewhere and on a pick. */
  let {
    id,
    onpick,
    onclose,
  }: {
    id: string;
    /** A term was clicked: `SearchBar` puts it in the box. */
    onpick: (term: string) => void;
    onclose: () => void;
  } = $props();

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    // Stopped as well as prevented: this Escape closes the panel and nothing else - not a
    // sidebar menu, not the grid's selection.
    e.preventDefault();
    e.stopPropagation();
    onclose();
  }
</script>

<!-- A dialog in the ARIA sense, so its keydown is a widget's own. -->
<div {id} class="help focus-container" role="dialog" aria-label="What the search box understands" tabindex="-1" {onkeydown}>
  <p class="lead">Everything typed must match. Click a term to add it to the search.</p>
  <div class="groups">
    {#each SEARCH_HELP as group (group.title)}
      <section>
        <h2>{group.title}</h2>
        <dl>
          {#each group.entries as entry (entry.text)}
            <div class="row">
              <dt>
                {#if entry.insert}
                  <button class="term" onclick={() => onpick(entry.text)}>{entry.text}</button>
                {:else}
                  <span class="sample">{entry.text}</span>
                {/if}
              </dt>
              <dd>{entry.does}</dd>
            </div>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
</div>

<style>
  /* The right-click menu's surface, at a reading width. Never taller than the window under
     the top bar: it scrolls instead, like the shortcut sheet. */
  .help {
    position: absolute;
    top: calc(100% + var(--s-2));
    left: 0;
    z-index: 40;
    width: min(760px, calc(100vw - 2 * var(--s-2)));
    max-height: calc(100vh - 64px);
    overflow: auto;
    padding: var(--s-3) var(--s-4) 0;
    background: var(--raised);
    color: var(--text);
    border-radius: var(--r-4);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
    cursor: default;
  }
  .lead { margin: 0 0 var(--s-3); color: var(--text-dim); }
  .groups { columns: 2; column-gap: var(--s-5); }
  /* A group is read as one block: never split over the two columns. */
  section { break-inside: avoid; padding-bottom: var(--s-3); }
  h2 { margin: 0 0 var(--s-1); font-size: var(--t-3); font-weight: 600; }
  dl { margin: 0; }
  .row {
    display: grid;
    grid-template-columns: 7.5rem minmax(0, 1fr);
    align-items: baseline;
    gap: var(--s-2);
    padding: 2px 0;
  }
  dt { min-width: 0; }
  dd { margin: 0; color: var(--text-dim); font-size: var(--t-2); }
  /* What is typed, set apart from what it means: the shortcut sheet's key cap. A term is a
     button and says so under the pointer; a sample is the same cap without the invitation. */
  .term, .sample {
    display: inline-block;
    max-width: 100%;
    padding: 1px 6px;
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    box-shadow: inset 0 0 0 1px var(--line);
    color: var(--text);
    font-size: var(--t-2);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .term { cursor: pointer; }
  .term:hover { background: var(--field-hover); }
  /* `overflow: visible`: clipped, an inline block's baseline is its bottom edge, and the
     sample sat a line above what it means. */
  .sample { box-shadow: none; background: none; padding-left: 0; white-space: normal; overflow: visible; }
</style>
