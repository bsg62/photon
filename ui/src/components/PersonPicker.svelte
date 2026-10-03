<script lang="ts">
  import { library } from '../lib/library.svelte';
  import type { PersonPicker } from '../lib/person-picker.svelte';
  import Icon from './Icon.svelte';

  let { picker, onclosed }: { picker: PersonPicker; onclosed: () => void } = $props();

  let field = $state<HTMLInputElement | undefined>();

  // Focus the field as soon as the dialog appears, and only then: the grid or the viewer
  // keeps the keys while it is closed. `picker.visible` is the dependency.
  $effect(() => {
    if (picker.visible) field?.focus();
  });

  /** The people a name can land on: the named ones, by id. A `c:` key is a Picasa contact
   *  no person is linked to - typing its name makes a new person, which the backend then
   *  links to it by that name. */
  const named = $derived(
    library.people.filter((p) => p.key.startsWith('p:')).map((p) => ({ id: Number(p.key.slice(2)), name: p.name })),
  );
  const suggestions = $derived(picker.suggestions(named));
  const hint = $derived(picker.hint(named));

  /** Closes and hands focus back, through the caller: only it knows whether the grid or the
   *  viewer is behind the dialog, and the field holding focus is about to leave the DOM. */
  function dismiss() {
    picker.close();
    onclosed();
  }

  /** One write, then the toast, as the keyword dialog does: the picker closes itself before
   *  the call lands, so a failure goes to the toast corner. A submit that does nothing (a
   *  blank name, a write still in flight) leaves the dialog up, hence the condition. */
  function submit(name?: string) {
    const written = picker.submit(name);
    if (!picker.visible) onclosed();
    written.then((message) => message && library.notify(message)).catch(library.reportError);
  }

  function onkeydown(e: KeyboardEvent) {
    // Enter on a suggestion or on Cancel is that button's own click; read as a submit of
    // the draft, it would be prevented and name the photo after what was typed instead.
    // Its propagation is still stopped: the viewer listens for keys on the window.
    if (e.key === 'Enter' && e.target !== field) {
      e.stopPropagation();
      return;
    }
    const answer = picker.keydown(e);
    if (answer === 'close') dismiss();
    else if (answer === 'submit') submit();
  }
</script>

{#if picker.visible}
  <!-- The backdrop is a mouse convenience; Escape and Cancel are the accessible ways out.
       Its mousedown goes no further: the viewer closes on the mouse's back button from the
       window, and would close beneath a dialog that is about to hand focus back to it. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onmousedown={(e) => e.stopPropagation()}
    onclick={(e) => e.target === e.currentTarget && dismiss()}
  >
    <div
      class="dialog focus-container"
      role="dialog"
      aria-modal="true"
      aria-labelledby="person-picker-title"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h1 id="person-picker-title">{picker.title}</h1>
        <button class="close" aria-label="Cancel" onclick={dismiss}>
          <Icon name="x" size={16} />
        </button>
      </header>

      <div class="row">
        <input
          bind:this={field}
          bind:value={picker.draft}
          type="text"
          placeholder="Name"
          aria-label="Name"
          aria-describedby="person-picker-hint"
          autocomplete="off"
          spellcheck="false"
        />
        <button class="primary" disabled={!picker.draft.trim() || picker.busy} onclick={() => submit()}>Name</button>
      </div>
      <!-- What Enter will do, by the backend's own rule: a name someone already has, in any
           case, is that person, not a second one. -->
      <p class="hint" id="person-picker-hint">{hint || 'Type a name, or choose someone below.'}</p>

      <!-- A group of buttons, not a listbox, for the keyword dialog's reason: separate tab
           stops, no roving, nothing selected among them. -->
      <div class="list" role="group" aria-label="People">
        {#each suggestions as person (person.id)}
          <button onclick={() => submit(person.name)}>{person.name}</button>
        {:else}
          <p class="none">{named.length === 0 ? 'No one is named yet.' : 'No one matches what you typed.'}</p>
        {/each}
      </div>

      <footer>
        <button class="secondary" onclick={dismiss}>Cancel</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  /* Above the viewer (20), which the dialog is opened over to name a face. */
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
    color: var(--text);
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
  .row { display: flex; gap: var(--s-2); }
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
  .primary,
  .secondary {
    flex: none;
    padding: 6px var(--s-3);
    border: 0;
    border-radius: var(--r-2);
    font: inherit;
    cursor: pointer;
  }
  .primary { background: var(--accent); color: var(--on-accent); }
  .primary:disabled { background: var(--field); color: var(--text-dim); cursor: default; }
  .secondary { background: var(--field); color: var(--text); }
  .secondary:hover { background: var(--hover); }
  .hint { margin: calc(-1 * var(--s-2)) 0 0; color: var(--text-dim); font-size: var(--t-2); }
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
  footer { display: flex; justify-content: flex-end; }
</style>
