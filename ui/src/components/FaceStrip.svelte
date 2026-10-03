<script lang="ts">
  import { tick } from 'svelte';
  import type { FaceAction, PeoplePageModel, StripKey } from '../lib/people-page.svelte';
  import { SINGLE } from '../lib/people-page.svelte';
  import { faceUrl } from '../lib/people';
  import Icon from './Icon.svelte';
  import NameBox from './NameBox.svelte';

  let {
    model,
    key,
    label,
    suggestion = false,
    onopen,
    onfocuslost,
  }: {
    model: PeoplePageModel;
    key: StripKey;
    label: string;
    /** Faces photon thinks are a named person's, not yet confirmed: drawn dashed. */
    suggestion?: boolean;
    onopen: (itemId: number) => void;
    /** Where focus goes when the strip has no face left to take it: the page. */
    onfocuslost?: () => void;
  } = $props();

  const ACTION_LABEL: Record<FaceAction, string> = {
    confirm: 'Confirm',
    reject: 'Not this person',
    ignore: 'Ignore these faces',
    unignore: 'Stop ignoring',
  };

  const faces = $derived(model.faces(key));
  const count = $derived(model.count(key));
  const expanded = $derived(model.isExpanded(key));
  const more = $derived(model.canShowMore(key));
  const selected = $derived(model.selected(key));

  /** Crops the route answered with an error, by face and key. Not retried under the same
   *  key: it is a 404 for a reason a second ask does not change (no cached preview, a face
   *  that is gone, a key that is stale). Keyed by both because a reload can hand the face a
   *  new key - its photo edited, its preview made - and that crop is worth asking for. */
  let failed = $state<Record<string, true>>({});
  const crop = (f: { id: number; thumbKey: string }) => `${f.id}:${f.thumbKey}`;

  let stripEl: HTMLElement | undefined = $state();

  /** Runs an action of the bar, then hands focus on: the action clears the selection, so
   *  the bar - holding the button or field that had focus - unmounts, and focus left on
   *  `<body>` reaches no key until a click. The strip's first remaining face takes it, or
   *  the page when none is left. */
  async function settle(action: () => void | Promise<void>) {
    const done = action();
    await tick();
    const at = document.activeElement;
    if (!at || at === document.body) {
      const face = stripEl?.querySelector<HTMLElement>('button.face');
      if (face) face.focus();
      else onfocuslost?.();
    }
    await done;
  }

  function onkeydown(e: KeyboardEvent, itemId: number) {
    // Enter opens; Space stays the button's own click, which toggles. Without the
    // preventDefault, Enter on a button also clicks it and would toggle as well.
    if (e.key === 'Enter') {
      e.preventDefault();
      onopen(itemId);
    }
  }
</script>

{#if faces.length}
  <div class="strip" role="group" aria-label={label} bind:this={stripEl}>
    <div class="faces">
      {#each faces as f, i (f.id)}
        <button
          class="face"
          class:suggestion
          class:selected={model.isSelected(key, f.id)}
          aria-pressed={model.isSelected(key, f.id)}
          aria-label="Face {i + 1} of {count}"
          onclick={() => model.toggle(key, f.id)}
          ondblclick={() => onopen(f.itemId)}
          onkeydown={(e) => onkeydown(e, f.itemId)}
        >
          {#if failed[crop(f)]}
            <span class="placeholder"><Icon name="user" size={20} /></span>
          {:else}
            <img
              src={faceUrl(f.id, f.thumbKey)}
              alt=""
              width="48"
              height="48"
              draggable="false"
              loading="lazy"
              onerror={() => (failed = { ...failed, [crop(f)]: true })}
            />
          {/if}
        </button>
      {/each}
    </div>
    {#if more || expanded}
      <div class="paging">
        {#if more && !expanded}
          <button class="link" onclick={() => model.showMore(key)}>Show all {count.toLocaleString()}</button>
        {:else if more}
          <button class="link" onclick={() => model.showMore(key)}
            >Show more ({(count - faces.length).toLocaleString()} left)</button
          >
        {/if}
        {#if expanded}
          <button class="link" onclick={() => model.showFewer(key)}>Show fewer</button>
        {/if}
      </div>
    {/if}
    {#if selected.length > 0}
      <div class="actions">
        <span class="selected-count">{selected.length} selected</span>
        {#each model.actionsFor(key) as action (action)}
          <button onclick={() => settle(() => model.act(key, action))}>{ACTION_LABEL[action]}</button>
        {/each}
        {#if key === SINGLE}
          <NameBox choose={(t) => model.choice(t)} commit={(t) => settle(() => model.nameSingles(t))} label="Name the selected faces" />
        {/if}
        <button onclick={() => settle(() => model.clearSelection())}>Clear</button>
        <span class="hint">Double-click a face to open its photo</span>
      </div>
    {/if}
  </div>
{/if}

<style>
  .strip { display: flex; flex-direction: column; gap: var(--s-2); min-width: 0; }
  .faces { display: flex; flex-wrap: wrap; gap: var(--s-1); }
  .face {
    position: relative;
    flex: none;
    width: 48px;
    height: 48px;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    /* What shows while the crop loads. */
    background: var(--field);
    overflow: hidden;
    cursor: pointer;
  }
  img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .placeholder { display: grid; place-items: center; width: 100%; height: 100%; color: var(--text-dim); }
  /* Inside the box, as Tile.svelte draws its ring: the focus ring is drawn outside it, so a
     face that is both focused and selected shows both marks. From the class, not from
     focus, for the same reason. */
  .face.selected::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    box-shadow: inset 0 0 0 2px var(--accent), inset 0 0 0 3px var(--surface);
    pointer-events: none;
  }
  /* A suggestion is a guess: dashed, inside the box like the ring, so the two can coexist. */
  .face.suggestion::before {
    content: '';
    position: absolute;
    inset: 0;
    border: 1px dashed var(--accent);
    border-radius: inherit;
    pointer-events: none;
  }
  .paging { display: flex; gap: var(--s-3); }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    cursor: pointer;
  }
  .link:hover { text-decoration: underline; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s-2); }
  .selected-count { font-weight: 600; }
  .actions button {
    padding: 5px var(--s-3);
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .actions button:hover { background: var(--field-hover); }
  .hint { color: var(--text-dim); font-size: var(--t-2); }
  @media (prefers-reduced-motion: reduce) { .actions button { transition: none; } }
</style>
