<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { NameChoice } from '../lib/people';

  let {
    choose,
    commit,
    placeholder = 'Add a name',
    label,
    initial = '',
    renaming = false,
    oncancel,
  }: {
    /** What committing `text` would do: `PeoplePageModel.choice`, bound to the person
     *  being renamed if any. */
    choose: (text: string) => NameChoice;
    commit: (text: string) => void | Promise<void>;
    placeholder?: string;
    label: string;
    initial?: string;
    /** Renaming a named person rather than naming a group: a new name replaces theirs
     *  instead of making someone, and the field takes focus as it opens, since the user
     *  just asked for it. */
    renaming?: boolean;
    /** Escape, or Enter on the name already there: how a rename is left. */
    oncancel?: () => void;
  } = $props();

  let input: HTMLInputElement | undefined = $state();
  let text = $state(untrack(() => initial));
  const choice = $derived(choose(text));

  /** Said before the name is committed, because a taken name merges (spec "The People
   *  page"): the user must see "Add to Anna" before pressing Enter. */
  const hint = $derived(
    choice.kind === 'merge' ? `Add to ${choice.name}`
    : choice.kind === 'new' ? (renaming ? `Rename to “${choice.name}”` : `New person “${choice.name}”`)
    : '',
  );

  onMount(() => {
    if (renaming) {
      input?.focus();
      input?.select();
    }
  });

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (choice.kind === 'new' || choice.kind === 'merge')) {
      e.preventDefault();
      const committed = text;
      text = '';
      await commit(committed);
    } else if (e.key === 'Enter' && choice.kind === 'same') {
      e.preventDefault();
      oncancel?.();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      text = initial;
      oncancel?.();
    }
  }
</script>

<div class="namebox">
  <input class="field" bind:this={input} bind:value={text} {placeholder} aria-label={label} {onkeydown} />
  {#if hint}<span class="hint" class:merge={choice.kind === 'merge'}>{hint} · Enter</span>{/if}
</div>

<style>
  .namebox { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  /* FolderTree's `.editor`, on a field ground: the page has no panel for a surface-coloured
     field to stand out from. */
  .field {
    width: 14rem;
    max-width: 100%;
    height: 28px;
    padding: 0 var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    color: inherit;
    font: inherit;
  }
  .field::placeholder { color: var(--text-dim); }
  .field:focus-visible { outline-offset: 0; }
  .hint { color: var(--text-dim); font-size: var(--t-2); }
  /* A merge puts these faces with someone already named: the one outcome the user must not
     commit without noticing, so the one coloured. */
  .hint.merge { color: var(--accent); }
</style>
