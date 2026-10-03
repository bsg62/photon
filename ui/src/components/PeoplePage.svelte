<script lang="ts">
  import { ask } from '@tauri-apps/plugin-dialog';
  import { tick } from 'svelte';
  import { api } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { fitMenu } from '../lib/menu-place';
  import { createPeoplePage, IGNORED_FACES, SINGLE, stripKey, type StripKey } from '../lib/people-page.svelte';
  import { faceStatus } from '../lib/status';
  import FaceStrip from './FaceStrip.svelte';
  import NameBox from './NameBox.svelte';

  let { onopen, onopensettings }: { onopen: (itemId: number) => void; onopensettings: () => void } = $props();

  const model = createPeoplePage({
    load: (strip) => api.peoplePage(strip),
    more: (person, which, offset, limit) => api.personFaces(person, which, offset, limit),
    name: (group, name) => api.namePerson(group, name),
    rename: (person, name) => api.renamePerson(person, name),
    confirm: (faces) => api.confirmFaces(faces),
    reject: (faces) => api.rejectFaces(faces),
    merge: (from, into) => api.mergePeople(from, into),
    ignoreGroup: (group, ignored) => api.ignorePerson(group, ignored),
    ignoreFaces: (faces, ignored) => api.ignoreFaces(faces, ignored),
    remove: (person) => api.deletePerson(person),
    ask: (message, title) => ask(message, { title, kind: 'warning' }),
    reportError: library.reportError,
  });

  /** Null until read: the page must not say "switched off" before it knows. */
  let enabled = $state<boolean | null>(null);
  let root: HTMLElement | undefined = $state();
  let renaming = $state<number | null>(null);
  let mergeMenu = $state<{ x: number; y: number; from: number } | null>(null);
  let mergeMenuEl = $state<HTMLDivElement | undefined>();
  /** Each person's Rename and Merge buttons, where focus goes back to when the field or
   *  the menu they opened closes: an element removed while it holds focus hands it to
   *  `<body>`, where no key reaches anything until the user clicks. */
  const renameButtons: Record<number, HTMLButtonElement | null> = {};
  const mergeButtons: Record<number, HTMLButtonElement | null> = {};
  const progress = $derived(faceStatus(library.faces));
  const counted = new Intl.NumberFormat();

  const page = $derived(model.page);
  const ignoredFaces = $derived(model.count(IGNORED_FACES));
  /** Through the model, not `page.singleCount`: a face named or ignored leaves at once,
   *  before the reload that confirms it. */
  const singleCount = $derived(model.count(SINGLE));
  const singleShown = $derived(model.faces(SINGLE).length);
  const empty = $derived(
    page !== null &&
      !model.unnamed.length &&
      !singleCount &&
      !model.suggestions.length &&
      !model.people.length &&
      !model.ignoredGroups.length &&
      !ignoredFaces,
  );

  // The page refetches on every library change that carries `data_changed` (spec
  // "Loading"): the face pass's grouping, a scan, and the page's own writes all announce
  // one. `dataVersion` moves exactly then; reading it here is what subscribes.
  $effect(() => {
    void library.dataVersion;
    void model.load().catch(library.reportError);
  });

  /** Whether a face pass is reporting: moves only when one starts or ends, not with each
   *  count, so the switch below is not re-read per progress event. */
  const passing = $derived(library.faces !== null);

  // The switch is read again, not once on mount: Settings opens over this page. Switching
  // Find faces off there empties the page with a data change, and the page must then say
  // why rather than "No faces grouped yet"; switching it on sends no data change, but
  // starts a pass, which has work whenever a photo has a preview to look at (off cleared
  // every photo's detection).
  // Numbered, because the reads overlap: an older answer landing after a newer one would
  // put back the switch's state from before the change.
  let switchRead = 0;
  $effect(() => {
    void library.dataVersion;
    void passing;
    const read = ++switchRead;
    api
      .faceDetection()
      .then((on) => {
        if (read === switchRead) enabled = on;
      })
      .catch(library.reportError);
  });

  $effect(() => {
    if (mergeMenu) mergeMenuEl?.focus();
  });

  export function focus() {
    root?.focus();
  }

  const plural = (n: number, one: string, many: string) => `${counted.format(n)} ${n === 1 ? one : many}`;

  function openMergeMenu(e: MouseEvent, from: number) {
    // The window's click listener closes menus; this click must not reach it and close the
    // menu it is opening.
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    mergeMenu = { x: r.left, y: r.bottom, from };
  }

  /** Closed before the question, which is a native dialog: the menu would otherwise sit
   *  open behind it, and again after it is answered. */
  function mergeInto(into: number) {
    const from = mergeMenu?.from;
    void closeMergeMenu(true);
    if (from !== undefined) void model.merge(from, into).then(rescueFocus).catch(library.reportError);
  }

  /** `restore` is false for a click outside the menu that focused something of its own:
   *  taking focus back from that would undo the user's click. */
  async function closeMergeMenu(restore: boolean) {
    const from = mergeMenu?.from;
    mergeMenu = null;
    if (from === undefined || !restore) return;
    await tick();
    (mergeButtons[from] ?? root)?.focus();
  }

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') void closeMergeMenu(true);
  }

  /** Whether focus has nowhere to be: on `<body>`, or inside the menu about to go. */
  function focusLost(): boolean {
    const at = document.activeElement;
    return !at || at === document.body || !!mergeMenuEl?.contains(at);
  }

  function onWindowClick() {
    if (mergeMenu) void closeMergeMenu(focusLost());
  }

  /** After a person is merged away or deleted, their row - holding the focused button or
   *  the one focus was handed back to - is gone; the page itself takes focus then. */
  async function rescueFocus() {
    await tick();
    if (focusLost()) root?.focus();
  }

  /** Each group's row in Unnamed and Ignored, by strip key: where focus goes on to when the
   *  group before it leaves. */
  const groupEls: Record<StripKey, HTMLElement | null> = {};

  /** Runs an action that removes a group from `list` (named, ignored, or ignored no
   *  longer) and hands focus to the group that takes its place, or to the page: the row
   *  that held the focused button or field is gone, and focus on `<body>` reaches no key.
   *  The model hides the group before its write, so one tick shows the row gone. */
  async function leaving(section: 'unnamed' | 'ignored', id: number, action: () => Promise<void>) {
    const list = () => (section === 'unnamed' ? model.unnamed : model.ignoredGroups);
    const at = list().findIndex((g) => g.id === id);
    const done = action();
    await tick();
    const next = at < 0 ? undefined : list()[at];
    const face = next && groupEls[stripKey(section, next.id)]?.querySelector<HTMLElement>('button.face');
    (face || root)?.focus();
    await done.catch(library.reportError);
  }

  async function closeRename(person: number) {
    renaming = null;
    await tick();
    // A rename into another person's name merges this one away, and their button with them.
    (renameButtons[person] ?? root)?.focus();
  }
</script>

<svelte:window onclick={onWindowClick} />

<div class="people focus-container" tabindex="-1" bind:this={root}>
  <header>
    <h1>People</h1>
    {#if progress}<span class="hint" role="status">{progress.label}</span>{/if}
  </header>

  {#if enabled === false}
    <div class="notice">
      <p>photon finds and groups faces only while Find faces is on.</p>
      <button class="primary" onclick={onopensettings}>Open Settings</button>
    </div>
  {:else if enabled && page}
    {#if empty}
      <div class="notice">
        <p>No faces grouped yet.</p>
        {#if !progress}
          <p class="hint">photon groups faces once it has found them; this page fills in as it goes.</p>
        {/if}
      </div>
    {/if}

    {#if model.unnamed.length || singleCount}
      <section aria-labelledby="people-unnamed">
        <h2 id="people-unnamed">
          Unnamed{#if model.unnamed.length}
            <span class="note">{plural(model.unnamed.length, 'group', 'groups')}, largest first</span>{/if}
        </h2>
        {#each model.unnamed as g (g.id)}
          <div class="row" bind:this={groupEls[stripKey('unnamed', g.id)]}>
            <FaceStrip {model} key={stripKey('unnamed', g.id)} label="Unnamed group" {onopen} onfocuslost={focus} />
            <div class="line">
              {#if g.offer}
                {@const offer = g.offer}
                <button class="primary" onclick={() => leaving('unnamed', g.id, () => model.nameGroup(g.id, offer.name))}>Yes, this is {offer.name}</button>
              {/if}
              <NameBox choose={(t) => model.choice(t)} commit={(t) => leaving('unnamed', g.id, () => model.nameGroup(g.id, t))} label="Name this group" />
              <button onclick={() => leaving('unnamed', g.id, () => model.ignoreGroup(g.id, true))}>Ignore</button>
            </div>
            {#if g.offer}
              <p class="hint">Offered because {g.offer.faces} of these faces are ones Picasa named {g.offer.name}.</p>
            {/if}
          </div>
        {/each}
        {#if singleCount > 0}
          <details class="row">
            <summary>{plural(singleCount, 'single face', 'single faces')}</summary>
            <FaceStrip {model} key={SINGLE} label="Single faces" {onopen} onfocuslost={focus} />
            {#if singleCount > singleShown}
              <p class="hint">
                Showing the first {counted.format(singleShown)}. Name or ignore some to see the rest.
              </p>
            {/if}
          </details>
        {/if}
      </section>
    {/if}

    {#if model.suggestions.length}
      <section aria-labelledby="people-suggestions">
        <h2 id="people-suggestions">Suggestions <span class="note">faces photon thinks are someone you named</span></h2>
        {#each model.suggestions as p (p.id)}
          {@const key = stripKey('suggestion', p.id)}
          <div class="row">
            <div class="line">
              <strong>{p.name}</strong>
              <span class="dim">{counted.format(model.count(key))} to check</span>
            </div>
            <FaceStrip {model} {key} suggestion label="Suggested faces for {p.name}" {onopen} onfocuslost={focus} />
            <div class="line">
              <button class="primary" onclick={() => model.confirmAll(p.id)}>{model.confirmAllLabel(p.id)}</button>
            </div>
          </div>
        {/each}
      </section>
    {/if}

    {#if model.people.length}
      <section aria-labelledby="people-named">
        <h2 id="people-named">People · {counted.format(model.people.length)}</h2>
        {#each model.people as p (p.id)}
          {@const key = stripKey('person', p.id)}
          {@const n = model.count(key)}
          <div class="row">
            <div class="line">
              {#if renaming === p.id}
                <NameBox
                  renaming
                  initial={p.name ?? ''}
                  choose={(t) => model.choice(t, p.id)}
                  commit={async (t) => {
                    await model.rename(p.id, t);
                    await closeRename(p.id);
                  }}
                  oncancel={() => closeRename(p.id)}
                  label="Rename {p.name}"
                />
              {:else}
                <strong>{p.name}</strong>
              {/if}
              <span class="dim">{n ? plural(n, 'face', 'faces') : 'No faces shown'}</span>
              <span class="buttons">
                <button bind:this={renameButtons[p.id]} onclick={() => (renaming = p.id)}>Rename</button>
                <button
                  bind:this={mergeButtons[p.id]}
                  disabled={model.people.length < 2}
                  aria-haspopup="menu"
                  onclick={(e) => openMergeMenu(e, p.id)}>Merge into…</button
                >
                <button class="danger" onclick={() => model.remove(p.id).then(rescueFocus).catch(library.reportError)}>Delete</button>
              </span>
            </div>
            <FaceStrip {model} {key} label="Faces of {p.name}" {onopen} onfocuslost={focus} />
          </div>
        {/each}
      </section>
    {/if}

    {#if model.ignoredGroups.length || ignoredFaces}
      <section aria-label="Ignored">
        <details class="row">
          <summary>
            Ignored · {plural(model.ignoredGroups.length, 'group', 'groups')}, {plural(ignoredFaces, 'face', 'faces')}
          </summary>
          {#each model.ignoredGroups as g (g.id)}
            <div class="ignored" bind:this={groupEls[stripKey('ignored', g.id)]}>
              <FaceStrip {model} key={stripKey('ignored', g.id)} label="Ignored group" {onopen} onfocuslost={focus} />
              <div class="line">
                <button onclick={() => leaving('ignored', g.id, () => model.ignoreGroup(g.id, false))}
                  >Stop ignoring</button
                >
              </div>
            </div>
          {/each}
          <FaceStrip {model} key={IGNORED_FACES} label="Ignored faces" {onopen} onfocuslost={focus} />
        </details>
      </section>
    {/if}
  {/if}
</div>

<!-- Beside the scroll container rather than in it, as the sidebar's menus are beside their
     list: `fitMenu` places it against the window, and inside a container that ever gained a
     transform or containment it would be placed against that instead. -->
{#if mergeMenu}
  {@const from = mergeMenu.from}
  <div
    class="menu focus-container"
    role="menu"
    tabindex="-1"
    aria-label="Merge into"
    bind:this={mergeMenuEl}
    use:fitMenu={mergeMenu}
    onkeydown={onMenuKeydown}
  >
    {#each model.people.filter((p) => p.id !== from) as p (p.id)}
      <button role="menuitem" onclick={() => mergeInto(p.id)}>{p.name}</button>
    {/each}
  </div>
{/if}

<style>
  .people {
    height: 100%;
    overflow: auto;
    padding: var(--s-4);
  }
  header { display: flex; align-items: baseline; gap: var(--s-3); margin-bottom: var(--s-3); }
  h1 { margin: 0; font-size: var(--t-5); font-weight: 600; }
  h2 { margin: var(--s-5) 0 var(--s-2); font-size: var(--t-4); font-weight: 600; }
  .note { margin-left: var(--s-2); color: var(--text-dim); font-size: var(--t-2); font-weight: 400; }
  section { display: flex; flex-direction: column; gap: var(--s-2); }
  .row {
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
    padding: var(--s-3);
    border: 1px solid var(--line);
    border-radius: var(--r-3);
    background: var(--surface);
  }
  details.row { gap: 0; }
  details.row[open] > :global(*:not(summary)) { margin-top: var(--s-2); }
  summary { cursor: pointer; font-weight: 600; }
  .ignored { display: flex; flex-direction: column; gap: var(--s-2); }
  .line { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s-2); }
  .buttons { display: flex; gap: 6px; margin-left: auto; }
  .dim { color: var(--text-dim); }
  .hint { margin: 0; color: var(--text-dim); font-size: var(--t-2); }
  .notice { margin-top: var(--s-5); }
  .notice p { margin: 0 0 var(--s-2); }
  /* Settings.svelte's buttons. */
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
  .primary { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  /* Spelled to out-rank the generic hover above, which would otherwise grey it. */
  .primary:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  .danger { color: var(--danger); }
  /* FolderTree.svelte's menus. */
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button {
    padding: 6px 10px;
    border-radius: var(--r-2);
    background: none;
    text-align: left;
  }
  .menu button:hover:not(:disabled) { background: var(--hover); }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
</style>
