<script lang="ts">
  import { library } from '../lib/library.svelte';
  import type { TagPicker } from '../lib/tag-picker.svelte';
  import Icon from './Icon.svelte';

  let { picker, onclosed }: { picker: TagPicker; onclosed: () => void } = $props();

  let field = $state<HTMLInputElement | undefined>();

  // Focus the field as soon as the dialog appears, and only then: the grid keeps the keys
  // while it is closed. `picker.visible` is the dependency; the element arrives with it.
  $effect(() => {
    if (picker.visible) field?.focus();
  });

  const suggestions = $derived(picker.suggestions(library.tags.map((t) => t.tag)));

  /** Closes and hands focus back. Every way out of the dialog goes through here or through
   *  `submit`, because the caller is what knows where focus belongs once the dialog has
   *  gone - the field that has it is about to leave the DOM, and focus would fall to
   *  `<body>`, where the grid's keys do not reach. */
  function dismiss() {
    picker.close();
    onclosed();
  }

  /** One write, then the toast. The picker closes itself synchronously before the call
   *  lands, so the failure path has no dialog left to put an error in - it goes to the same
   *  corner the report would have. A submit that does nothing (a blank name) leaves the
   *  dialog up, which is why the hand-back is conditional. */
  function submit(tag?: string) {
    const written = picker.submit(tag);
    if (!picker.visible) onclosed();
    written.then((message) => message && library.notify(message)).catch(library.reportError);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      // This Escape belongs to the dialog in front, not to the window handlers behind it:
      // the grid closes its context menu on a window-level Escape.
      e.stopPropagation();
      dismiss();
    }
  }
</script>

{#if picker.visible}
  <!-- The backdrop is a mouse convenience; Escape and Cancel are the accessible ways out,
       so it needs no role or key handler of its own. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={(e) => e.target === e.currentTarget && dismiss()}>
    <div
      class="dialog focus-container"
      role="dialog"
      aria-modal="true"
      aria-labelledby="tag-picker-title"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h1 id="tag-picker-title">
          {picker.mode === 'add' ? 'Add a keyword to' : 'Remove a keyword from'}
          {picker.count === 1 ? '1 photo' : `${picker.count.toLocaleString()} photos`}
        </h1>
        <button class="close" aria-label="Cancel" onclick={dismiss}>
          <Icon name="x" size={16} />
        </button>
      </header>

      <!-- A form, so Enter submits from the field without a keydown handler of its own. -->
      <form
        onsubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <input
          bind:this={field}
          bind:value={picker.draft}
          type="text"
          placeholder={picker.mode === 'add' ? 'New or existing keyword' : 'Keyword to remove'}
          aria-label={picker.mode === 'add' ? 'Keyword to add' : 'Keyword to remove'}
          autocomplete="off"
          spellcheck="false"
        />
        <button type="submit" disabled={!picker.draft.trim() || picker.busy}>
          {picker.mode === 'add' ? 'Add' : 'Remove'}
        </button>
      </form>

      <!-- The library's own keywords, narrowed as you type. This is the point of the dialog
           over a bare prompt: it is how you avoid a second “Beach” beside “beach”. -->
      <!-- A group of buttons, not a `listbox`: the options are separate tab stops with no
           arrow-key roving and nothing selected among them, so the listbox roles would
           describe a widget this is not. -->
      <div class="list" role="group" aria-label="Existing keywords">
        {#each suggestions as tag (tag)}
          <button onclick={() => submit(tag)}>{tag}</button>
        {:else}
          <p class="none">
            {library.tags.length === 0
              ? 'No keywords in the library yet.'
              : 'No keyword matches what you typed.'}
          </p>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    /* Definite tracks, for the reason Settings' backdrop gives: an implicit row grows to the
       dialog's own content height and centres it off the screen. */
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    place-items: center;
    padding: var(--s-4);
    background: var(--scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    width: min(420px, 100%);
    max-height: min(480px, 100%);
    padding: var(--s-4);
    background: var(--surface);
    border-radius: var(--r-4);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-dialog);
  }
  header { display: flex; align-items: center; gap: var(--s-3); }
  h1 { flex: 1; margin: 0; font-size: var(--t-4); font-weight: 600; }
  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
  }
  .close:hover { color: var(--text); background: var(--hover); }
  form { display: flex; gap: var(--s-2); }
  input {
    flex: 1;
    min-width: 0;
    padding: 6px var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    color: var(--text);
    font: inherit;
  }
  input::placeholder { color: var(--text-dim); }
  form button {
    flex: none;
    padding: 6px var(--s-3);
    border: 0;
    border-radius: var(--r-2);
    background: var(--accent);
    color: var(--on-accent);
    font: inherit;
    cursor: pointer;
  }
  form button:disabled { background: var(--field); color: var(--text-dim); cursor: default; }
  .list { display: flex; flex-direction: column; min-height: 0; overflow-y: auto; }
  .list button {
    padding: 6px var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .list button:hover { background: var(--hover); }
  .none { margin: 0; padding: 6px var(--s-2); color: var(--text-dim); font-size: var(--t-2); }
</style>
