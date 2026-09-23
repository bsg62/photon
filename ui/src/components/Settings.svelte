<script lang="ts">
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { onMount, tick } from 'svelte';
  import { api, errorMessage, type AppInfo, type TagCount, type TagRule, type ThemeChoice, type WatchedFolder } from '../lib/api';
  import { theme } from '../lib/app-theme.svelte';
  import { library } from '../lib/library.svelte';
  import { folderStatus, photoCountLabel, type SettingsSection } from '../lib/settings';
  import { createTagRenamer } from '../lib/tag-renamer.svelte';
  import { filterTags, ruleLabel } from '../lib/tags';
  import Icon from './Icon.svelte';
  import SizeControl from './SizeControl.svelte';

  let { section = 'folders', onclose }: { section?: SettingsSection; onclose: () => void } = $props();

  const THEMES: { value: ThemeChoice; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  /** The Hamming distance the backend stores directly (see the design doc): there is no
   *  name-to-distance table to keep in step, and a value outside these three is clamped by
   *  the backend the way an unknown theme falls back. */
  const SIMILAR_DISTANCES: { value: number; label: string; hint: string }[] = [
    { value: 0, label: 'Off', hint: 'Only byte-identical files count as duplicates.' },
    {
      value: 7,
      label: 'Conservative',
      hint: 'Finds every close copy: resized, re-saved, sent through a chat app. The default.',
    },
    { value: 10, label: 'Loose', hint: 'Also looks for more heavily altered copies, and finds most of those.' },
  ];

  // Seeded from the prop once: the dialog is mounted fresh each time it opens, and the
  // section list is the user's to drive after that.
  // svelte-ignore state_referenced_locally
  let current = $state<SettingsSection>(section);
  let dialog = $state<HTMLDivElement | undefined>();
  let counts = $state<Map<number, number>>(new Map());
  let info = $state<AppInfo | null>(null);
  let rules = $state<TagRule[]>([]);
  let tagFilter = $state('');
  const renamer = createTagRenamer({
    rename: (from, to) => library.renameTag(from, to),
    confirm: (message) => ask(message, { title: 'Merge tags', kind: 'warning' }),
    errorMessage,
  });
  let renameInput = $state<HTMLInputElement | undefined>();
  const shownTags = $derived(filterTags(library.tags, tagFilter));

  $effect(() => {
    dialog?.focus();
  });

  // Re-read on every grid version: a scan that adds or removes photos while the dialog is
  // open bumps it, and the counts would otherwise stay at what they were on open.
  // `stale` drops a response that a later request has already overtaken.
  $effect(() => {
    void library.info.version;
    let stale = false;
    api
      .watchedFolderStats()
      .then((stats) => {
        if (!stale) counts = new Map(stats.map((s) => [s.watchedId, s.photoCount]));
      })
      .catch(library.reportError);
    return () => {
      stale = true;
    };
  });

  // Every rule change rebuilds the grid, so keying on the version refetches after our own
  // changes as well as any made elsewhere. `stale` as for the counts above.
  $effect(() => {
    void library.info.version;
    let stale = false;
    api
      .listTagRules()
      .then((r) => {
        if (!stale) rules = r;
      })
      .catch(library.reportError);
    return () => {
      stale = true;
    };
  });

  onMount(() => {
    api
      .appInfo()
      .then((i) => (info = i))
      .catch(library.reportError);
  });

  /** Seconds per photo. Null until read, so the field never shows a value that is not the
   *  stored one. */
  let interval = $state<number | null>(null);

  onMount(() => {
    api
      .slideshowInterval()
      .then((s) => (interval = s))
      .catch(library.reportError);
  });

  /** The Hamming distance Off/Conservative/Loose means. Null until read, for the same reason
   *  `interval` is: the control must not show a value that is not the stored one. */
  let similarDistance = $state<number | null>(null);

  onMount(() => {
    api
      .similarDistance()
      .then((d) => (similarDistance = d))
      .catch(library.reportError);
  });

  function setSimilarDistance(distance: number) {
    api
      .setSimilarDistance(distance)
      .then((stored) => (similarDistance = stored))
      .catch(library.reportError);
  }

  /** On `change`, not `input`: the backend clamps, and clamping "1" on the way to typing
   *  "15" would fight the user. What comes back is what was stored, so an out-of-range
   *  entry snaps to the limit in the field too. */
  function saveInterval(e: Event & { currentTarget: HTMLInputElement }) {
    const seconds = Math.round(Number(e.currentTarget.value));
    const field = e.currentTarget;
    if (!Number.isFinite(seconds)) {
      field.value = String(interval ?? '');
      return;
    }
    api
      .setSlideshowInterval(seconds)
      .then((stored) => {
        interval = stored;
        field.value = String(stored);
      })
      .catch(library.reportError);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }

  function lastSegment(path: string): string {
    const parts = path.split(/[/\\]/).filter(Boolean);
    return parts[parts.length - 1] ?? path;
  }

  async function addFolder() {
    const path = await open({ directory: true, multiple: false, title: 'Add a folder to photon' });
    if (typeof path !== 'string') return;
    try {
      await api.addFolder(path);
      await library.refreshFolders();
    } catch (e) {
      library.reportError(e);
    }
  }

  async function remove(watched: WatchedFolder) {
    try {
      const confirmed = await ask(`Remove “${watched.path}” from photon? Your files stay where they are.`, {
        title: 'Remove folder',
        kind: 'warning',
      });
      if (!confirmed) return;
      await api.removeFolder(watched.id).catch(library.reportError);
      await library.refreshFolders();
    } catch (e) {
      library.reportError(e);
    }
  }

  /** The field appears a tick after it opens; focusing and selecting it then lets a small
   *  correction be a few keystrokes. */
  async function startRename(tag: TagCount) {
    renamer.start(tag.tag);
    await tick();
    renameInput?.focus();
    renameInput?.select();
  }

  /** Focus follows the outcome: back to the dialog once the field has gone, so Escape
   *  still closes Settings, or back into the field when it stayed open (a declined merge,
   *  a refused name, a failed save) so the user can go on typing. `tick` first, because
   *  the field is disabled while the commit runs and cannot take focus until that clears. */
  async function commitRename() {
    await renamer.commit(library.tags, rules);
    await tick();
    if (renamer.editing === null) dialog?.focus();
    else renameInput?.focus();
  }

  function onRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commitRename();
    } else if (e.key === 'Escape') {
      // The dialog closes on Escape too; this one only closes the field.
      e.preventDefault();
      e.stopPropagation();
      renamer.cancel();
      dialog?.focus();
    }
  }

  /** Escape in a non-empty filter clears it, as a search field does, without also closing
   *  the dialog. */
  function onFilterKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && tagFilter !== '') {
      e.preventDefault();
      e.stopPropagation();
      tagFilter = '';
    }
  }

  async function removeTag(tag: TagCount) {
    try {
      const confirmed = await ask(
        `Remove “${tag.tag}” from photon? The keyword stays in your photo files, and you can restore it under Changes.`,
        { title: 'Remove tag', kind: 'warning' },
      );
      if (!confirmed) return;
      await library.hideTag(tag.tag);
    } catch (e) {
      library.reportError(e);
    }
  }

  async function restore(rule: TagRule) {
    try {
      await library.restoreTagRule(rule.tag);
    } catch (e) {
      library.reportError(e);
    }
  }
</script>

<!-- The backdrop is a mouse convenience; Escape and the close button are the accessible ways
     out, so it needs no role or key handler of its own. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog focus-container"
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-title"
    tabindex="-1"
    bind:this={dialog}
    {onkeydown}
  >
    <header>
      <h1 id="settings-title">Settings</h1>
      <button class="close" aria-label="Close settings" onclick={onclose}><Icon name="x" size={16} /></button>
    </header>

    <div class="body">
      <nav aria-label="Settings sections">
        <button class:active={current === 'folders'} aria-current={current === 'folders'} onclick={() => (current = 'folders')}>
          Folders
        </button>
        <button class:active={current === 'appearance'} aria-current={current === 'appearance'} onclick={() => (current = 'appearance')}>
          Appearance
        </button>
        <button class:active={current === 'tags'} aria-current={current === 'tags'} onclick={() => (current = 'tags')}>
          Tags
        </button>
        <button class:active={current === 'slideshow'} aria-current={current === 'slideshow'} onclick={() => (current = 'slideshow')}>
          Slideshow
        </button>
        <button class:active={current === 'duplicates'} aria-current={current === 'duplicates'} onclick={() => (current = 'duplicates')}>
          Duplicates
        </button>
        <button class:active={current === 'about'} aria-current={current === 'about'} onclick={() => (current = 'about')}>
          About
        </button>
      </nav>

      <section>
        {#if current === 'folders'}
          <h2>Folders</h2>
          <p class="hint">photon watches these folders for photos. Removing one never touches its files.</p>
          <!-- Every watched root is listed here whether or not it holds photos. An offline
               root, or one never scanned, has no section in the grid and so no row in the
               sidebar; this list is the only place it can still be rescanned or removed. -->
          {#if library.folders.watched.length === 0}
            <p class="empty">No folders yet. Add one to start building your library.</p>
          {:else}
            <ul class="folders">
              {#each library.folders.watched as w (w.id)}
                {@const status = folderStatus(w, library.scans[w.id], !!library.degraded[w.id])}
                {@const scanning = status.kind === 'scanning'}
                <li class:offline={!w.online}>
                  <div class="meta">
                    <span class="name">{lastSegment(w.path)}</span>
                    <span class="path" title={w.path}>{w.path}</span>
                    <span class="details">
                      <span class="status {status.kind}">{status.label}</span>
                      · {photoCountLabel(counts.get(w.id) ?? 0)}
                    </span>
                  </div>
                  <div class="actions">
                    <!-- `rescan_folder` is a no-op while a scan of that folder is running,
                         and reports nothing back, so don't offer it. -->
                    <button
                      disabled={scanning}
                      title={scanning ? 'This folder is being scanned' : undefined}
                      onclick={() => api.rescanFolder(w.id).catch(library.reportError)}>Rescan</button
                    >
                    <button onclick={() => api.revealWatched(w.id).catch(library.reportError)}>Reveal</button>
                    <button class="danger" onclick={() => remove(w)}>Remove…</button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
          <button class="add" onclick={addFolder}>Add folder…</button>
        {:else if current === 'tags'}
          <h2>Tags</h2>
          <p class="hint">Tags are the keywords in your photos. Renaming or removing one changes how photon shows it; your files keep their keywords.</p>
          {#if library.tags.length === 0}
            <p class="empty">No tags. Keywords saved in your photos appear here.</p>
          {:else}
            <input class="filter" type="search" placeholder="Filter tags" aria-label="Filter tags" bind:value={tagFilter} onkeydown={onFilterKeydown} />
            <ul class="tags">
              {#each shownTags as tag (tag.tag)}
                <li>
                  {#if renamer.editing === tag.tag}
                    <div class="meta">
                      <input
                        class="rename"
                        bind:this={renameInput}
                        bind:value={renamer.text}
                        disabled={renamer.busy}
                        aria-label="New name for {tag.tag}"
                        aria-invalid={renamer.error !== ''}
                        onkeydown={onRenameKeydown}
                        onblur={() => renamer.cancel()}
                      />
                      {#if renamer.error}<span class="error">{renamer.error}</span>{/if}
                    </div>
                  {:else}
                    <div class="meta">
                      <span class="name">{tag.tag}</span>
                      <span class="details">{photoCountLabel(tag.total)}</span>
                    </div>
                    <div class="actions">
                      <button onclick={() => startRename(tag)}>Rename</button>
                      <button class="danger" onclick={() => removeTag(tag)}>Remove…</button>
                    </div>
                  {/if}
                </li>
              {:else}
                <li class="empty">No tag matches “{tagFilter}”.</li>
              {/each}
            </ul>
          {/if}
          {#if rules.length > 0}
            <h2>Changes</h2>
            <ul class="tags">
              {#each rules as rule (rule.tag)}
                <li>
                  <span class="meta name">{ruleLabel(rule)}</span>
                  <div class="actions">
                    <button onclick={() => restore(rule)}>Restore</button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        {:else if current === 'appearance'}
          <h2>Appearance</h2>
          <p class="hint">System follows your desktop. The photo viewer is always dark, so every photo is seen against the same ground.</p>
          <!-- A group of independent toggle buttons, each its own Tab stop with Enter/Space
               to activate — not the APG radiogroup pattern (one roving tab stop, arrows to
               move and select). role="radio" without that key handling lies to a screen
               reader, so this is role="group" with aria-pressed, not role="radiogroup". -->
          <div class="segmented" role="group" aria-label="Theme">
            {#each THEMES as option (option.value)}
              <button
                aria-pressed={theme.choice === option.value}
                class:checked={theme.choice === option.value}
                onclick={() => theme.set(option.value)}
              >
                {option.label}
              </button>
            {/each}
          </div>
          <h2>Photo size</h2>
          <p class="hint">How large the grid draws each photo.</p>
          <SizeControl />
        {:else if current === 'slideshow'}
          <h2>Slideshow</h2>
          <p class="hint">Press S in the viewer to play the current view from the photo on screen. Space pauses, the arrow keys step, Escape ends it.</p>
          <label class="interval">
            Show each photo for
            <input type="number" min="1" max="60" step="1" value={interval ?? ''} disabled={interval === null} onchange={saveInterval} />
            seconds
          </label>
        {:else if current === 'duplicates'}
          <h2>Find look-alikes</h2>
          <p class="hint">
            The Duplicates view already finds files that are byte-for-byte the same. This widens it to
            photos that are the same picture after a resize or a re-save, compared as shown - an edit
            counts as the one photo it came from, not a look-alike of it.
          </p>
          <div class="segmented" role="group" aria-label="Find look-alikes">
            {#each SIMILAR_DISTANCES as option (option.value)}
              <button
                aria-pressed={similarDistance === option.value}
                class:checked={similarDistance === option.value}
                disabled={similarDistance === null}
                onclick={() => setSimilarDistance(option.value)}
              >
                {option.label}
              </button>
            {/each}
          </div>
          {#if similarDistance !== null}
            {@const chosen = SIMILAR_DISTANCES.find((o) => o.value === similarDistance) ?? SIMILAR_DISTANCES[1]}
            <p class="hint">{chosen.hint}</p>
          {/if}
        {:else}
          <h2>About</h2>
          {#if info}
            <dl>
              <dt>Version</dt>
              <dd>photon {info.version}</dd>
              <dt>Library</dt>
              <dd class="library">
                <span class="path selectable">{info.libraryPath}</span>
                <button onclick={() => api.revealLibrary().catch(library.reportError)}>Reveal</button>
              </dd>
              <dt>Licence</dt>
              <dd>{info.licence}</dd>
            </dl>
          {/if}
        {/if}
      </section>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    /* Definite tracks. Left implicit, the row grows to the dialog's whole content height:
       the dialog's `height: min(520px, 100%)` has nothing to resolve against, so a long
       Tags list centred a 520px box in a row thousands of pixels tall, off the screen. */
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    place-items: center;
    padding: 16px;
    background: var(--scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(720px, 100%);
    height: min(520px, 100%);
    /* Clips the header's and the section list's chrome to the rounded corners. */
    overflow: hidden;
    background: var(--surface);
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
  h2 { margin: 0 0 var(--s-1); font-size: var(--t-3); font-weight: 600; }
  button {
    padding: 5px var(--s-3);
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  button:hover:not(:disabled) { background: var(--field-hover); }
  button:disabled { color: var(--text-dim); opacity: 0.6; cursor: default; }
  .close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; background: none; color: var(--text-dim); }
  .close:hover:not(:disabled) { background: var(--hover); color: var(--text); }
  .add { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  /* Spelled to out-rank the generic hover above, which would otherwise grey it. */
  .add:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  .body { display: flex; flex: 1; min-height: 0; }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 150px;
    padding: var(--s-2) 6px;
    background: var(--chrome);
    border-right: 1px solid var(--line);
  }
  nav button { height: 28px; padding: 0 var(--s-2); background: none; text-align: left; }
  nav button:hover:not(:disabled) { background: var(--hover); }
  nav button.active, nav button.active:hover:not(:disabled) { background: var(--accent-soft); }
  section { flex: 1; min-width: 0; padding: var(--s-3) var(--s-4); overflow: auto; }
  .hint, .empty { margin: 0 0 var(--s-3); color: var(--text-dim); }
  .interval { display: flex; align-items: center; gap: 8px; }
  .interval input { width: 64px; padding: 5px var(--s-2); border: 0; border-radius: var(--r-2); background: var(--field); color: inherit; font: inherit; }
  .segmented { display: inline-flex; gap: 2px; padding: 2px; border-radius: var(--r-3); background: var(--field); }
  .segmented button { padding: 4px 14px; border-radius: var(--r-2); background: none; }
  .segmented button:hover:not(:disabled) { background: var(--hover); }
  /* After the hover rule and spelled as long, so the chosen segment keeps its accent under
     the pointer by specificity rather than by source order alone. */
  .segmented button.checked, .segmented button.checked:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  .folders { margin: 0 0 12px; padding: 0; list-style: none; }
  .folders li, .tags li { border-bottom: 1px solid var(--line); }
  .folders li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
  }
  .meta { display: flex; flex: 1; flex-direction: column; min-width: 0; }
  .name { font-weight: 600; }
  .path { overflow: hidden; color: var(--text-dim); font-size: var(--t-2); text-overflow: ellipsis; white-space: nowrap; }
  .details { color: var(--text-dim); font-size: var(--t-2); }
  .offline .name { opacity: 0.6; }
  .status.scanning { color: var(--accent); }
  .status.degraded, .status.offline { color: var(--text); }
  .actions { display: flex; flex-shrink: 0; gap: 6px; }
  .danger { color: var(--danger); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 16px; margin: 8px 0 0; }
  dt { color: var(--text-dim); }
  dd { margin: 0; min-width: 0; }
  .library { display: flex; align-items: center; gap: 8px; }
  .selectable { user-select: text; }
  .filter, .rename {
    width: 100%;
    padding: 5px var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    color: inherit;
    font: inherit;
  }
  .filter { margin-bottom: 8px; }
  .filter::placeholder { color: var(--text-dim); }
  .tags { margin: 0 0 16px; padding: 0; list-style: none; }
  .tags li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
  }
  .error { color: var(--danger); font-size: var(--t-2); }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
