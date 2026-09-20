<script lang="ts">
  import type { ExportDialog } from '../lib/export-dialog.svelte';
  import { library } from '../lib/library.svelte';
  import Icon from './Icon.svelte';

  let { dialog, onclosed }: { dialog: ExportDialog; onclosed: () => void } = $props();

  let chooser = $state<HTMLButtonElement | undefined>();

  // Focus the folder button as soon as the dialog appears: choosing a destination is the
  // one thing that has to happen before anything else can.
  $effect(() => {
    if (dialog.visible) chooser?.focus();
  });

  /** Closes and hands focus back; the caller knows where focus belongs once the dialog has
   *  gone, and the button that has it is about to leave the DOM. */
  function dismiss() {
    dialog.close();
    onclosed();
  }

  function choose() {
    dialog.choose().catch(library.reportError);
  }

  function submit() {
    const running = dialog.submit();
    if (!dialog.visible) onclosed();
    running.then((message) => message && library.notify(message)).catch(library.reportError);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      // This Escape belongs to the dialog in front, not to the window handlers behind it.
      e.stopPropagation();
      dismiss();
    }
  }
</script>

{#if dialog.visible}
  <!-- The backdrop is a mouse convenience; Escape and Cancel are the accessible ways out. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={(e) => e.target === e.currentTarget && dismiss()}>
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="export-title"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h1 id="export-title">
          Export {dialog.count === 1 ? '1 photo' : `${dialog.count.toLocaleString()} photos`}
        </h1>
        <button class="close" aria-label="Cancel" onclick={dismiss}><Icon name="x" size={16} /></button>
      </header>

      <button class="dest" bind:this={chooser} onclick={choose} class:refused={dialog.problem !== null}>
        <Icon name="folder" size={16} />
        <span class:placeholder={dialog.dest === null}>{dialog.dest ?? 'Choose a folder…'}</span>
      </button>
      {#if dialog.problem}
        <!-- Against the field, while the folder is still in hand: photon refuses a
             destination inside a watched folder, and that is worth saying before the export
             rather than after it. -->
        <p class="problem" role="alert">{dialog.problem}</p>
      {/if}

      <label class="option">
        <input
          type="checkbox"
          checked={dialog.applyEdits}
          onchange={(e) => void dialog.setApplyEdits(e.currentTarget.checked).catch(library.reportError)}
        />
        <span>Apply edits to the copies</span>
      </label>
      <!-- Always shown, because it is the one surprise in the feature: photon reads camera
           information and never writes it, so a re-encoded copy cannot carry it. An
           unedited photo is copied byte for byte either way and keeps everything. -->
      <p class="note">
        A photo you have turned or cropped is re-encoded, and the copy carries no camera
        information. Photos you have not edited are copied exactly as they are.
      </p>

      <div class="actions">
        <button class="ghost" onclick={dismiss}>Cancel</button>
        <button class="primary" disabled={dialog.dest === null || dialog.busy} onclick={submit}>Export</button>
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
    /* Definite tracks, for the reason Settings' backdrop gives. */
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    place-items: center;
    padding: var(--s-4);
    background: var(--scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    width: min(440px, 100%);
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
  .dest {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-2) var(--s-3);
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .dest:hover { background: var(--field-hover); }
  .dest.refused { box-shadow: inset 0 0 0 1px var(--danger); }
  .problem { margin: 0; color: var(--danger); font-size: var(--t-2); }
  /* The path is the only part that can be any length, and it is the end of it that says
     which folder this is. */
  .dest span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; }
  .dest .placeholder { color: var(--text-dim); direction: ltr; }
  .option { display: flex; align-items: center; gap: var(--s-2); cursor: pointer; }
  .note { margin: 0; color: var(--text-dim); font-size: var(--t-2); }
  .actions { display: flex; justify-content: flex-end; gap: var(--s-2); }
  .actions button {
    padding: 6px var(--s-3);
    border: 0;
    border-radius: var(--r-2);
    font: inherit;
    cursor: pointer;
  }
  .ghost { background: none; color: var(--text); }
  .ghost:hover { background: var(--hover); }
  .primary { background: var(--accent); color: var(--on-accent); }
  .primary:disabled { background: var(--field); color: var(--text-dim); cursor: default; }
</style>
