import { open } from '@tauri-apps/plugin-dialog';
import { api } from './api';
import { library } from './library.svelte';

/** The system's folder picker, and the folder picked there watched: Settings' "Add folder…"
 *  and the empty library's. A refusal (a folder inside one already watched, say) is a toast;
 *  a picker closed without a choice is nothing. The scan the backend starts shows in the
 *  status bar. */
export async function addFolderFromPicker(): Promise<void> {
  const path = await open({ directory: true, multiple: false, title: 'Add a folder to photon' });
  if (typeof path !== 'string') return;
  try {
    await api.addFolder(path);
    await library.refreshFolders();
  } catch (e) {
    library.reportError(e);
  }
}
