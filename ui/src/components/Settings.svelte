<script lang="ts">
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { api, type AppInfo, type WatchedFolder } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import { folderStatus, photoCountLabel, type SettingsSection } from '../lib/settings';

  let { section = 'folders', onclose }: { section?: SettingsSection; onclose: () => void } = $props();

  // Seeded from the prop once: the dialog is mounted fresh each time it opens, and the
  // section list is the user's to drive after that.
  // svelte-ignore state_referenced_locally
  let current = $state<SettingsSection>(section);
  let dialog = $state<HTMLDivElement | undefined>();
  let counts = $state<Map<number, number>>(new Map());
  let info = $state<AppInfo | null>(null);

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

  onMount(() => {
    api
      .appInfo()
      .then((i) => (info = i))
      .catch(library.reportError);
  });

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
</script>

<!-- The backdrop is a mouse convenience; Escape and the close button are the accessible ways
     out, so it needs no role or key handler of its own. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-title"
    tabindex="-1"
    bind:this={dialog}
    {onkeydown}
  >
    <header>
      <h1 id="settings-title">Settings</h1>
      <button class="close" aria-label="Close settings" onclick={onclose}>✕</button>
    </header>

    <div class="body">
      <nav aria-label="Settings sections">
        <button class:active={current === 'folders'} aria-current={current === 'folders'} onclick={() => (current = 'folders')}>
          Folders
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
    place-items: center;
    padding: 16px;
    background: #0009;
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(720px, 100%);
    height: min(520px, 100%);
    background: var(--panel);
    border-radius: 8px;
    box-shadow: 0 12px 48px #000a;
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 10px 16px;
    border-bottom: 1px solid #0003;
  }
  h1 { margin: 0; font-size: 15px; font-weight: 600; }
  h2 { margin: 0 0 4px; font-size: 14px; font-weight: 600; }
  button {
    padding: 4px 10px;
    border: 1px solid #fff2;
    border-radius: 4px;
    background: var(--panel-2);
    cursor: pointer;
  }
  button:hover:not(:disabled) { background: #3a3e45; }
  button:disabled { color: var(--muted); cursor: default; }
  .close { border: 0; background: none; }
  .body { display: flex; flex: 1; min-height: 0; }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 150px;
    padding: 8px;
    border-right: 1px solid #0003;
  }
  nav button { border: 0; background: none; text-align: left; }
  nav button.active { background: #ffffff14; }
  section { flex: 1; min-width: 0; padding: 12px 16px; overflow: auto; }
  .hint, .empty { margin: 0 0 12px; color: var(--muted); }
  .folders { margin: 0 0 12px; padding: 0; list-style: none; }
  .folders li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-bottom: 1px solid #ffffff0d;
  }
  .meta { display: flex; flex: 1; flex-direction: column; min-width: 0; }
  .name { font-weight: 600; }
  .path { overflow: hidden; color: var(--muted); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
  .details { color: var(--muted); font-size: 12px; }
  .offline .name { opacity: 0.6; }
  .status.scanning { color: var(--accent); }
  .status.degraded, .status.offline { color: var(--text); }
  .actions { display: flex; flex-shrink: 0; gap: 6px; }
  .danger { color: var(--danger); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 16px; margin: 8px 0 0; }
  dt { color: var(--muted); }
  dd { margin: 0; min-width: 0; }
  .library { display: flex; align-items: center; gap: 8px; }
  .selectable { user-select: text; }
</style>
