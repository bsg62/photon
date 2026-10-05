<script lang="ts">
  import Icon from './Icon.svelte';
  import ShortcutList from './ShortcutList.svelte';

  let { onclose }: { onclose: () => void } = $props();

  let dialog = $state<HTMLDivElement | undefined>();

  // Focus moves into the sheet as it appears: what is behind it is inert, and the keys
  // below are read here.
  $effect(() => {
    dialog?.focus();
  });

  function onkeydown(e: KeyboardEvent) {
    // No key goes further, for the person dialog's reason: the viewer listens on the window,
    // and an arrow pressed to scroll this list would step past the photo behind it.
    e.stopPropagation();
    // `?` closes what it opened; a chord is not that key (`opensShortcuts`).
    if (e.key === 'Escape' || (e.key === '?' && !e.ctrlKey && !e.metaKey && !e.altKey)) {
      e.preventDefault();
      onclose();
    }
  }
</script>

<!-- The backdrop is a mouse convenience; Escape and the close button are the accessible ways
     out. Its mousedown goes no further, as the person dialog's does not: the viewer closes on
     the mouse's back button from the window. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onmousedown={(e) => e.stopPropagation()} onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog focus-container"
    role="dialog"
    aria-modal="true"
    aria-labelledby="shortcuts-title"
    tabindex="-1"
    bind:this={dialog}
    {onkeydown}
  >
    <header>
      <h1 id="shortcuts-title">Keyboard shortcuts</h1>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={16} /></button>
    </header>
    <div class="body"><ShortcutList wide /></div>
  </div>
</div>

<style>
  /* Above the viewer (20) and compare, which the sheet is opened over. */
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
    width: min(820px, 100%);
    max-height: 100%;
    /* Clips the header's chrome to the rounded corners. */
    overflow: hidden;
    background: var(--surface);
    color: var(--text);
    border-radius: var(--r-4);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-dialog);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px var(--s-3) 10px var(--s-4);
    background: var(--chrome);
    border-bottom: 1px solid var(--line);
  }
  h1 { margin: 0; font-size: var(--t-4); font-weight: 600; }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--r-3);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
  }
  .close:hover { color: var(--text); background: var(--hover); }
  /* The list is longer than a small window: it scrolls, the header stays. */
  .body { min-height: 0; padding: var(--s-4) var(--s-4) 0; overflow: auto; }
</style>
