/** The logic of photon's own dropdown (`Select.svelte`): which option is active, when the list
 *  is open, and what each key does - the WAI-ARIA "select-only combobox" pattern. A factory
 *  rather than component code so it can be tested; what is left in the component is wiring.
 *
 *  Why not a native `<select>`: the webviews draw its open list themselves (a GTK popup, a
 *  Windows list, a macOS menu), so it could never match the rest of the UI, and the closed
 *  control needed `appearance: none` just to follow the theme. */

/** How long a pause ends a type-ahead word: the APG's suggested half second. */
export const TYPEAHEAD_RESET_MS = 500;

/** How far PageUp and PageDown move. */
const PAGE = 10;

export interface SelectKey {
  key: string;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
}

export function createSelect(opts: {
  count: () => number;
  /** Index of the option the control currently holds, or -1. */
  selected: () => number;
  label: (index: number) => string;
  /** Called only for a different option: choosing the one already held is not a change. */
  choose: (index: number) => void;
  /** While it answers true the list cannot be opened: no click, no key. */
  disabled?: () => boolean;
}) {
  let open = $state(false);
  let active = $state(0);
  let typed = '';
  let typedTimer: ReturnType<typeof setTimeout> | undefined;

  const clamp = (i: number) => Math.max(0, Math.min(opts.count() - 1, i));

  function clearTyping() {
    typed = '';
    clearTimeout(typedTimer);
    typedTimer = undefined;
  }

  const off = () => opts.disabled?.() ?? false;

  function show(at = opts.selected()) {
    if (off()) return;
    active = clamp(at < 0 ? 0 : at);
    open = true;
  }

  function close() {
    open = false;
    clearTyping();
  }

  function commit(index = active) {
    close();
    if (index !== opts.selected()) opts.choose(index);
  }

  /** Moves to the next option whose label starts with what has been typed. A word being
   *  typed is matched from the active option itself, so "de" stays on "Date..." while it
   *  grows; a single letter repeated steps past it, cycling through the options that start
   *  with that letter, which is how a native select answers "d", "d", "d". */
  function typeahead(ch: string) {
    typed += ch.toLowerCase();
    clearTimeout(typedTimer);
    typedTimer = setTimeout(clearTyping, TYPEAHEAD_RESET_MS);
    const n = opts.count();
    const cycling = typed.length > 1 && [...typed].every((c) => c === typed[0]);
    const word = cycling ? typed[0] : typed;
    const from = word.length === 1 ? active + 1 : active;
    for (let k = 0; k < n; k++) {
      const i = (from + k) % n;
      if (opts.label(i).toLowerCase().startsWith(word)) {
        active = i;
        return;
      }
    }
  }

  const printable = (e: SelectKey) => e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey;

  /** Applies a key. Returns whether the control used it, which is the caller's cue to
   *  prevent its default and stop it there - so the viewer's Enter (apply a crop) and
   *  Escape (cancel it) never see a key the list answered. Tab commits but reports false: it
   *  still has to move focus on. */
  function key(e: SelectKey): boolean {
    const n = opts.count();
    if (n === 0 || off()) return false;
    if (!open) {
      switch (e.key) {
        case 'ArrowDown':
        case 'ArrowUp':
        case 'Enter':
        case ' ':
          show();
          return true;
        case 'Home':
          show(0);
          return true;
        case 'End':
          show(n - 1);
          return true;
      }
      if (printable(e)) {
        show();
        typeahead(e.key);
        return true;
      }
      return false;
    }
    switch (e.key) {
      case 'ArrowDown':
        active = clamp(active + 1);
        return true;
      case 'ArrowUp':
        if (e.altKey) commit();
        else active = clamp(active - 1);
        return true;
      case 'Home':
        active = 0;
        return true;
      case 'End':
        active = n - 1;
        return true;
      case 'PageDown':
        active = clamp(active + PAGE);
        return true;
      case 'PageUp':
        active = clamp(active - PAGE);
        return true;
      case 'Escape':
        close();
        return true;
      case 'Enter':
        commit();
        return true;
      case 'Tab':
        commit();
        return false;
      case ' ':
        // Mid-word a space is part of the word ("Date m..."), not a choice.
        if (typed) typeahead(' ');
        else commit();
        return true;
    }
    if (printable(e)) {
      typeahead(e.key);
      return true;
    }
    return false;
  }

  return {
    get open() {
      return open;
    },
    get active() {
      return active;
    },
    show,
    close,
    commit,
    key,
    toggle() {
      if (open) close();
      else show();
    },
    /** The pointer over an option makes it the active one, as a native list does. */
    hover(index: number) {
      active = clamp(index);
    },
  };
}
