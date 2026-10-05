<script lang="ts">
  import type { Snippet } from 'svelte';
  import { menuOwnsKey, menuStep } from '../lib/menu-keys';
  import { fitMenu, type Point } from '../lib/menu-place';

  /** photon's context menu: the surface, where it goes, and its keys. The grid's tile menu,
   *  the sidebar's three and the viewer's each had their own copy of all of it, and none of
   *  them answered an arrow key.
   *
   *  It is mounted while open and that is all it knows about being open: whoever opened it
   *  holds the state and closes it - on a click anywhere, from the window, and on Escape,
   *  from the window too or through `onescape`. Its children are plain markup, styled from
   *  here:
   *
   *      <button role="menuitem">Hide photo <span class="hint">H</span></button>
   *      <div class="sep" role="separator"></div>
   *      <div class="heading">Add to album</div>
   *      <button role="menuitem" class="sub">Lisbon</button>
   *      <button role="menuitem" class="danger">Delete…</button>
   *      <div class="note">No albums of your own yet.</div> */
  let {
    at,
    label,
    onescape,
    children,
  }: {
    /** Where the pointer was; `fitMenu` keeps all of the menu on screen from there. */
    at: Point;
    /** The accessible name, for a menu whose items do not say what they are for. */
    label?: string;
    /** Escape pressed in the menu, for an opener with no window listener of its own. The
     *  key then goes no further. */
    onescape?: () => void;
    children: Snippet;
  } = $props();

  let el = $state<HTMLDivElement | undefined>();

  // The menu takes the focus as it opens, so its keys reach it, and hands it back as it
  // closes. Before, a menu closed by Escape left the focus on `<body>`: in the grid that is
  // the arrow keys dead until a click. Handed back only while the focus is still the
  // menu's (or already fell to the body with it): an item that moved it on - into a dialog,
  // back to the grid - has said where it belongs.
  $effect(() => {
    const menu = el;
    if (!menu) return;
    const before = document.activeElement;
    menu.focus();
    return () => {
      const now = document.activeElement;
      const lost = now === null || now === document.body || menu.contains(now);
      if (lost && before instanceof HTMLElement && before.isConnected) before.focus();
    };
  });

  function items(): HTMLElement[] {
    return el ? [...el.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)')] : [];
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && onescape) {
      e.stopPropagation();
      onescape();
      return;
    }
    if (!menuOwnsKey(e)) return;
    // Nothing behind the menu hears a key pressed in it (`menuOwnsKey`).
    e.stopPropagation();
    const list = items();
    const to = menuStep(e.key, list.indexOf(document.activeElement as HTMLElement), list.length);
    if (to === null) return;
    e.preventDefault();
    list[to].focus();
  }
</script>

<div class="menu focus-container" role="menu" tabindex="-1" aria-label={label} bind:this={el} use:fitMenu={at} {onkeydown}>
  {@render children()}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    /* Never taller than the window, so `fitMenu` can always place all of it on screen. */
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: var(--s-1);
    background: var(--raised);
    color: var(--text);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  /* The items are the caller's markup, so they are reached with :global. */
  .menu :global(button[role='menuitem']) {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--s-5);
    flex: none;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .menu :global(button[role='menuitem']:hover:not(:disabled)) { background: var(--hover); }
  /* The item the arrow keys are on. Drawn inside the item: the menu scrolls, and clips a
     ring that sits outside its box. */
  .menu :global(button[role='menuitem']:focus-visible) { background: var(--hover); outline-offset: -2px; }
  .menu :global(button[role='menuitem']:disabled) { color: var(--text-dim); cursor: default; }
  .menu :global(.danger) { color: var(--danger); }
  /* An item under a heading. Spelled out to out-rank the item's own padding above. */
  .menu :global(button[role='menuitem'].sub) { padding-left: 18px; }
  /* The key that does the same, at the trailing edge. */
  .menu :global(.hint) { flex: none; color: var(--text-dim); font-size: var(--t-2); }
  .menu :global(.sep) { flex: none; height: 1px; margin: var(--s-1) 0; background: var(--line); }
  .menu :global(.heading) {
    flex: none;
    margin-top: var(--s-1);
    padding: 6px 10px 2px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
    border-top: 1px solid var(--line);
  }
  .menu :global(.note) { flex: none; padding: 4px 18px 6px; color: var(--text-dim); font-size: var(--t-2); }
</style>
