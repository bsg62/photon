import {
  api,
  errorMessage,
  events,
  type AlbumSummary,
  type SavedSearch,
  type Folder,
  type FolderList,
  type GridEntry,
  type GridInfo,
  type GridView,
  type ExportProgress,
  type Person,
  type ScanProgressEvent,
  type TagCount,
} from './api';
import { keepCopiesName } from './copies';
import { lastIndexAtOrBefore } from './layout';
import { PageCache } from './pages';
import type { UnlistenFn } from '@tauri-apps/api/event';

/** `error` is a failure the user should see; `done` is an action reporting what it did.
 *  Both use the same channel because they compete for the same corner of the screen. */
export interface Toast { id: number; message: string; kind: 'error' | 'done' }

/** How many rows one `gridRows` call may ask for: `MAX_ROWS` in `commands.rs`, which
 *  `clamp_count` applies without telling the caller it truncated. */
const GRID_ROWS_CHUNK = 1000;

/** App-wide reactive state: the grid snapshot, the folder tree, scan status and selection. */
export class LibraryStore {
  info = $state<GridInfo>({
    version: -1,
    len: 0,
    sections: [],
    starredCount: 0,
    duplicateCount: 0,
    hiddenCount: 0,
    view: 'all',
    searchQuery: '',
    person: null,
    album: null,
    tag: null,
    copiesOf: null,
  });
  folders = $state<FolderList>({ watched: [], folders: [] });
  /** The sidebar's three collections. Refetched on every `library-changed` (a scan can
   *  add a face, a keyword or purge an album member) and after every album mutation. */
  albums = $state<AlbumSummary[]>([]);
  searches = $state<SavedSearch[]>([]);
  people = $state<Person[]>([]);
  tags = $state<TagCount[]>([]);
  scans = $state<Record<number, ScanProgressEvent>>({});
  /** Watched folder ids the OS won't let photon watch live, from the most recent
   *  `folder-status` event for each: they fall back to periodic rescans instead. */
  degraded = $state<Record<number, boolean>>({});
  /** How many photos each folder held when its current scan started, for the status bar's
   *  progress bar: a scan does not know its total ahead of time, and the previous count is
   *  the best estimate of it. Absent for a folder's first scan. */
  expected = $state<Record<number, number>>({});
  private selectedOffset = $state<number | null>(null);
  /** The photo at `selectedOffset`, when it was selected. See `rebindSelection`. */
  private selectedId: number | null = null;
  /** The photo ids in the selection, as a set replaced wholesale on every change — Svelte's
   *  runes do not track mutation of a plain Set.
   *
   *  Ids, not offsets. An offset only means "this photo" against one version of the index,
   *  and a scan that indexes a photo into an earlier folder shifts every later one: an
   *  offset-based selection would silently ring, and star, the neighbours of what the user
   *  picked. Ids survive every rebuild untouched, which is also why there is no multi-photo
   *  counterpart to `rebindSelection`.
   *
   *  A photo purged by a scan leaves its id behind here. Nothing is drawn wrong — there is
   *  no tile left to ring — but `selectionCount` over-reports until the next plain click.
   *  Pruning it needs a "which of these ids are still live" round trip, which is not worth
   *  an IPC surface for a count one click from correct. Do not "fix" this with offsets. */
  private selection = $state<Set<number>>(new Set());
  /** The grid offset a Shift+click extends from: the last plain click or Ctrl+click. Plain,
   *  not `$state` — nothing renders from it. */
  private anchor: number | null = null;
  /** Bumped on every range fetch — a Shift+click or a Ctrl+A. Two fast Shift+clicks issue overlapping calls
   *  with no ordering guarantee on their fetches - a wide range started first can still be
   *  fetching its later chunks when a narrow range started second finishes first. The rule is
   *  last *call* wins, not last chunk to resolve, so a call abandons its write once a later
   *  call has started; there is no backend ordering to preserve the way `searchQueryChain`
   *  preserves one, so a promise chain would only make the second call wait on the first
   *  instead of pre-empting it. */
  private extendCall = 0;
  /** What a rubber band adds to, captured when it starts; empty for a band that replaces. */
  private bandBase = new Set<number>();
  /** The selection a band started from. Not the same as `bandBase`, which is empty for a
   *  band that replaces: an abandoned drag puts back what was selected either way.
   *
   *  There is deliberately no saved *lead* beside it. A band never moves the lead while it
   *  is being drawn - only a successful `endBand` does - so there is nothing to put back,
   *  and saving one would be worse than useless: a grid rebuilt under the drag re-finds the
   *  lead by id (`rebindSelection`), and writing a pre-rebuild offset back over that answer
   *  is the exact trap that makes Enter open the neighbouring photo. */
  private bandPrevious = new Set<number>();

  /** Selected grid offset. */
  get selected(): number | null {
    return this.selectedOffset;
  }

  set selected(offset: number | null) {
    this.selectedOffset = offset;
    // Which photo that offset meant, remembered now while the page holding it is loaded -
    // by the time the grid is rebuilt the pages are gone.
    const id = offset === null ? null : (this.pages.get(offset)?.id ?? null);
    this.selectedId = id;
    // Every caller of the plain setter is a collapse: an arrow key, a plain click, a
    // right-click outside the selection. Keeping that rule here rather than at each call
    // site is what stops a new caller silently leaving a stale multi-selection behind.
    this.selection = id === null ? new Set() : new Set([id]);
    this.anchor = offset;
  }
  /** Selects `offset` knowing it holds photo `id`, for callers that know the id without the
   *  page being loaded: "Locate in photon" and the viewer closing, both of which arrive by
   *  id and land on an offset the grid has not fetched yet. The plain setter would record
   *  no id for such an offset, and the next rebuild would clamp instead of re-find. */
  selectItem(offset: number, id: number): void {
    // Closing the viewer on the photo it was opened with keeps the selection; navigating
    // away inside the viewer and closing there collapses to the photo on screen.
    const collapse = id !== this.selectedId;
    this.selectedOffset = offset;
    this.selectedId = id;
    if (collapse) this.selection = new Set([id]);
    this.anchor = offset;
  }

  /** Ctrl/Cmd+click: adds or removes one photo. The lead and the anchor move to it either
   *  way, so the next Shift+click extends from where the user last clicked. */
  toggleSelected(offset: number): void {
    const id = this.pages.get(offset)?.id;
    // Reachable: Ctrl+click on a placeholder tile during a fast scroll, before its page has
    // arrived. There is no id to toggle, so this is a deliberate silent no-op, not dead code.
    if (id === undefined) return;
    const next = new Set(this.selection);
    if (!next.delete(id)) next.add(id);
    this.selection = next;
    this.selectedOffset = next.size === 0 ? null : offset;
    this.selectedId = next.size === 0 ? null : id;
    this.anchor = offset;
  }

  /** Shift+click: replaces the selection with the range between the anchor and `offset`.
   *
   *  The ids come from the backend rather than from the loaded pages: a range can span
   *  thousands of photos the grid has never rendered. `MAX_ROWS` in `commands.rs` clamps a
   *  `grid_rows` ask to 1000 *silently*, so a single call for a wider range would select its
   *  first thousand photos and drop the rest without an error anywhere. */
  async extendSelection(offset: number): Promise<void> {
    const from = this.anchor ?? this.selectedOffset ?? 0;
    const start = Math.max(0, Math.min(from, offset));
    const end = Math.min(this.info.len - 1, Math.max(from, offset));
    const ids = await this.fetchIds(start, end);
    if (!ids) return;
    this.selection = ids;
    this.selectedOffset = offset;
    this.selectedId = this.pages.get(offset)?.id ?? null;
    // The anchor stays put, so dragging the far end back and forth re-ranges from the
    // same start rather than walking away from it.
  }

  /** Ctrl/Cmd+A: selects the photos of the folder the lead is in, or — outside the library
   *  view — every photo in the view.
   *
   *  The lead stays where it is, so Enter still opens the photo the user was on. The anchor
   *  moves to the start of what was selected, which is what a Shift+click afterwards extends
   *  from: the selection began there. */
  async selectAll(): Promise<void> {
    const range = this.selectAllRange();
    if (!range) return;
    const [start, end] = range;
    const ids = await this.fetchIds(start, end);
    if (!ids) return;
    this.selection = ids;
    if (this.selectedOffset === null) {
      this.selectedOffset = start;
      this.selectedId = this.pages.get(start)?.id ?? null;
    }
    this.anchor = start;
  }

  /** What Ctrl/Cmd+A covers, as a grid range.
   *
   *  Every view but All is a set the user asked for — an album, a search, the starred
   *  photos — so "all" is that set, and the folder sections inside it are an arrangement of
   *  it rather than a bound. All is the whole library, where the unit the user is actually
   *  looking at is one folder: on a fifty-thousand photo library, selecting every photo is
   *  never what this key was pressed for, and starring the result would be a long operation
   *  nobody asked for. */
  private selectAllRange(): [number, number] | null {
    const len = this.info.len;
    if (len === 0) return null;
    const sections = this.info.sections;
    if (this.info.view !== 'all' || sections.length === 0) return [0, len - 1];
    const at = this.selectedOffset ?? 0;
    const section = sections[lastIndexAtOrBefore(sections, at, (s) => s.offset)];
    return [section.offset, Math.min(len - 1, section.offset + section.count - 1)];
  }

  /** The ids of every photo in `start..end`, or `null` when this fetch has been overtaken
   *  and must not write anything.
   *
   *  The ids come from the backend rather than from the loaded pages: a range can span
   *  thousands of photos the grid has never rendered. `MAX_ROWS` in `commands.rs` clamps a
   *  `grid_rows` ask to 1000 *silently*, so a single call for a wider range would take its
   *  first thousand photos and drop the rest without an error anywhere. */
  private async fetchIds(start: number, end: number): Promise<Set<number> | null> {
    return this.fetchIdsOf([[start, end]]);
  }

  /** `fetchIds` over several ranges at once - what a rubber band covers. One call id and one
   *  version for the whole set, so a band is written wholly or not at all: two ranges of it
   *  resolved against different indexes would name photos from two different grids. */
  private async fetchIdsOf(ranges: [number, number][]): Promise<Set<number> | null> {
    if (ranges.length === 0 || ranges.some(([start, end]) => end < start)) return null;
    const version = this.info.version;
    const call = ++this.extendCall;
    const ids = new Set<number>();
    for (const [start, end] of ranges) {
      for (let at = start; at <= end; at += GRID_ROWS_CHUNK) {
        const count = Math.min(GRID_ROWS_CHUNK, end - at + 1);
        const rows = await api.gridRows(at, count);
        // A refresh has landed while this was in flight; its own selection is the current one.
        if (version !== this.info.version) return null;
        // A later range call has started; that call's write wins, not whichever call's fetch
        // happens to finish last.
        if (call !== this.extendCall) return null;
        // this.info.version can lag a rebuild the backend has already published: the
        // library-changed listener that would bump it has not run yet, so the guard above can
        // pass while these rows were fetched against a newer index than the offsets they were
        // asked for. PageCache.ensure discards a page on the same mismatch; a range built from
        // it would otherwise name photos for offsets the user never saw.
        if (rows.version !== version) return null;
        for (const entry of rows.rows) ids.add(entry.id);
      }
    }
    return ids;
  }

  /** Starts a rubber band. The selection to build on is captured now: every preview is
   *  `base` plus what the band covers, so dragging the rectangle smaller takes photos back
   *  off instead of piling each frame's worth on the last. */
  beginBand(additive: boolean): void {
    this.bandPrevious = new Set(this.selection);
    this.bandBase = additive ? new Set(this.selection) : new Set();
  }

  /** Previews the band, resolving offsets through the loaded pages: synchronous, so the
   *  tiles ring as the pointer moves, and correct for everything on screen. A tile whose
   *  page has not arrived resolves to nothing and is skipped - the silent rule
   *  `toggleSelected` already follows - and `endBand` is what puts it right. */
  bandTo(ranges: [number, number][]): void {
    const next = new Set(this.bandBase);
    for (const [start, end] of ranges) {
      for (let at = start; at <= end; at++) {
        const id = this.pages.get(at)?.id;
        if (id !== undefined) next.add(id);
      }
    }
    this.selection = next;
  }

  /** Ends a band: the same ranges resolved through the backend, which is the authority.
   *  The lead and the anchor land on the band's first photo, so Enter opens something
   *  inside it and a later Shift+click extends from where the band began. */
  async endBand(ranges: [number, number][]): Promise<void> {
    const base = this.bandBase;
    const previous = this.bandPrevious;
    this.bandBase = new Set();
    this.bandPrevious = new Set();

    // A band that covers nothing is a band, not a failure: dragging over empty space is how
    // a selection is cleared, and how an additive drag that ends up covering nothing leaves
    // what it started with. Answering it like an overtaken fetch would put the selection
    // back that the preview had visibly just taken away.
    if (ranges.length === 0) {
      this.selection = new Set(base);
      if (base.size === 0) {
        this.selectedOffset = null;
        this.selectedId = null;
        this.anchor = null;
      }
      return;
    }

    const ids = await this.fetchIdsOf(ranges);
    if (!ids) {
      // Overtaken, or the grid was rebuilt under the drag. The preview was drawn from
      // offsets that mean something else now, so the band is abandoned and the selection
      // goes back to what it was. The lead is left alone: it was never moved by the drag,
      // and a rebuild has already re-found it by id.
      this.selection = previous;
      return;
    }
    for (const id of base) ids.add(id);
    this.selection = ids;
    const first = ranges[0][0];
    this.selectedOffset = first;
    this.selectedId = this.pages.get(first)?.id ?? null;
    this.anchor = first;
  }

  /** Abandons a band (Escape mid-drag): the selection goes back to what it was. The lead is
   *  left alone for the reason `bandPrevious` gives - the drag never moved it. */
  cancelBand(): void {
    this.selection = new Set(this.bandPrevious);
    this.bandBase = new Set();
    this.bandPrevious = new Set();
  }

  clearSelection(): void {
    this.selection = new Set();
    this.selectedOffset = null;
    this.selectedId = null;
    this.anchor = null;
  }

  isSelected(id: number): boolean {
    return this.selection.has(id);
  }

  /** Whether the tile at `offset` should ring, given the id its page currently holds (or
   *  `undefined` when that page has not loaded). The plain `selected` setter records an id
   *  only when the target offset's page is already cached — `End`, the sidebar's folder
   *  jump and a click during a fast scroll can all select an offset with nothing loaded yet,
   *  and `this.selection` is then empty. Falling back to the offset itself when the
   *  selection is empty is what still rings that tile once its entry arrives: the lead has
   *  to stay visible regardless of caching, because the ring is what tells the user where
   *  the keyboard is. */
  isSelectedTile(offset: number, id: number | undefined): boolean {
    if (id !== undefined) return this.selection.has(id);
    return this.selection.size === 0 && this.selectedOffset === offset;
  }

  get selectionCount(): number {
    return this.selection.size;
  }

  get selectedItemIds(): number[] {
    return [...this.selection];
  }

  /** Bumped whenever pages arrive, so `entry()` readers re-run. */
  pageTick = $state(0);
  /** True until the grid has jumped to the folder the last session left it on — or has
   *  established there is none. The grid does not record a new folder while this is set: a
   *  freshly built grid starts at offset 0, and remembering that would overwrite the stored
   *  folder with the library's first one before anything could read it. */
  restoring = $state(true);
  toasts = $state<Toast[]>([]);
  /** The export running now, or null when none is. An export can take minutes, so the
   *  status bar says where it has got to; the dialog that started it is long closed. */
  exporting = $state<ExportProgress | null>(null);

  private folderById = $derived(new Map(this.folders.folders.map((f) => [f.id, f])));
  private onlineByWatched = $derived(new Map(this.folders.watched.map((w) => [w.id, w.online])));
  private pages = new PageCache<GridEntry>((o, c) => api.gridRows(o, c), () => void this.refresh().catch(this.reportError));
  private unlisten: UnlistenFn[] = [];
  private nextToast = 0;
  private initPromise: Promise<void> | null = null;
  /** Bumped by `dispose()` so an in-flight `init()` can tell it was cancelled. */
  private generation = 0;

  /** Idempotent and safe to call again (root remount, HMR): a second call while
   *  already initialised/initialising is a no-op, and never leaves more than the
   *  three event subscriptions registered here. */
  init(): Promise<void> {
    if (this.initPromise) return this.initPromise;
    const generation = ++this.generation;
    this.initPromise = (async () => {
      const unlisten = await Promise.all([
        events.onLibraryChanged(() => {
          void this.refresh().catch(this.reportError);
          void this.refreshCollections().catch(this.reportError);
        }),
        events.onFolderStatus((e) => {
          this.degraded[e.watchedId] = e.degraded;
          void this.refreshFolders().catch(this.reportError);
        }),
        events.onExportProgress((e) => {
          // `done === total` is the end whatever happened on the way, including an export
          // where every photo failed - so the bar always clears.
          this.exporting = e.done < e.total ? e : null;
        }),
        events.onScanProgress((e) => {
          const previous = this.scans[e.watchedId];
          this.scans[e.watchedId] = e;
          if (!e.done && (!previous || previous.done)) void this.snapshotExpected(e).catch(this.reportError);
          if (e.done) void this.refreshFolders().catch(this.reportError);
        }),
      ]);
      if (generation !== this.generation) {
        // dispose() ran while we were subscribing: undo it instead of leaking.
        for (const u of unlisten) u();
        return;
      }
      this.unlisten = unlisten;
      await Promise.all([this.refresh(), this.refreshFolders(), this.refreshCollections()]);
    })();
    return this.initPromise;
  }

  dispose(): void {
    this.generation++;
    this.initPromise = null;
    for (const u of this.unlisten) u();
    this.unlisten = [];
    this.degraded = {};
  }

  async refresh(): Promise<void> {
    const info = await api.gridInfo();
    if (info.version < this.info.version) return;
    this.pages.reset(info.version);
    this.info = { ...info, copiesOf: keepCopiesName(this.info.copiesOf, info.copiesOf) };
    this.pageTick++;
    await this.rebindSelection();
  }

  /** Puts the selection back on the photo it was on, after the index has been rebuilt.
   *
   *  An offset only means "this photo" against one version of the index: a scan that indexes
   *  a photo into an earlier folder shifts every later offset by one, and the selection would
   *  slide onto the next photo without anything looking wrong. Clamping alone catches only
   *  the offset falling off the end, which is the rarer half of the problem.
   *
   *  Falls back to clamping when there is no id to work from, or when the photo has left this
   *  view entirely - in that case the offset is as good an answer as any, and the next
   *  deliberate selection restores an id to track. */
  private async rebindSelection(): Promise<void> {
    const clamp = () => {
      if (this.selectedOffset !== null && this.selectedOffset >= this.info.len) {
        this.selectedOffset = this.info.len ? this.info.len - 1 : null;
      }
    };
    const id = this.selectedId;
    if (id === null) {
      // No id means no way to tell how far the rebuild moved things, so the anchor - a
      // stale offset with nothing left to confirm it - cannot be trusted either. Leaving it
      // set would let the next Shift+click, with no plain click first, range from a photo
      // that was never clicked. `extendSelection`'s `?? this.selectedOffset` fallback (also
      // null here) already gives the same answer a first-ever Shift+click would.
      this.anchor = null;
      clamp();
      return;
    }
    // Captured before the lead moves, so it can be compared against where the lead
    // *was* rather than where it is about to go.
    const before = this.selectedOffset;
    const version = this.info.version;
    const at = await api.gridOffsetOfItem(id);
    // A newer refresh has landed while this was in flight; its own rebind is the current one.
    if (version !== this.info.version) return;
    if (at === null) {
      // The lead's id no longer resolves to an offset in this view, so there is nothing to
      // re-find the anchor by either - the same reasoning as the id === null branch above,
      // reached one step later. The id leaves the selection too: its photo has left the view
      // (hidden from the viewer, unstarred in Starred), and a selection still holding it
      // would hand the next action - Hide, Export, Compare - a photo no tile shows.
      if (this.selection.has(id)) {
        const rest = new Set(this.selection);
        rest.delete(id);
        this.selection = rest;
      }
      this.selectedId = null;
      this.anchor = null;
      clamp();
      return;
    }
    this.selectedOffset = at;
    // The anchor is a grid offset too, and means nothing once the rebuild has moved rows
    // underneath it: left on its stale value, the next Shift+click would range from the
    // wrong photo without anything looking wrong. It normally agrees with the lead, so it
    // moves with it here. The one time it does not is right after `extendSelection`, which
    // deliberately leaves the anchor at the start of the range while the lead moves to the
    // far end - carrying the lead's rebind onto the anchor there would relocate the start
    // of the *next* range to a photo the user never clicked, so it is left untouched.
    if (this.anchor === before) this.anchor = at;
  }

  /** Records the folder's photo count at the start of a scan, from the first progress
   *  event of it. That event arrives after the first batch has been written, so what this
   *  scan has already added is taken back out: on a brand-new folder the count would
   *  otherwise equal `added`, and the bar would read 100% from the first tick and then run
   *  past it. Nothing to measure against (a first scan) is recorded as absent, which the
   *  bar shows as indeterminate. */
  private async snapshotExpected(first: ScanProgressEvent): Promise<void> {
    const stats = await api.watchedFolderStats();
    const count = (stats.find((s) => s.watchedId === first.watchedId)?.photoCount ?? 0) - first.added;
    // A later event may already have marked this scan done; a stale snapshot is harmless
    // but pointless.
    if (count > 0) this.expected[first.watchedId] = count;
    else delete this.expected[first.watchedId];
  }

  /** Sequence of the most recently *issued* folder-list request; see `refreshFolders`. */
  private folderSeq = 0;

  async refreshFolders(): Promise<void> {
    // Three callers can have one of these in flight at once, and they can answer out of
    // order. Removing a watched folder mid-scan emits a final `done: true` whose listener
    // fires a refresh that may read the database before the deletion commits, while
    // `FolderTree.remove` awaits its own; if the first answers last, the deleted root comes
    // back in the sidebar - with a working context menu - until some unrelated event
    // refreshes it again. Only the newest request may write, the same rule `refresh` applies
    // through the grid version.
    const seq = ++this.folderSeq;
    const folders = await api.listFolders();
    if (seq !== this.folderSeq) return;
    this.folders = folders;
  }

  /** Sequence of the most recently issued collections request; same rule as `folderSeq`. */
  private collectionsSeq = 0;

  /** Refetches albums, saved searches, people and tags together. Only the newest request
   *  may write. */
  async refreshCollections(): Promise<void> {
    const seq = ++this.collectionsSeq;
    const [albums, searches, people, tags] = await Promise.all([
      api.listAlbums(),
      api.listSavedSearches(),
      api.listPeople(),
      api.listTags(),
    ]);
    if (seq !== this.collectionsSeq) return;
    this.albums = albums;
    this.searches = searches;
    this.people = people;
    this.tags = tags;
  }

  /** Switches which photos the grid shows. The backend rebuilds its index, so the grid is
   *  reloaded from scratch rather than patched. */
  async setView(view: GridView): Promise<void> {
    await this.switchView(() => api.setGridView(view));
  }

  /** Shows the photos of one Picasa contact. */
  async setPersonView(hash: string): Promise<void> {
    await this.switchView(() => api.setPersonView(hash));
  }

  /** Shows one album. */
  async setAlbumView(albumId: number): Promise<void> {
    await this.switchView(() => api.setAlbumView(albumId));
  }

  /** Shows the photos carrying one keyword. */
  async setTagView(tag: string): Promise<void> {
    await this.switchView(() => api.setTagView(tag));
  }

  /** Shows one photo and its copies. */
  async setCopiesView(itemId: number): Promise<void> {
    await this.switchView(() => api.setCopiesView(itemId));
  }

  /** One shape for every view switch: the command, then a refresh, with failures reported
   *  rather than thrown, since every caller is a click handler. */
  private async switchView(command: () => Promise<void>): Promise<void> {
    try {
      await command();
      this.clearSelection();
      await this.refresh();
    } catch (e) {
      this.reportError(e);
    }
  }

  albumName(albumId: number | null): string {
    return this.albums.find((a) => a.id === albumId)?.name ?? '';
  }

  personName(hash: string | null): string {
    return this.people.find((p) => p.hash === hash)?.name ?? '';
  }

  /** Album mutations. Each refetches the collections itself: the backend only announces a
   *  grid change, and only when the album on screen is the one that changed. Errors are
   *  thrown to the caller, which decides whether a toast or an open field is the answer. */
  async createAlbum(name: string): Promise<number> {
    const album = await api.createAlbum(name);
    await this.refreshCollections();
    return album.id;
  }

  async renameAlbum(albumId: number, name: string): Promise<void> {
    await api.renameAlbum(albumId, name);
    await this.refreshCollections();
  }

  async deleteAlbum(albumId: number): Promise<void> {
    await api.deleteAlbum(albumId);
    await this.refreshCollections();
  }

  /** Saved-search mutations. Like the album ones they refetch the collections themselves:
   *  none of them changes the grid, so no `library_changed` is coming to do it. */
  async saveSearch(name: string, query: string): Promise<void> {
    await api.saveSearch(name, query);
    await this.refreshCollections();
  }

  async renameSavedSearch(searchId: number, name: string): Promise<void> {
    await api.renameSavedSearch(searchId, name);
    await this.refreshCollections();
  }

  async deleteSavedSearch(searchId: number): Promise<void> {
    await api.deleteSavedSearch(searchId);
    await this.refreshCollections();
  }

  async addToAlbum(albumId: number, itemIds: number[]): Promise<void> {
    await api.addToAlbum(albumId, itemIds);
    await this.refreshCollections();
  }

  async removeFromAlbum(albumId: number, itemIds: number[]): Promise<void> {
    await api.removeFromAlbum(albumId, itemIds);
    await this.refreshCollections();
  }

  /** Hides or unhides photos; the backend's rebuild announces the change. Every photo acted
   *  on leaves the view, so once the write lands the selection moves to the nearest photo
   *  that stays - the next one after the lead, or the one before when nothing follows - as a
   *  file manager does after a delete: working through Duplicates, the arrow keys carry on
   *  from where the user was rather than from the top of the library. Nothing acted on stays
   *  selected: `rebindSelection` re-finds only the lead, and a selection still holding them
   *  would offer the next action - "Hide 12 photos" - to photos the user can no longer see.
   *  A failed write keeps the selection, so the user can try again.
   *
   *  The photo to move to is chosen *before* the write, from the index the user was looking
   *  at: the backend announces its rebuild before the command returns, so afterwards the
   *  loaded pages may already be the new index, where "the next offset" is one photo further
   *  on. Its new offset is then asked for by id, for the same reason. */
  async setHidden(itemIds: number[], hidden: boolean): Promise<void> {
    const lead = this.selectedOffset;
    const leaving = new Set(itemIds);
    // Walks outward only through loaded pages: past one that is not loaded there is no id to
    // select, and the selection is simply cleared.
    const nearest = (from: number, step: 1 | -1): GridEntry | undefined => {
      for (let at = from + step; at >= 0 && at < this.info.len; at += step) {
        const entry = this.pages.get(at);
        if (!entry) return undefined;
        if (!leaving.has(entry.id)) return entry;
      }
      return undefined;
    };
    const next = lead === null ? undefined : (nearest(lead, 1) ?? nearest(lead, -1));
    await api.setItemsHidden(itemIds, hidden);
    this.clearSelection();
    if (next && lead !== null) {
      this.selectItem(lead, next.id);
      await this.rebindSelection();
    }
  }


  /** Tag rule changes. The backend's rebuild announces a library change, which refetches
   *  the collections too; refetching here as well means the caller's list is current when
   *  its await returns, not a round trip later. The change's own error is thrown to the
   *  caller; the refetch's is only reported, because the change is saved by then and the
   *  rename field would otherwise stay open on a tag that no longer exists. */
  async renameTag(from: string, to: string): Promise<void> {
    await api.renameTag(from, to);
    await this.refreshCollections().catch(this.reportError);
  }

  async hideTag(tag: string): Promise<void> {
    await api.hideTag(tag);
    await this.refreshCollections().catch(this.reportError);
  }

  async restoreTagRule(tag: string): Promise<void> {
    await api.restoreTagRule(tag);
    await this.refreshCollections().catch(this.reportError);
  }

  /** Chains each `setSearchQuery` call onto the previous one, so two `setSearchQuery` calls
   *  issued close together (for example a debounced search followed by Escape clearing it)
   *  apply to the backend in the order they were issued rather than in whatever order their
   *  IPC round trips happen to finish. This says nothing about `setView`, which is not on
   *  this chain. `cancel()` on the debouncer only stops a call that hasn't fired yet — a
   *  call already dispatched cannot be cancelled, so ordering has to be guaranteed here
   *  instead. */
  private searchQueryChain: Promise<void> = Promise.resolve();

  /** Searches for `query`. A blank query returns the backend to the All view. */
  setSearchQuery(query: string): Promise<void> {
    const next = this.searchQueryChain.then(async () => {
      try {
        await api.setSearchQuery(query);
        this.clearSelection();
        await this.refresh();
      } catch (e) {
        this.reportError(e);
      }
    });
    // Stored separately from what's returned: `next` never rejects today because the
    // try/catch above routes every failure into reportError, but the chain must not depend
    // on that staying true. If some future failure ever did throw past this point, a bare
    // `this.searchQueryChain = next` would leave the chain permanently rejected and every
    // later setSearchQuery call would reject with it — search would go silently dead for
    // the rest of the session. `.catch(() => {})` keeps the chain alive regardless; callers
    // still see `next`, so a real failure is still reported and still rejects for them.
    this.searchQueryChain = next.catch(() => {});
    return next;
  }

  async ensure(start: number, end: number): Promise<void> {
    if (await this.pages.ensure(start, Math.min(end, this.info.len))) this.pageTick++;
  }

  entry(offset: number): GridEntry | undefined {
    void this.pageTick;
    return this.pages.get(offset);
  }

  folderOf(folderId: number): Folder | undefined {
    return this.folderById.get(folderId);
  }

  isOnline(folderId: number): boolean {
    const folder = this.folderById.get(folderId);
    return folder ? (this.onlineByWatched.get(folder.watchedId) ?? true) : true;
  }

  isScanning(watchedId: number): boolean {
    const scan = this.scans[watchedId];
    return !!scan && !scan.done;
  }

  /** True while any *currently watched* folder is relying on periodic rescans instead of
   *  live filesystem events, so the status bar can say live updates are limited.
   *
   *  Filtered against `folders.watched` rather than read straight off `degraded`: removing
   *  a folder emits no `folder-status` event (only a grid refresh), so a stale `true` entry
   *  for its id would otherwise survive the removal and the notice would never clear. */
  get anyDegraded(): boolean {
    return this.folders.watched.some((w) => this.degraded[w.id]);
  }

  reportError = (e: unknown): void => {
    this.toast(errorMessage(e), 'error');
  };

  /** Says what an action did. Some actions - a keyword written to a selection - change
   *  nothing the user can see from where they are standing, and silence there is
   *  indistinguishable from a click that missed. */
  notify = (message: string): void => {
    this.toast(message, 'done');
  };

  private toast(message: string, kind: Toast['kind']): void {
    const id = this.nextToast++;
    this.toasts.push({ id, message, kind });
    // A report of something that worked is read at a glance or not at all; an error is
    // read, so it stays longer.
    setTimeout(() => this.dismissToast(id), kind === 'error' ? 6000 : 4000);
  }

  dismissToast(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }
}

export const library = new LibraryStore();
