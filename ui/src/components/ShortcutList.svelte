<script lang="ts">
  import { chordLabel, SHORTCUTS } from '../lib/shortcuts';
  import { isMac } from '../lib/url';

  /** `wide` lays the groups out in two columns, for the sheet; Settings' section is too
   *  narrow for a second one. */
  let { wide = false }: { wide?: boolean } = $props();

  const mac = isMac();
</script>

<div class="groups" class:wide>
  {#each SHORTCUTS as group (group.id)}
    <section>
      <h2>{group.title}</h2>
      <dl>
        {#each group.rows as row (row.does)}
          <div class="row">
            <dt>
              {#each row.keys as chord, n (n)}
                <span class="chord">
                  {#each chordLabel(chord, mac) as key, i (i)}{#if i > 0}<span class="plus">+</span>{/if}<kbd>{key}</kbd>{/each}
                </span>
              {/each}
            </dt>
            <dd>{row.does}</dd>
          </div>
        {/each}
      </dl>
    </section>
  {/each}
</div>

<style>
  .groups.wide { columns: 2; column-gap: var(--s-6); }
  /* A group is read as one block: never split over the two columns. */
  section { break-inside: avoid; padding-bottom: var(--s-4); }
  h2 { margin: 0 0 var(--s-1); font-size: var(--t-3); font-weight: 600; }
  dl { margin: 0; }
  .row {
    display: grid;
    /* Narrow enough that the descriptions keep to one line in the sheet's two columns; the
       one row of keys wider than this (Shift+← beside Shift+→) wraps onto a second. */
    grid-template-columns: 9.5rem minmax(0, 1fr);
    align-items: baseline;
    gap: var(--s-3);
    padding: 3px 0;
  }
  dt { display: flex; flex-wrap: wrap; gap: 4px var(--s-2); }
  dd { margin: 0; color: var(--text-dim); }
  .chord { display: inline-flex; align-items: center; gap: 2px; white-space: nowrap; }
  .plus { color: var(--text-dim); font-size: var(--t-2); }
  kbd {
    min-width: 1.6em;
    padding: 1px 6px;
    border-radius: var(--r-2);
    background: var(--field);
    box-shadow: inset 0 0 0 1px var(--line);
    color: var(--text);
    font: inherit;
    font-size: var(--t-2);
    text-align: center;
  }
</style>
