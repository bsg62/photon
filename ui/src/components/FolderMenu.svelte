<script lang="ts">
  import { api, type Folder } from '../lib/api';
  import { library } from '../lib/library.svelte';
  import type { Point } from '../lib/menu-place';
  import Menu from './Menu.svelte';

  /** A folder's context menu, wherever the folder is drawn: its row in the sidebar, and its
   *  header in the grid. One list of items, so the two cannot come to offer different things.
   *  Rescan and Reveal act on the watched folder it belongs to; adding and removing watched
   *  folders live in Settings. */
  let {
    at,
    folder,
    onclose,
    onrename,
    restore,
  }: {
    at: Point;
    folder: Folder;
    /** The opener holds the menu's state and drops it here, before the item acts. */
    onclose: () => void;
    /** "Rename in photon…": the name is edited in the sidebar's row, which the sidebar owns. */
    onrename: (folder: Folder) => void;
    /** `Menu`'s `restore`, for the grid. */
    restore?: () => void;
  } = $props();

  /** The folder is read before the menu is closed: closing unmounts this, and the opener's
   *  state the prop is read from is gone with it. */
  function run(action: (f: Folder) => Promise<unknown> | void) {
    const f = folder;
    onclose();
    Promise.resolve(action(f)).catch(library.reportError);
  }
</script>

<Menu {at} {restore}>
  <!-- `rescan_folder` is a no-op while a scan of that folder is running, and reports
       nothing back, so don't offer it. -->
  <button
    role="menuitem"
    disabled={library.isScanning(folder.watchedId)}
    title={library.isScanning(folder.watchedId) ? 'This folder is being scanned' : undefined}
    onclick={() => run((f) => api.rescanFolder(f.watchedId))}>Rescan</button
  >
  <button role="menuitem" onclick={() => run((f) => api.revealFolder(f.id))}>Reveal in file manager</button>
  <!-- "in photon": the directory keeps its name; only photon's label for it changes. -->
  <button role="menuitem" onclick={() => run(onrename)}>Rename in photon…</button>
  {#if folder.alias !== null}
    <button role="menuitem" title={`Show it as “${folder.name}” again`} onclick={() => run((f) => library.setFolderAlias(f.id, null))}
      >Use folder name</button
    >
  {/if}
  <!-- Hide folder: its photos, and any added to it later, until Unhide folder. Its row then
       leaves the sidebar on its own - the rows are the view's folders, and a hidden folder's
       photos are in no view but Hidden, unless the user unhid one by hand, which keeps the
       folder listed (and this menu offering Unhide folder) wherever that photo shows. -->
  <button role="menuitem" onclick={() => run((f) => library.setFolderHidden(f.id, !f.hidden))}
    >{folder.hidden ? 'Unhide folder' : 'Hide folder'}</button
  >
</Menu>
