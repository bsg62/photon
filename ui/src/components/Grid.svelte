<script lang="ts">
  import { api } from '../lib/api';
  import { canCompare } from '../lib/compare.svelte';
  import { copiesNotice, showCopiesLabel } from '../lib/copies';
  import { isOwnAlbum, ownAlbums } from '../lib/albums';
  import { isCopyPhotoShortcut } from '../lib/copy-photo';
  import { library } from '../lib/library.svelte';
  import { gridSize } from '../lib/app-grid-size.svelte';
  import { buildRows, columnsFor, edgeScrollSpeed, firstVisibleOffset, GAP, itemSpan, itemsInRect, layoutSections, type Rect, rowOfItem, topFolderId, totalHeight, visibleRange } from '../lib/layout';
  import { move, type NavKey } from '../lib/nav';
  import { yearMarks } from '../lib/timeline';
  import Tile from './Tile.svelte';
  import Timeline from './Timeline.svelte';

  let {
    onopen,
    onkeywords,
    onexport,
    oncompare,
    onshowcopies,
  }: {
    onopen: (offset: number) => void;
    onkeywords: (mode: 'add' | 'remove') => void;
    onexport: () => void;
    oncompare: (ids: number[]) => void;
    /** "Show duplicates" on one photo. App owns the view switch and the selection after it. */
    onshowcopies: (id: number) => void;
  } = $props();

  const NAV_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'];
  const VISIBLE_DEBOUNCE_MS = 150;
  /** How far the pointer must move before a press becomes a rubber band rather than a
   *  click. Below this a steady hand and a shaky one must mean the same thing. */
  const BAND_THRESHOLD = 4;
  /** How close to the viewport's edge a band has to be dragged before the grid scrolls
   *  under it, and how fast it may scroll, in pixels **per second** - not per frame, or the
   *  same gesture would scroll twice as far on a 120Hz screen as on a 60Hz one. It stays a
   *  fixed number of pixels while the tile does not, because the margin is a property of the
   *  pointer's reach rather than of the grid: 48px is two fifths of the smallest tile (120)
   *  and under a quarter of the largest (224), so at every size a band that stops short of
   *  the edge does not creep. */
  const BAND_EDGE = 48;
  const BAND_SCROLL_MAX = 1400;
  /** The longest frame the scroll will act on. A tab that was in the background, or a slow
   *  first paint, hands back a delta of whole seconds; without a cap that is one enormous
   *  jump through the library. */
  const BAND_FRAME_MAX_MS = 50;

  let viewport: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);
  let scrollTop = $state(0);

  const columns = $derived(columnsFor(Math.max(0, width - 2 * GAP), gridSize.width));
  /** Recent is laid out as one continuous run of tiles with no folder headers; every other
   *  view keeps the index's folder sections. See `layoutSections` for why. Both the layout
   *  and the keyboard navigation read these rather than `library.info.sections`, so arrow
   *  keys move along the rows the eye sees.
   *
   *  A tile's offline dimming follows the photo's own folder for the same reason: one
   *  Recent row holds photos from several folders, so the section's folder answers for at
   *  most the first of them. */
  const sections = $derived(layoutSections(library.info.view, library.info.sections, library.info.len));
  const headers = $derived(library.info.view !== 'recent');
  const rows = $derived(buildRows(sections, columns, headers, gridSize.width));
  const total = $derived(totalHeight(rows));
  /** The year strip. It needs folder headers to mark (so Recent, which has none, never
   *  shows it), more than one year to choose between, and something to scroll. */
  const marks = $derived(yearMarks(sections, rows));
  const scrubbable = $derived(marks.length > 1 && total > height);
  const rendered = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, height * 2);
    return rows.slice(start, end);
  });
  const onScreen = $derived.by(() => {
    const [start, end] = visibleRange(rows, scrollTop, height, 0);
    return itemSpan(rows.slice(start, end));
  });

  $effect(() => {
    const span = itemSpan(rendered);
    if (span) void library.ensure(span[0], span[1]);
  });

  // Jump to the folder the last session ended on, once there is something to jump to.
  //
  // Both guards are load-bearing. `len === 0` waits for the first grid: on a first run the
  // scan is still working when this mounts, and asking an empty index for a folder's offset
  // answers null, which is indistinguishable from "that folder is gone". `width === 0`
  // waits for the first layout pass: row tops are computed from the column count, so a jump
  // measured before the viewport has a width lands somewhere else once it gets one.
  //
  // It runs once. `started` guards the re-entry the `await`s open up, and `restoring` — read
  // by the effect below — stays set until the jump has actually been made.
  let started = false;
  $effect(() => {
    if (started || !library.restoring || library.info.len === 0 || width === 0) return;
    started = true;
    void (async () => {
      try {
        const folderId = await api.lastFolder();
        if (folderId === null) return;
        const offset = await api.gridOffsetOfFolder(folderId);
        // The folder survives in the library but has no section in this view (every photo
        // in it has gone missing, say). Staying at the top beats scrolling nowhere.
        if (offset === null) return;
        scrollToOffset(offset, 'start');
      } catch {
        // A failed restore is not worth a toast: the grid is simply where it already is.
      } finally {
        library.restoring = false;
      }
    })();
  });

  // Remember the folder at the top of the grid, so the next launch can come back to it.
  //
  // Only in the All view: Starred, Recent and Search are excursions, and letting one
  // overwrite this would mean closing photon from Starred lost the place the user was
  // actually browsing. Only on change, too — the folder at the top changes a handful of
  // times a session, while `scrollTop` changes on every frame of a flick, and this is a
  // database write.
  //
  // `library.restoring` gates the first write: until the restore below has run (or found
  // nothing to restore), the top of a freshly built grid is offset 0, and writing that
  // would overwrite the stored folder with the first one in the library before it could
  // ever be read.
  let remembered: number | null = null;
  $effect(() => {
    if (library.info.view !== 'all' || library.restoring) return;
    const folderId = topFolderId(rows, sections, scrollTop);
    if (folderId === null || folderId === remembered) return;
    remembered = folderId;
    api.setLastFolder(folderId).catch(() => {});
  });

  // Tell the thumbnail queue what's on screen once scrolling settles.
  $effect(() => {
    const span = onScreen;
    void library.pageTick;
    const timer = setTimeout(() => {
      if (!span) return;
      const ids: number[] = [];
      for (let o = span[0]; o < span[1]; o++) {
        const e = library.entry(o);
        if (e) ids.push(e.id);
      }
      api.setVisible(ids).catch(() => {});
    }, VISIBLE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  export function scrollToOffset(offset: number, align: 'start' | 'nearest' = 'start') {
    const i = rowOfItem(rows, offset);
    if (i < 0 || !viewport) return;
    // Before the first layout pass `height` is still 0 (e.g. Task 14 jumping to a
    // folder right after mount): 'nearest' would then over-scroll by a row, so wait
    // for a real viewport height. 'start' doesn't depend on `height` and stays exact.
    if (align === 'nearest' && height === 0) return;
    const row = rows[i];
    if (align === 'start') {
      const header = rows[i - 1];
      viewport.scrollTop = header?.kind === 'header' && header.first === row.first ? header.top : row.top;
    } else if (row.top < viewport.scrollTop) {
      viewport.scrollTop = row.top;
    } else if (row.top + row.height > viewport.scrollTop + height) {
      viewport.scrollTop = row.top + row.height - height;
    }
    // Every programmatic scroll hands the pin over to where it just put the user, rather
    // than leaving it saying where they were. The browser's scroll event is a task away, so
    // until it arrives the pin would otherwise describe a place nobody is at any more - and
    // a size change landing in that gap (on launch both sit behind IPC round trips) would
    // scroll back to it. Read back from `viewport`, not from the value written above: the
    // browser clamps a scroll past the end of the canvas, and the pin has to name the row
    // that is actually at the top.
    pinned = firstVisibleOffset(rows, viewport.scrollTop);
  }

  /** Keeping the user's place when the tile size changes.
   *
   *  Every row's `top` is computed from the tile width, so the pixel position the viewport
   *  is holding names a different photo the instant the width moves - the grid jumps to
   *  another year when the tiles grow. The photo to come back to therefore has to be read
   *  from the layout as it was *before* the change and scrolled to in the layout as it is
   *  after, and one run of an effect can only ever see one of those: `rows` is a `$derived`,
   *  so the run woken by the new width already reads the new rows. The pin is kept current
   *  on every run where the width has *not* moved - a scroll, a resize, a rebuilt index -
   *  and by `scrollToOffset`, which re-pins whatever it scrolls to; the run that sees a new
   *  width spends the pin instead of taking it again.
   *
   *  All three values are read on every run, the restoring one included: an effect depends
   *  only on what that run read, so a restoring run that skipped `scrollTop` would stop
   *  hearing about scrolls and pin a stale offset for the next change.
   *
   *  `$effect`, not `$effect.pre`: the canvas is only as tall as the old layout until the
   *  DOM catches up, and a scroll into the part that does not exist yet is clamped away. */
  let pinnedWidth = gridSize.width;
  let pinned: number | null = null;
  $effect(() => {
    const tile = gridSize.width;
    const layout = rows;
    const top = scrollTop;
    if (tile !== pinnedWidth) {
      pinnedWidth = tile;
      // The pin is spent unconditionally, and it is `scrollToOffset` that keeps it honest:
      // every programmatic scroll re-pins (see there), so a jump the browser has not yet
      // reported - the launch restore, App's jump to the top on a view change - has already
      // handed this its own folder rather than leaving offset 0 behind to drag the user
      // back to the top of the library.
      //
      // Do not guard this on the viewport still standing where the pin was taken, however
      // obviously right that reads. By the time this runs Svelte's render effect has already
      // written the new `style:height` onto `.canvas`, and when the tiles *shrink* the canvas
      // shrinks with them: the browser then clamps `scrollTop` to the shorter canvas
      // synchronously, during the very layout that reading `viewport.scrollTop` forces,
      // before any scroll event exists. Measured in headless Chromium: a 600px-tall scroller
      // at 5000 whose content went 10000 -> 3000 read back 2400 in the same task. So on every
      // shrink deep enough to clamp, the comparison fails, the restore is skipped, and the
      // user is left wherever the clamp dropped them - the end of the library.
      if (pinned !== null) scrollToOffset(pinned, 'start');
      return;
    }
    pinned = firstVisibleOffset(layout, top);
  });

  export function focus() {
    viewport?.focus();
  }


  function onkeydown(e: KeyboardEvent) {
    // A drag owns the grid while it lasts: Escape abandons it (below), and every other key
    // is ignored rather than acted on. Enter is the one that mattered - it opens the viewer,
    // which makes the grid `inert`, and `inert` does not stop a running animation frame: the
    // band would go on scrolling and rewriting the selection behind the photo.
    if (band && e.key !== 'Escape') return;
    const sel = library.selected;
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      // One path is all a file manager takes, so this follows the same rule as the menu's
      // Reveal item: only when exactly one photo is selected, not just the lead of a wider
      // selection.
      if (library.selectionCount !== 1) return;
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) api.revealInFileManager(entry.id).catch(library.reportError);
      return;
    }
    if (isCopyPhotoShortcut(e, (window.getSelection()?.toString() ?? '') !== '')) {
      e.preventDefault();
      // The clipboard holds one picture, so this follows the Reveal rule: exactly one photo
      // selected, not just the lead of a wider selection.
      if (library.selectionCount !== 1) return;
      const entry = sel === null ? undefined : library.entry(sel);
      if (entry) void library.copyPhoto(entry.id);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      // preventDefault or the webview selects the chrome's own text behind the grid.
      e.preventDefault();
      library.selectAll().catch(library.reportError);
      return;
    }
    if (e.key === 'Escape') {
      // A drag in progress owns this Escape: clearing the selection is the opposite of
      // putting it back, and this handler runs first when the viewport has focus.
      if (cancelBandKey()) return;
      // The window handler closes the menu on Escape. Clearing here as well would do both at
      // once, so the first Escape only ever dismisses the menu.
      if (menu) return;
      library.clearSelection();
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (sel !== null) onopen(sel);
      return;
    }
    if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === 'c') {
      // The guard excludes every modifier so this never answers a chord that belongs to the
      // webview or the OS - the browser's copy chord, or Alt+C on a layout that composes with
      // it. Ctrl+A and Ctrl+Shift+R above require their modifiers, this one requires their
      // absence, which is the same rule Compare's own key handler applies to its letters.
      // Same 2-4 rule Compare enforces on its own panes (`canCompare`); consulted here
      // rather than re-expressed, so the range lives in one place.
      if (!canCompare(library.selectionCount)) return;
      e.preventDefault();
      oncompare(library.selectedItemIds);
      return;
    }
    if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.toLowerCase() === 'h') {
      // The menu's Hide (or, in Hidden, Unhide), under the same no-modifier rule as C. Not
      // Delete: photon never deletes, and a key that reads as "delete" would promise it.
      // `setHidden` moves the selection to the next photo that stays, so this can be
      // pressed again and again through Duplicates; the scroll follows it there.
      const ids = library.selectedItemIds;
      if (ids.length === 0) return;
      e.preventDefault();
      library
        .setHidden(ids, library.info.view !== 'hidden')
        .then(() => {
          if (library.selected !== null) scrollToOffset(library.selected, 'nearest');
        })
        .catch(library.reportError);
      return;
    }
    if (!NAV_KEYS.includes(e.key) || library.info.len === 0) return;
    e.preventDefault();
    const next = move(sel, e.key as NavKey, sections, columns);
    library.selected = next;
    scrollToOffset(next, 'nearest');
  }
  // ---- the tile's context menu ----

  let menu = $state<{ x: number; y: number } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();
  /** The copy count of the one photo the menu was opened on, once the backend answers.
   *  `menuSeq` makes a late answer harmless: one for a menu since closed, or reopened on a
   *  different photo, would otherwise offer that photo's copies under this one. */
  let menuCopies = $state<{ id: number; count: number } | null>(null);
  let menuSeq = 0;

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  /** Shift extends, Ctrl/Cmd toggles, a plain click collapses to one. Shift wins when both
   *  are held, which is what every file manager does. */
  function tileClick(e: MouseEvent, offset: number) {
    if (e.shiftKey) {
      void library.extendSelection(offset).catch(library.reportError);
    } else if (e.ctrlKey || e.metaKey) {
      library.toggleSelected(offset);
    } else {
      library.selected = offset;
    }
  }

  /** The rubber band, in the canvas's own coordinates - the same space `Row.top` is in, so
   *  the band keeps its grip on the photos it was started over while the wheel scrolls
   *  under it. `null` when no button is down. */
  let band = $state<Rect | null>(null);
  /** True once the pointer has moved past the threshold: until then the press is still a
   *  click, and nothing is drawn or selected. */
  let banding = $state(false);
  /** The pointer that owns the current drag. A second finger's events - and the right
   *  button's `pointerup` during a left drag - must not steer or end someone else's band. */
  let bandPointer: number | null = null;
  /** Set when a band ends, so the click that follows the release does not also land on a
   *  tile and collapse the selection the band just made. */
  let swallowClick = false;

  /** Where the pointer was last seen, in the window's coordinates. The autoscroll loop
   *  needs it: the grid moves under a pointer that is holding still, so every frame has to
   *  ask again what canvas position that same screen position now names. */
  let bandAt: { x: number; y: number } | null = null;
  /** The running autoscroll frame, if any. */
  let bandScroll: number | null = null;
  /** When the last autoscroll frame ran, and the sub-pixel part of the scroll it could not
   *  apply. Without the remainder a one-pixel-deep hold asks for a fraction of a pixel per
   *  frame for ever and the grid never moves at all. */
  let bandScrollAt = 0;
  let bandScrollRest = 0;

  /** A point in the window's coordinates, in the canvas's. */
  function atCanvas(x: number, y: number): { x: number; y: number } {
    const box = viewport.getBoundingClientRect();
    return { x: x - box.left + viewport.scrollLeft, y: y - box.top + viewport.scrollTop };
  }

  function bandRanges(rect: Rect): [number, number][] {
    return itemsInRect(rows, rect, gridSize.width);
  }

  function bandDown(e: PointerEvent) {
    // The left button only: the right one opens the menu, and the middle one is nothing.
    // A press that lands on the open menu belongs to the menu.
    if (e.button !== 0 || (e.target as HTMLElement).closest('.menu')) return;
    // A press already owns the grid: a second finger must not take it over, or the band it
    // starts would capture `bandPrevious` from the first one's preview and the real
    // selection would be gone for good.
    if (bandPointer !== null) return;
    // The viewport is the scrolling element, so a press on its own scrollbar arrives here.
    // Without this the thumb's drag starts a band whose far corner races down the canvas
    // with the scroll, and the release selects everything it passed.
    if (e.offsetX > viewport.clientWidth || e.offsetY > viewport.clientHeight) return;
    // The menu's own dismissal is the click that follows a press, and a band swallows that
    // click; left alone the menu would sit over the new selection describing the old one.
    menu = null;
    bandPointer = e.pointerId;
    bandAt = { x: e.clientX, y: e.clientY };
    const at = atCanvas(e.clientX, e.clientY);
    band = { x0: at.x, y0: at.y, x1: at.x, y1: at.y };
    banding = false;
    // A release whose click never arrived (the pointer left the window under capture) would
    // otherwise leave the flag set and swallow this press's click instead.
    swallowClick = false;
  }

  function bandMove(e: PointerEvent) {
    if (!band || e.pointerId !== bandPointer) return;
    bandAt = { x: e.clientX, y: e.clientY };
    const at = atCanvas(e.clientX, e.clientY);
    if (!banding) {
      if (Math.abs(at.x - band.x0) < BAND_THRESHOLD && Math.abs(at.y - band.y0) < BAND_THRESHOLD) return;
      banding = true;
      // Capture, so the drag survives the pointer leaving the viewport or the window. It
      // throws for a pointer the browser no longer considers active (and for the synthetic
      // ones the screenshot harness dispatches); the band works without it, ending early if
      // the pointer leaves, so this is reported and not fatal.
      try {
        viewport.setPointerCapture(e.pointerId);
      } catch {
        // Nothing to do: the drag continues uncaptured.
      }
      library.beginBand(e.ctrlKey || e.metaKey || e.shiftKey);
    }
    dragTo(at.x, at.y);
    // The pointer may have come to rest in the margin, where nothing more will be heard from
    // it until it moves again; the loop is what keeps the grid moving under it.
    startBandScroll();
  }

  /** Moves the band's far corner and previews what it now covers. */
  function dragTo(x: number, y: number) {
    if (!band) return;
    band = { ...band, x1: x, y1: y };
    library.bandTo(bandRanges(band));
  }

  /** Scrolls the grid while the band is held near an edge, a frame at a time.
   *
   *  The band's far corner is recomputed from the pointer's *screen* position each frame,
   *  because the canvas has moved under it: without that the rectangle would stay the size
   *  it was and the scroll would slide the grid out from under it.
   *
   *  The preview is still drawn from the loaded pages, so tiles scrolled past before their
   *  page arrives do not ring at once - they ring on the next scrolling frame, since each
   *  one previews again. `endBand`'s fetch is what makes the result right either way, which
   *  is the whole reason the band is answered twice.
   *
   *  The loop keeps running while the pointer is in a margin, even where the grid cannot
   *  move - at the end of the library, or when the step rounds away to nothing. It is one
   *  idle frame either way, and stopping there means never starting again: the wheel still
   *  scrolls during a drag (so the end can stop being the end) and a scan can lengthen the
   *  grid, and neither of those sends a pointer event to restart anything. */
  function startBandScroll() {
    if (bandScroll !== null || !banding) return;
    bandScrollAt = performance.now();
    bandScrollRest = 0;
    const step = (now: number) => {
      bandScroll = null;
      if (!banding || !band || !bandAt) return;
      const box = viewport.getBoundingClientRect();
      const speed = edgeScrollSpeed(bandAt.y, box.top, box.bottom, BAND_EDGE, BAND_SCROLL_MAX);
      if (speed === 0) {
        bandScrollRest = 0;
        bandScrollAt = now;
        return;
      }
      const elapsed = Math.min(BAND_FRAME_MAX_MS, Math.max(0, now - bandScrollAt));
      bandScrollAt = now;
      const wanted = (speed * elapsed) / 1000 + bandScrollRest;
      const whole = Math.trunc(wanted);
      bandScrollRest = wanted - whole;
      if (whole !== 0) viewport.scrollTop += whole;
      const at = atCanvas(bandAt.x, bandAt.y);
      dragTo(at.x, at.y);
      bandScroll = requestAnimationFrame(step);
    };
    bandScroll = requestAnimationFrame(step);
  }

  function stopBandScroll() {
    if (bandScroll !== null) cancelAnimationFrame(bandScroll);
    bandScroll = null;
    bandScrollRest = 0;
    bandAt = null;
  }

  function bandUp(e: PointerEvent) {
    // Another button or another finger releasing says nothing about this drag.
    if (!band || e.pointerId !== bandPointer || e.button !== 0) return;
    bandPointer = null;
    const finished = banding ? band : null;
    band = null;
    banding = false;
    stopBandScroll();
    if (!finished) return;
    // The release is followed by a click on whatever is under it; without this a band that
    // ended over a tile would collapse to that one photo.
    swallowClick = true;
    focus();
    library.endBand(bandRanges(finished)).catch(library.reportError);
  }

  /** Escape abandons the drag and puts the selection back. Returns whether it did, because
   *  the grid's own Escape clears the selection - the opposite - and must not also run.
   *  Both the viewport's handler and the window's call this: the viewport has focus during a
   *  drag that began with a click, but a drag whose press did not focus it does not, so
   *  neither handler alone covers every Escape. */
  /** Abandons a drag that was interrupted rather than finished: the browser taking the
   *  gesture for a pan (`pointercancel`, which a touchscreen sends after a few moves), or
   *  the viewer opening on top of the grid.
   *
   *  Before autoscroll a missed teardown left a stuck rectangle; now it leaves a loop that
   *  scrolls to the end of the library rewriting the selection as it goes, and a
   *  `bandPointer` that never clears, so no band can be started again. Every way out of a
   *  drag has to come through here. */
  function abandonBand() {
    if (banding) library.cancelBand();
    band = null;
    banding = false;
    bandPointer = null;
    stopBandScroll();
  }

  function cancelBandKey(): boolean {
    if (!band) return false;
    abandonBand();
    return true;
  }

  /** Right-clicking outside the selection selects that tile first, so what the menu acts on
   *  is always what is outlined. Inside it, the whole selection stands. */
  function tileMenu(e: MouseEvent, offset: number) {
    e.preventDefault();
    const entry = library.entry(offset);
    if (!entry) return;
    if (!library.isSelected(entry.id)) library.selected = offset;
    menu = { x: e.clientX, y: e.clientY };
    const seq = ++menuSeq;
    menuCopies = null;
    if (library.selectionCount === 1) {
      const id = entry.id;
      api
        .copyCount(id)
        .then((count) => {
          if (seq === menuSeq && menu) menuCopies = { id, count };
        })
        // No item is the right answer to a failed lookup: the menu's other verbs still work.
        .catch(() => {});
    }
  }

  function closeMenu() {
    menu = null;
  }

  /** "1 photo" / "12 photos", for a message naming a specific count. */
  function counted(n: number): string {
    return n === 1 ? '1 photo' : `${n.toLocaleString()} photos`;
  }

  const count = $derived(library.selectionCount);
  /** "photo" / "12 photos", for menu items that name what they will act on. */
  const subject = $derived(count === 1 ? 'photo' : counted(count));

  function withSelection(action: (ids: number[]) => Promise<unknown>) {
    const ids = library.selectedItemIds;
    menu = null;
    if (ids.length) action(ids).catch(library.reportError);
    focus();
  }

  /** Unlike the other verbs, this one does not act on the click: the keyword dialog is an
   *  overlay, so App owns it - everything behind an overlay is made `inert` there, and a
   *  dialog mounted inside the grid would be one of the things made inert. All this does is
   *  close the menu and ask. Focus comes back to the grid when App closes the dialog. */
  function pickKeyword(mode: 'add' | 'remove') {
    menu = null;
    onkeywords(mode);
  }

  /** Stars or unstars everything selected. The backend skips a folder whose `.picasa.ini`
   *  it cannot write and answers with how many landed, so a read-only folder in the
   *  selection costs the user a toast rather than the other eleven photos. */
  async function star(ids: number[], starred: boolean) {
    const done = await api.setStars(ids, starred);
    if (done < ids.length) {
      throw new Error(
        `${counted(ids.length - done)} of ${ids.length.toLocaleString()} could not be ${starred ? 'starred' : 'unstarred'}`,
      );
    }
  }
</script>

<svelte:window
  onclick={closeMenu}
  onkeydown={(e) => {
    if (e.key !== 'Escape') return;
    // A drag in progress owns this Escape; the menu keeps its own, as it always has.
    if (cancelBandKey()) return;
    closeMenu();
  }}
  onpointerup={bandUp}
/>

<div class="grid">
  <div
    class="viewport"
    bind:this={viewport}
    bind:clientWidth={width}
    bind:clientHeight={height}
    onscroll={() => (scrollTop = viewport.scrollTop)}
    onpointerdown={bandDown}
    onpointermove={bandMove}
    onpointercancel={abandonBand}
    onclickcapture={(e) => {
      if (!swallowClick) return;
      swallowClick = false;
      e.stopPropagation();
    }}
    {onkeydown}
    tabindex="0"
    role="grid"
    aria-label="Photos"
  >
    {#if library.info.len === 0}
      <p class="empty">
        {#if library.info.view === 'starred'}
          No starred photos. Star one in the viewer, or in Picasa.
        {:else if library.info.view === 'search'}
          No photos match “{library.info.searchQuery}”
        {:else if library.info.view === 'album'}
          {#if isOwnAlbum(library.albums, library.info.album)}
            “{library.albumName(library.info.album)}” is empty. Right-click a photo to add it.
          {:else}
            This album has no photos in the library.
          {/if}
        {:else if library.info.view === 'person'}
          No photos of {library.personName(library.info.person)}.
        {:else if library.info.view === 'duplicates'}
          No duplicates. Every photo in the library is the only copy of itself.
        {:else if library.info.view === 'copies'}
          {copiesNotice(library.info.copiesOf, library.info.len, library.entry(0)?.id)}
        {:else if library.info.view === 'tag'}
          No photos tagged “{library.info.tag}”.
        {:else if library.info.view === 'hidden'}
          No hidden photos. Right-click a photo and choose Hide to put it away here.
        {:else}
          No photos yet. Add a folder to get started.
        {/if}
      </p>
    {/if}
    <div class="canvas" class:banding style:height="{total}px">
      {#if banding && band}
        <div
          class="band"
          style:left="{Math.min(band.x0, band.x1)}px"
          style:top="{Math.min(band.y0, band.y1)}px"
          style:width="{Math.abs(band.x1 - band.x0)}px"
          style:height="{Math.abs(band.y1 - band.y0)}px"
        ></div>
      {/if}
      {#each rendered as row (row.top)}
        {#if row.kind === 'header'}
          {@const folder = library.folderOf(sections[row.section].folderId)}
          <div class="header" style:top="{row.top}px">
            <span class="name">{folder?.name ?? ''}</span>
            <span class="path">{folder?.path ?? ''}</span>
          </div>
        {:else}
          <div class="row" style:top="{row.top}px" style:gap="{GAP}px" style:padding-left="{GAP}px">
            {#each { length: row.count } as _, i (row.first + i)}
              {@const offset = row.first + i}
              {@const entry = library.entry(offset)}
              <Tile
                {entry}
                selected={library.isSelectedTile(offset, entry?.id)}
                dimmed={!!entry && !library.isOnline(entry.folderId)}
                onselect={(e) => tileClick(e, offset)}
                onopen={() => onopen(offset)}
                onmenu={(e) => tileMenu(e, offset)}
                tile={gridSize.width}
              />
            {/each}
          </div>
        {/if}
      {/each}
    </div>
    {#if library.info.view === 'copies' && library.info.len >= 1}
      <!-- The group has shrunk to the photo itself since the view opened (a copy was deleted
           and the rescan purged it), or the anchor itself is gone and its copies are what
           remain on screen. Not `.empty`: that class overlays the whole viewport, which would
           sit on top of the tiles still showing. This sits in normal flow, below the canvas. -->
      {@const notice = copiesNotice(library.info.copiesOf, library.info.len, library.entry(0)?.id)}
      {#if notice}
        <p class="lone">{notice}</p>
      {/if}
    {/if}
  </div>
  {#if scrubbable}
    <Timeline {marks} {total} viewport={height} {scrollTop} onscrub={(top) => (viewport.scrollTop = top)} />
  {/if}
</div>

{#if menu}
  <!-- Only photon's own album offers "Remove from": Picasa's are changed in Picasa. -->
  {@const albumId =
    library.info.view === 'album' && isOwnAlbum(library.albums, library.info.album) ? library.info.album : null}
  <div
    class="menu focus-container"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style:left="{menu.x}px"
    style:top="{menu.y}px"
  >
    {#if count === 1}
      <button role="menuitem" onclick={() => withSelection((ids) => api.revealInFileManager(ids[0]))}>
        Reveal in file manager
      </button>
      <!-- One photo only: the clipboard holds one picture. -->
      <button role="menuitem" onclick={() => withSelection((ids) => library.copyPhoto(ids[0]))}>Copy photo (Ctrl+C)</button>
    {/if}
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, true))}>Star {subject}</button>
    <button role="menuitem" onclick={() => withSelection((ids) => star(ids, false))}>Unstar {subject}</button>
    {#if canCompare(count)}
      <button
        role="menuitem"
        onclick={() => {
          menu = null;
          oncompare(library.selectedItemIds);
        }}>Compare {subject}</button
      >
    {/if}
    <button role="menuitem" onclick={() => pickKeyword('add')}>Add keyword to {subject}…</button>
    <button role="menuitem" onclick={() => pickKeyword('remove')}>Remove keyword from {subject}…</button>
    <button
      role="menuitem"
      onclick={() => {
        menu = null;
        onexport();
      }}>Export {subject}…</button
    >
    {#if albumId !== null}
      <button role="menuitem" onclick={() => withSelection((ids) => library.removeFromAlbum(albumId, ids))}>
        Remove {subject} from “{library.albumName(albumId)}”
      </button>
    {/if}
    {#if library.info.view === 'hidden'}
      <button role="menuitem" onclick={() => withSelection((ids) => library.setHidden(ids, false))}>
        Unhide {subject} (H)
      </button>
    {:else}
      <!-- Not "Delete": photon never deletes a photo. The file stays where it is and the
           Hidden view gives it back. -->
      <button role="menuitem" onclick={() => withSelection((ids) => library.setHidden(ids, true))}>
        Hide {subject} (H)
      </button>
    {/if}
    {#if count === 1 && menuCopies && menuCopies.count > 0}
      {@const id = menuCopies.id}
      <button
        role="menuitem"
        onclick={() => {
          menu = null;
          onshowcopies(id);
        }}>{showCopiesLabel(menuCopies.count)}</button
      >
    {/if}
    <div class="heading">Add to album</div>
    {#each ownAlbums(library.albums) as album (album.id)}
      <!-- Adding is idempotent, so the album the photos are already in is not filtered out
           here: the grid rows do not know their memberships, and asking per photo for a
           menu would be a round trip for nothing. -->
      <button role="menuitem" class="album" onclick={() => withSelection((ids) => library.addToAlbum(album.id, ids))}>
        {album.name}
      </button>
    {:else}
      <div class="none">No albums of your own yet — create one in the sidebar.</div>
    {/each}
  </div>
{/if}

<style>
  .grid { display: flex; height: 100%; background: var(--surface); }
  .viewport { position: relative; flex: 1; min-width: 0; height: 100%; overflow-y: auto; outline: none; }
  /* The grid is in the tab order (tabindex="0"), so tabbing into it must show something -
     with nothing selected there is no tile ring to stand in for it. Drawn inside, like the
     tile's ring and for the same reason: the viewport scrolls a row flush to its own top
     edge, which clips anything outside the box. */
  .viewport:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .canvas { position: relative; }
  /* While a band is drawn, dragging over a folder header must not select its text. */
  .canvas.banding { user-select: none; }
  .band {
    position: absolute;
    z-index: 1;
    background: var(--accent-soft);
    border-radius: var(--r-1);
    box-shadow: 0 0 0 1px var(--accent);
    pointer-events: none;
  }
  .header, .row { position: absolute; left: 0; right: 0; }
  /* 32px is layout.ts's HEADER: every row below is placed by it, so the type fits the box
     rather than the box growing to the type. */
  .header { display: flex; align-items: baseline; gap: var(--s-3); height: 32px; padding: 7px var(--s-2) 0; }
  /* min-width: 0 and overflow: hidden so a folder name wider than the grid ellipsises
     instead of forcing a horizontal scrollbar, which would change the viewport's measured
     clientHeight. max-width caps the name so a long one cannot squeeze .path (flex: 1 1 auto,
     not 1 1 0, so the path keeps its own room rather than starting from nothing) down to
     zero. overflow: hidden moves a flex item's baseline to its own bottom edge, not its
     text's; giving .name and .path the same line-height puts both bottom edges - and so both
     baselines - on the same line, which plain `align-items: baseline` alone no longer does
     once either child clips its own overflow. */
  .header .name { flex: 0 1 auto; max-width: 70%; min-width: 0; overflow: hidden; font-size: var(--t-4); font-weight: 600; line-height: 20px; white-space: nowrap; text-overflow: ellipsis; }
  .header .path { flex: 1 1 auto; min-width: 0; color: var(--text-dim); font-size: var(--t-1); line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; color: var(--text-dim); margin: 0; }
  /* The Copies view's notice line: in normal flow, below the tile(s) still on screen -
     unlike `.empty`, which overlays the whole viewport and would sit on top of them. */
  .lone { padding: var(--s-3); color: var(--text-dim); margin: 0; }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button { padding: 6px 10px; border: 0; border-radius: var(--r-2); background: none; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--hover); }
  .menu .album { padding-left: 18px; }
  .menu .heading {
    margin-top: var(--s-1);
    padding: 6px 10px 2px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
    border-top: 1px solid var(--line);
  }
  .menu .none { padding: 4px 18px 6px; color: var(--text-dim); font-size: var(--t-2); }
</style>
