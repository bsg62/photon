<script lang="ts">
  import { untrack } from 'svelte';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { api, errorMessage, mediaUrl, type ViewerItem } from '../lib/api';
  import { isLinux } from '../lib/url';
  import { videoState } from '../lib/video-state.svelte';
  import { formatDuration, videoUrl } from '../lib/video';
  import { nextStill } from '../lib/slideshow-order';
  import { createVideoPlayer } from '../lib/video-player.svelte';
  import { createAlbumMembership } from '../lib/album-membership.svelte';
  import { ownAlbums, picasaAlbumsOf } from '../lib/albums';
  import { createTagEditor } from '../lib/tag-editor.svelte';
  import { formatCaption } from '../lib/caption';
  import { photoCaptionLine } from '../lib/photo-caption';
  import { isCopyPhotoShortcut } from '../lib/copy-photo';
  import { createCopyFeedback } from '../lib/copied.svelte';
  import { cameraRows, copyGroups, formatDimensions } from '../lib/exif';
  import { showCopiesLabel } from '../lib/copies';
  import { ASPECTS, HANDLES, type Handle } from '../lib/crop';
  import { createCropTool } from '../lib/crop-tool.svelte';
  import { containedBox, faceBox } from '../lib/faces';
  import { createSlideshow } from '../lib/slideshow.svelte';
  import { createStarToggle } from '../lib/star-toggle.svelte';
  import { library } from '../lib/library.svelte';
  import { pictureChanged } from '../lib/picture';
  import Icon from './Icon.svelte';
  import VideoControls from './VideoControls.svelte';
  import {
    MAX_ZOOM,
    MIN_ZOOM,
    clampPan,
    clampZoom,
    closesViewer,
    positionInSection,
    wheelStep,
  } from '../lib/nav';

  let {
    offset,
    onclose,
    onlocate,
    onsearch,
    onshowcopies,
  }: {
    offset: number;
    onclose: (offset: number) => void;
    /** "Locate in photon": the viewer closes and the grid lands on this photo, looking for
     *  it in the Hidden view when `hidden`. */
    onlocate: (itemId: number, hidden?: boolean) => void;
    /** A camera or lens in the info panel was clicked: leave the viewer for that search. */
    onsearch: (query: string) => void;
    /** "Show N duplicates in the grid": the viewer closes and the grid holds this photo and
     *  its copies, as the tile menu's item does. */
    onshowcopies: (itemId: number) => void;
  } = $props();

  const PRELOAD_RADIUS = 2;
  let current = $state(untrack(() => offset));
  let item = $state<ViewerItem | null>(null);
  let fullSrc = $state<string | null>(null);
  let error = $state<string | null>(null);
  let zoom = $state(MIN_ZOOM);
  let pan = $state({ x: 0, y: 0 });
  /** The info panel: camera, keywords, people and albums. Its faces are outlined over the
   *  photo while it is open. */
  let info = $state(false);
  let dragging = $state(false);
  let stage = $state<HTMLDivElement | null>(null);
  /** The frame's on-screen size, for placing the face outlines and the crop rectangle. */
  let frameW = $state(0);
  let frameH = $state(0);
  /** The photo on screen has left the current view but still exists: unstarred while
   *  Starred is showing. It stays up - the user is looking at it - without a position in
   *  the caption, until the next navigation. */
  let orphaned = $state(false);
  /** Bumped to make the loader run again for an offset `current` already holds. */
  let reload = $state(0);
  /** Deliberately not `$state`: nothing renders from a partial wheel total, and making it
   *  reactive would re-run effects on every wheel event of a flick. */
  let wheelTotal = 0;
  let dragFrom = { x: 0, y: 0, panX: 0, panY: 0 };

  // ---- slideshow ----

  /** The photo being left, held opaque over the stage until its successor has decoded and
   *  then faded out: a crossfade rather than a cut through black. Only set during a
   *  slideshow, and only for a photo shown fitted, since this layer knows nothing of zoom
   *  or pan. */
  let outgoing = $state<{ src: string; fading: boolean } | null>(null);
  /** Must match the `.outgoing` transition in the styles below. */
  const CROSSFADE_MS = 600;

  /** The kind at grid offset `i`, loading its page first: what `nextStill` walks. */
  async function kindAt(i: number) {
    await library.ensure(i, i + 1);
    return library.entry(i)?.kind;
  }

  const slideshow = createSlideshow({
    advance: () => {
      const len = library.info.len;
      const from = current;
      void nextStill(from, len, kindAt).then((next) => {
        // The show may have been stopped, or the user may have navigated by hand, while this
        // awaited: either makes `current` no longer `from`, or the slideshow no longer
        // active, and a `goto` landing on top of either would be a stale write.
        if (!slideshow.active || current !== from) return;
        // One photo has no next: `goto` would reload it, blanking the screen every interval.
        if (next !== null && next !== current) goto(next);
      });
    },
    interval: () => api.slideshowInterval(),
    fullscreen: { get: () => api.windowFullscreen(), set: (on) => api.setWindowFullscreen(on) },
  });

  async function startSlideshow() {
    info = false;
    if (item?.kind === 'video') {
      const from = current;
      const len = library.info.len;
      const next = await nextStill(from, len, kindAt);
      // The viewer may have closed, or the user may have navigated elsewhere, while this
      // awaited: closing stops nothing else running, and a late `goto` (and the fullscreen
      // `slideshow.start` below) would otherwise resurrect a slideshow on a viewer nobody is
      // looking at, or hijack wherever the user has since navigated to.
      if (destroyed || current !== from) return;
      if (next === null) {
        library.notify('There are no photos here to show.');
        return;
      }
      goto(next);
    }
    void slideshow.start(fullSrc !== null || error !== null);
  }

  function stopSlideshow() {
    slideshow.stop();
    outgoing = null;
  }

  /** One step by hand - an arrow or the wheel. Outside a show it is `goto`, unchanged.
   *  During one it skips videos in the direction of travel: the show never plays a video,
   *  so landing on one by hand would leave it sitting on a still poster, its timer running,
   *  until the next advance moved it on. It wraps as the show's own advance does, and it
   *  takes the same two staleness guards as `advance`, for the same reason: the walk
   *  awaits pages, and the show may have stopped or the user moved on meanwhile. */
  function step(dir: 1 | -1) {
    if (!slideshow.active) {
      goto(current + dir);
      return;
    }
    const from = current;
    void nextStill(from, library.info.len, kindAt, dir).then((next) => {
      if (!slideshow.active || current !== from) return;
      if (next !== null && next !== current) goto(next);
    });
  }

  // The countdown runs from the moment the photo is on screen; a photo that cannot be shown
  // counts as shown, so one bad file does not end the show.
  //
  // `outgoing` is read untracked, and that is load-bearing: `goto` sets it while the old
  // `fullSrc` is still in place, so an effect that woke for it would fade the old photo out
  // before the new one had even been asked for. The layer removes itself on a timer rather
  // than on `transitionend`, which never fires when a preloaded photo decodes within the
  // frame the layer was inserted in - no transition runs, and the layer would stay forever.
  $effect(() => {
    if (fullSrc === null && error === null) return;
    slideshow.shown();
    untrack(() => {
      const leaving = outgoing;
      if (!leaving) return;
      leaving.fading = true;
      setTimeout(() => {
        if (outgoing === leaving) outgoing = null;
      }, CROSSFADE_MS + 100);
    });
  });

  // Closing by any route - Escape, the back button, the grid going away - leaves fullscreen.
  $effect(() => () => slideshow.stop());

  /** Set once, on unmount: `startSlideshow`'s await for a still to land on can span the
   *  viewer closing underneath it, and a `goto`/fullscreen landing afterwards on a viewer
   *  nobody is looking at would never be stopped by anything. Not `$state`: nothing renders
   *  it, and it must not itself wake the effect that sets it. */
  let destroyed = false;
  $effect(() => () => {
    destroyed = true;
  });

  /** Photos are numbered within their own folder, not across the library. In the All view
   *  that count matches what the file manager shows for that directory; in Starred or
   *  Search it is the folder's position among the current view's results instead, since
   *  those views only show a subset of the folder's photos. Recent is laid out as one run
   *  and so is numbered flat — see `positionInSection`. */
  const position = $derived(positionInSection(library.info.sections, current));
  const caption = $derived(item ? formatCaption(item, orphaned ? { index: 0, count: 0 } : position) : '');
  const captionLine = $derived(item ? photoCaptionLine(item.caption) : null);

  // The star goes into the folder's Picasa INI, then the library; the rebind below sees the
  // rebuild that follows. In the Starred view that rebuild is the photo leaving the grid,
  // which is what `orphaned` is for.
  const star = createStarToggle(api.setStar);

  function toggleStar() {
    star.toggle().catch(library.reportError);
  }

  // The album checkboxes in the info panel. Bound with the star, per photo, and optimistic
  // for the same reason.
  const membership = createAlbumMembership({
    add: (albumId, ids) => library.addToAlbum(albumId, ids),
    remove: (albumId, ids) => library.removeFromAlbum(albumId, ids),
  });

  function toggleAlbum(albumId: number) {
    membership.toggle(albumId).catch(library.reportError);
  }

  // The info panel's checkboxes are photon's albums only; Picasa's are listed read-only, and
  // only the ones this photo is in - every one unchecked would bury photon's own.
  const ownAlbumList = $derived(ownAlbums(library.albums));
  const picasaHere = $derived(item ? picasaAlbumsOf(library.albums, item.albums) : []);

  // The tag editor in the info panel. Bound per photo with the star and the album
  // checkboxes, and optimistic for the same reason.
  const tags = createTagEditor({
    add: (id, tag) => api.addItemTag(id, tag),
    remove: (id, tag) => api.removeItemTag(id, tag),
  });

  function addTag() {
    const draft = tags.draft;
    tags.draft = '';
    tags.add(draft).catch((e) => {
      // Give the user back what they typed, but only if they haven't already started
      // typing something else while the call was in flight.
      if (tags.draft === '') tags.draft = draft;
      library.reportError(e);
    });
  }

  function removeTag(tag: string) {
    tags.remove(tag).catch(library.reportError);
  }

  // ---- edits ----
  //
  // Nothing here draws an edit. A turn or a crop is written to the library, which renders
  // it into the thumbnails and the full image; the rebuild that follows hands the rebind
  // effect below a new `thumbKey`, and that reloads the picture. So an edit reaches the
  // screen the same way a file changed on disk does.

  /** Whether the photo on screen can be edited: loaded, showable, and still in this view
   *  (an edit reloads the picture, and an orphaned photo has no offset to reload from). */
  const editable = $derived(!!item && !error && !orphaned);

  function rotate(direction: 'cw' | 'ccw') {
    if (!item || !editable) return;
    api.rotateItem(item.id, direction === 'cw').catch(library.reportError);
  }

  function resetEdit() {
    if (!item || !editable) return;
    api.setItemEdit(item.id, 0, null).catch(library.reportError);
  }

  const crop = createCropTool();
  /** Where the uncropped picture sits in the frame while cropping: the rectangle's
   *  fractions are fractions of this box. */
  const cropBox = $derived(
    crop.active && item ? containedBox(item.uncroppedWidth, item.uncroppedHeight, frameW, frameH) : null,
  );

  function startCrop() {
    if (!item || !editable) return;
    stopSlideshow();
    info = false;
    zoom = MIN_ZOOM;
    pan = { x: 0, y: 0 };
    crop.begin(item.edit?.crop, item.uncroppedWidth, item.uncroppedHeight);
  }

  function applyCrop() {
    if (!item) return;
    // The tool closes only once the write has succeeded, so a refused rectangle leaves the
    // user where they can fix it rather than back in the viewer with nothing changed.
    api
      .setItemEdit(item.id, item.edit?.turns ?? 0, crop.wire())
      .then(() => crop.cancel())
      .catch(library.reportError);
  }

  function cropPointerDown(e: PointerEvent, handle: Handle) {
    if (e.button !== 0) return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    crop.startDrag(handle, e.clientX, e.clientY);
  }

  function cropPointerMove(e: PointerEvent) {
    if (cropBox) crop.dragTo(e.clientX, e.clientY, cropBox.width, cropBox.height);
  }

  const isVideo = $derived(item?.kind === 'video');
  let videoEl = $state<HTMLVideoElement | null>(null);
  /** photon's own controls for the video on screen (`VideoControls`). One player for the
   *  viewer, following whichever element is mounted; loop, mute and volume live in
   *  `sessionPrefs`, so they carry over from one video to the next. */
  const player = createVideoPlayer();
  $effect(() => {
    const el = videoEl;
    if (el) return player.attach(el);
  });
  /** The `<video>` on screen reported an error: it would otherwise sit there black, with
   *  controls that do nothing. Reset by the loader for every photo it loads. */
  let playbackFailed = $state(false);

  /** Why a video can't be played here, reactively: read directly in the derived's own
   *  synchronous body rather than inside the load effect's async chain, whose reads past its
   *  first `await` Svelte no longer tracks - so a viewer opened before `App`'s `onMount` sets
   *  `videoState` (base and support are both known late) notices the moment they land,
   *  instead of being stuck on whatever it saw at load. `base === null` is the server not up
   *  yet (or never coming up); GStreamer's missing plugins are a Linux-only fix, so that
   *  wording is reserved for exactly that case. */
  const videoIssue = $derived.by((): 'crashed' | 'gstreamer' | 'generic' | 'playback' | null => {
    if (!item || item.kind !== 'video') return null;
    // First, ahead of everything: a video the crash-loop guard failed took the window down
    // when it was opened, so no `<video>` may be made for it, whatever else is true. The
    // backend says so as a flag (`videoCrashed`) rather than the UI matching `thumbError`
    // against a copy of the guard's sentence, which nothing would keep in step.
    if (item.videoCrashed) return 'crashed';
    if (!videoState.supported || !videoState.base) {
      return !videoState.supported && isLinux() ? 'gstreamer' : 'generic';
    }
    return playbackFailed ? 'playback' : null;
  });
  const videoIssueMessage = $derived(
    videoIssue === 'crashed'
      ? (item?.thumbError ?? "This video can't be opened.")
      : videoIssue === 'gstreamer'
        ? "This system can't play videos. Install GStreamer's good and libav plugins (see the README's video section)."
        : videoIssue === 'generic'
          ? "This video can't be played here. See the README's video section for details."
          : videoIssue === 'playback'
            ? "This video can't be played here."
            : null,
  );
  /** The src the `<video>` plays, computed the same reactive way as `videoIssue`: a plain
   *  function of `item`, `videoState.base` and whether there is an issue, with nothing to
   *  await. */
  const videoFullSrc = $derived(
    item && item.kind === 'video' && videoIssue === null && videoState.base
      ? videoUrl(videoState.base, item.id)
      : null,
  );

  const camera = $derived(item ? cameraRows(item) : []);
  const copies = $derived(item ? copyGroups(item.copies) : []);
  /** The photo as displayed, orientation applied: the coordinates Picasa's faces are in. */
  const oriented = $derived.by(() => {
    if (!item) return { width: 0, height: 0 };
    const quarter = item.orientation >= 5 && item.orientation <= 8;
    return quarter ? { width: item.height, height: item.width } : { width: item.width, height: item.height };
  });
  const faceBoxes = $derived.by(() => {
    if (!item || !info || crop.active) return [];
    const image = containedBox(oriented.width, oriented.height, frameW, frameH);
    return item.faces.map((f) => ({ name: f.name, box: faceBox(f, image) }));
  });

  // Click-to-copy on the caption. The clipboard goes through the Tauri plugin rather than
  // `navigator.clipboard`, which needs a secure context and answers differently in the
  // three webviews photon ships in.
  const copy = createCopyFeedback(writeText);
  $effect(() => () => copy.dispose());

  function copyName() {
    if (item) copy.copy(item.fileName).catch(library.reportError);
  }

  let menu = $state<{ x: number; y: number } | null>(null);
  let menuEl = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  function oncontextmenu(e: MouseEvent) {
    e.preventDefault();
    if (item) menu = { x: e.clientX, y: e.clientY };
  }

  function closeMenu() {
    menu = null;
  }

  function locate() {
    if (!item) return;
    closeMenu();
    onlocate(item.id, item.hidden);
  }

  /** Hide or unhide the photo on screen. It leaves the view it is in either way, and the
   *  viewer's `orphaned` state - built for unstarring in Starred - keeps it on screen with
   *  the arrow keys carrying on from where it was, so nothing here has to move the viewer. */
  function toggleHidden() {
    if (!item) return;
    closeMenu();
    api.setItemsHidden([item.id], !item.hidden).catch(library.reportError);
  }

  function reveal() {
    if (!item) return;
    closeMenu();
    api.revealInFileManager(item.id).catch(library.reportError);
  }

  function openInApp() {
    if (!item) return;
    closeMenu();
    api.openInDefaultApp(item.id).catch(library.reportError);
  }

  function viewport(): { width: number; height: number } {
    return { width: stage?.clientWidth ?? 0, height: stage?.clientHeight ?? 0 };
  }

  function goto(next: number) {
    const last = library.info.len - 1;
    if (last < 0) return;
    // A menu opened on the previous photo would otherwise vanish while this one loads and
    // reappear over it, having eaten one Escape on the way.
    menu = null;
    // An orphaned photo holds no offset: whatever sits at `current` now is its right-hand
    // neighbour, so "next" is `current` itself, and `current` has to reload rather than
    // stay - assigning it the value it already has would wake nothing.
    if (orphaned && next === current + 1) next = current;
    const target = Math.min(last, Math.max(0, next));
    // `!isVideo`, even though a video never sets `fullSrc` today: the crossfade layer is an
    // `<img>` (see the markup below), so a video's URL painted into it would show nothing -
    // this is the guard against that regressing if `fullSrc` is ever reused for one.
    if (slideshow.active && fullSrc && !isVideo && target !== current && zoom === MIN_ZOOM) {
      outgoing = { src: fullSrc, fading: false };
    }
    if (target === current) reload++;
    else current = target;
  }

  /** The offset handed back to the grid on close. An orphaned photo's offset can sit past
   *  the end of the view that dropped it. */
  function close() {
    stopSlideshow();
    onclose(Math.max(0, Math.min(current, library.info.len - 1)));
  }


  /** An offset the rebind below has already resolved, so the loader can tell "the same photo,
   *  renumbered" from "a different photo". Deliberately not `$state`: writing it must not
   *  wake anything, and it is always set immediately before the `current` that does. */
  let rebound: number | null = null;

  // `current` is an index into a grid that is rebuilt whole whenever anything changes, so it
  // stops meaning "the photo the user opened" the moment a scan indexes something ahead of
  // it: one photo copied into an earlier folder shifts every later offset by one, and the
  // viewer would go on showing the *next* photo under the same caption, silently. Clamping
  // alone only catches the case where the offset falls off the end.
  //
  // So the photo is re-found by id after every rebuild. Only when nothing is loaded yet —
  // the first paint, or after the photo has gone — is there an id to work from, and staying
  // in range is then all that can be done.
  /** Rebuild effects issued so far; see its use below. Not state: nothing renders it. */
  let detailsSeq = 0;

  /** Takes a re-read of the photo on screen. A change the picture itself shows - the file
   *  rewritten (a new thumbnail key or size), or no longer decodable - reloads the photo,
   *  which resets the zoom as any other new picture does. Anything else replaces `item` in
   *  place, keeping zoom and pan. The star and album toggles are not rebound;
   *  they hold their own state and may be mid-write.
   *
   *  `canReload` is false for a photo this view no longer holds: a reload loads whatever
   *  sits at `current`, which for that photo is some other one. Its picture is left as it
   *  is, and its details too, since they would describe a picture not on screen. */
  function refreshDetails(fresh: ViewerItem, canReload: boolean) {
    const old = untrack(() => item);
    if (!old || old.id !== fresh.id) return;
    if (pictureChanged(old, fresh)) {
      if (canReload) {
        rebound = null;
        reload++;
      }
    } else {
      item = fresh;
    }
  }

  $effect(() => {
    void library.info.version;
    const last = library.info.len - 1;
    const showing = untrack(() => item?.id);
    // Rebuilds come every 250ms during a scan, and their replies can arrive out of order;
    // the photo's id cannot tell an older reply from a newer one, this can.
    const seq = ++detailsSeq;
    if (showing === undefined) {
      if (last >= 0 && untrack(() => current) > last) current = last;
      return;
    }
    void (async () => {
      const at = await api.gridOffsetOfItem(showing);
      // The user navigated while this was in flight; that move is the newer truth.
      if (untrack(() => item?.id) !== showing) return;
      if (at === null) {
        // Left this view, or left the library? Only the second deserves a message, and only
        // the backend can tell: it refuses a photo the scanner has marked missing.
        let fresh: ViewerItem | null = null;
        try {
          fresh = await api.viewerItem(showing);
        } catch {
          fresh = null;
        }
        if (untrack(() => item?.id) !== showing || seq !== detailsSeq) return;
        if (fresh) {
          orphaned = true;
          refreshDetails(fresh, false);
        } else error = 'This photo is no longer available.';
        return;
      }
      orphaned = false;
      if (at !== untrack(() => current)) {
        rebound = at;
        current = at;
      }
      // The photo is the same, but what the library says about it may not be: a renamed
      // or removed tag, a face named in Picasa, a rescan's metadata.
      try {
        const fresh = await api.viewerItem(showing);
        if (untrack(() => item?.id) === showing && seq === detailsSeq) refreshDetails(fresh, true);
      } catch {
        // Gone between the two calls; the next rebuild reports it.
      }
    })();
  });

  $effect(() => {
    const at = current;
    void reload;
    // A renumbering, not a navigation: the photo on screen is already the right one, so
    // reloading it would blank it and throw away the zoom and pan for nothing.
    if (rebound === at) {
      rebound = null;
      return;
    }
    let cancelled = false;
    slideshow.changed();
    item = null;
    fullSrc = null;
    error = null;
    playbackFailed = false;
    orphaned = false;
    // Every photo opens fitted to the window: arriving at the next one already at 400% or
    // panned into a corner leaves you lost. A crop being drawn belonged to the last photo.
    zoom = MIN_ZOOM;
    pan = { x: 0, y: 0 };
    crop.cancel();
    (async () => {
      // `untrack`, because `ensure` reads `library.info.len` and this call is still inside
      // the effect's tracked window. `refresh()` assigns a new `info` object on every
      // library-changed event, so without it any background scan finishing - or any single
      // watched file changing - re-runs this effect, blanking the photo on screen and
      // throwing away the zoom and pan the user set, although nothing about that photo
      // changed. The one dependency this effect wants is `current`.
      await untrack(() => library.ensure(at, at + 1));
      const entry = library.entry(at);
      if (cancelled) return;
      if (!entry) {
        error = 'This photo is no longer available.';
        return;
      }
      const it = await api.viewerItem(entry.id);
      if (cancelled) return;
      item = it;
      star.bind(it.id, it.starred);
      membership.bind(it.id, it.albums);
      tags.bind(it.id, it.tags);
      // A video plays through its own element below, or shows its poster with a message when
      // it can't; both are read reactively from `videoIssue`/`videoFullSrc`, not decided
      // here, and neither says anything about the Failed check that follows, so this skips
      // it - and the still-image preload - entirely rather than running either.
      if (it.kind === 'video') {
        return; // no neighbour preload from a video, and nothing to decode
      }
      if (it.thumbState === 'failed') {
        error = it.thumbError ?? "This photo can't be shown.";
        return;
      }
      // An edited photo's URL carries its key: the path does not change with the edit, and
      // an `<img>` handed the URL it already has shows the picture it already has. The
      // untouched photo keeps the bare URL the neighbour preload below warms.
      const url = mediaUrl(`image/${it.id}`) + (it.edit ? `?k=${it.thumbKey}` : '');
      const full = new Image();
      full.src = url;
      full.decode().then(
        () => {
          if (!cancelled) fullSrc = url;
        },
        () => {},
      );
      const near = await api.neighbours(it.id, PRELOAD_RADIUS);
      if (cancelled) return;
      for (const id of near) new Image().src = mediaUrl(`image/${id}`);
    })().catch((e) => {
      if (!cancelled) error = errorMessage(e);
    });
    return () => {
      cancelled = true;
      // Leaving a video - navigation, close, or a reload of the same offset - must release
      // its decode pipeline rather than leave it running behind a photo or an unmounted
      // element: pause first (a `load()` alone can keep playing until it resets), drop the
      // source so nothing is left to buffer, then load() to actually abandon it.
      videoEl?.pause();
      videoEl?.removeAttribute('src');
      videoEl?.load();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    // A drag along the video's position bar owns the keyboard while it lasts: Escape puts the
    // video back where the drag began, and nothing else may navigate away from under it.
    if (player.scrubbing) {
      if (e.key === 'Escape') {
        e.preventDefault();
        player.abandonScrub();
      }
      return;
    }
    // An open menu takes Escape first, the way any menu does; the viewer is next.
    if (e.key === 'Escape' && menu) {
      e.preventDefault();
      closeMenu();
      return;
    }
    // The crop tool owns the keyboard while it is open: Enter applies, Escape cancels, and
    // nothing else may navigate away from the photo under the rectangle.
    if (crop.active) {
      if (e.key === 'Escape') {
        e.preventDefault();
        crop.cancel();
      } else if (e.key === 'Enter' && !(e.target instanceof HTMLSelectElement)) {
        e.preventDefault();
        applyCrop();
      }
      return;
    }
    // Escape leaves the slideshow first and the viewer second, so the photo the show
    // stopped on is still there to look at.
    if (e.key === 'Escape' && slideshow.active) {
      e.preventDefault();
      stopSlideshow();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    // While the zoom slider has focus the arrow keys belong to it, which is how a range
    // input is expected to behave. Navigation stays available everywhere else.
    if (e.target instanceof HTMLInputElement) return;
    // Ctrl+C / Cmd+C copies the photo on screen - after the input guard, so a caption or
    // keyword field keeps its own copy, and never while text is selected (isCopyPhotoShortcut).
    // The backend refuses to copy a video, so the shortcut is simply dead over one.
    if (item && !isVideo && isCopyPhotoShortcut(e, (window.getSelection()?.toString() ?? '') !== '')) {
      e.preventDefault();
      void library.copyPhoto(item.id);
      return;
    }
    // Backspace closes as well, like the back button. Deliberately placed here: after the
    // input guard, so a text field gets its character deleted rather than the viewer
    // slammed shut; but before the empty-library check below, so it still closes when a
    // scan has emptied the grid underneath an open viewer — the case where the viewer
    // shows "This photo is no longer available" and Escape must still work.
    if (e.key === 'Backspace') {
      e.preventDefault();
      close();
      return;
    }
    // A playing video answers Space, L (loop), M (mute) and Shift+←/→ (seek); plain arrows
    // fall through to move between items as they always have. Not during a slideshow, which
    // never stops on a video and keeps Space for itself.
    if (isVideo && videoFullSrc && !slideshow.active && player.handleKey(e)) {
      e.preventDefault();
      return;
    }
    // Plain letters only: a modifier means the key belongs to the webview or the OS.
    if (!e.ctrlKey && !e.metaKey && !e.altKey) {
      // A video has no turn or crop; swallow the keys rather than let them fall through to
      // whatever else a plain letter might do below.
      if (isVideo && ['r', 'R', 'c', 'C'].includes(e.key)) {
        e.preventDefault();
        return;
      }
      if (e.key === 'r' || e.key === 'R') {
        e.preventDefault();
        rotate(e.key === 'r' ? 'cw' : 'ccw');
        return;
      }
      if (e.key === 'c' || e.key === 'C') {
        e.preventDefault();
        startCrop();
        return;
      }
      if (e.key === 'h' || e.key === 'H') {
        // Hide or unhide the photo on screen, as the right-click menu does. It stays on
        // screen (see `toggleHidden`), so a second H undoes the first.
        e.preventDefault();
        toggleHidden();
        return;
      }
      if (e.key === 's' || e.key === 'S') {
        e.preventDefault();
        if (slideshow.active) stopSlideshow();
        else startSlideshow();
        return;
      }
      if (e.key === ' ' && slideshow.active) {
        e.preventDefault();
        slideshow.toggle();
        return;
      }
      if (e.key === 'i' || e.key === 'I') {
        e.preventDefault();
        info = !info;
        return;
      }
    }
    const last = library.info.len - 1;
    if (last < 0) return;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      step(e.key === 'ArrowLeft' ? -1 : 1);
      return;
    }
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? last : null;
    if (next !== null) {
      e.preventDefault();
      goto(next);
    }
  }

  /** The mouse's back button closes the viewer, like Escape, Backspace and the ✕.
   *
   *  `mousedown`, not `pointerdown`: on Linux WebKitGTK never sees the back button, because
   *  wry swallows GDK button 8 and dispatches a synthetic `mousedown`/`mouseup` with button 3
   *  in its place (wry-0.55.1/src/webkitgtk/synthetic_mouse_events.rs). No pointer event is
   *  ever fired for it, so a `pointerdown` listener works on Windows and macOS only.
   *
   *  On the window rather than the viewer element so a press anywhere counts, including on
   *  the zoom slider. `preventDefault` stops the webview treating it as history navigation;
   *  there is nowhere to go back to, but the press would otherwise be handled twice. The pan
   *  handler below is unaffected — it already ignores every button but the left one. */
  function onbackbutton(e: MouseEvent) {
    if (!closesViewer(e.button)) return;
    e.preventDefault();
    close();
  }

  function onwheel(e: WheelEvent) {
    e.preventDefault();
    // The wheel navigates between photos here, not zoom - zoom is the slider alone - and the
    // spec drops only zoom, pan and crop for a video, so the wheel keeps moving between
    // photos over one exactly as it does over a photo.
    if (crop.active) return;
    const stepped = wheelStep(wheelTotal, e.deltaY);
    wheelTotal = stepped.accumulated;
    if (stepped.step === 0) return;
    if (slideshow.active) step(stepped.step > 0 ? 1 : -1);
    else goto(current + stepped.step);
  }

  function onzoom(e: Event & { currentTarget: HTMLInputElement }) {
    if (isVideo) return;
    zoom = clampZoom(Number(e.currentTarget.value));
    const { width, height } = viewport();
    // Zooming back out shrinks how far the photo may travel, so a pan that was legal at 4x
    // has to be pulled back in rather than left hanging off the edge.
    pan = clampPan(pan.x, pan.y, zoom, width, height);
  }

  function onpointerdown(e: PointerEvent) {
    if (isVideo) return;
    // A press anywhere but the info panel clears a text selection left in it: a click does
    // not, and the next Ctrl+C would copy that text instead of the photo, silently.
    if (!(e.target as HTMLElement).closest('.info')) window.getSelection()?.removeAllRanges();
    // The zoom slider, the buttons and the info panel sit on the same surface: a press on
    // any of them is theirs, not the start of a pan.
    if ((e.target as HTMLElement).closest('.zoom, .close, .bar, .info')) return;
    // Left button only. Without this every button panned, which is why the right button
    // looked like the pan control: the left one was being swallowed by the browser's native
    // image drag before the pointer stream could produce a move.
    if (e.button !== 0) return;
    if (zoom === MIN_ZOOM) return;
    dragging = true;
    dragFrom = { x: e.clientX, y: e.clientY, panX: pan.x, panY: pan.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    slideshow.poke();
    if (!dragging) return;
    const { width, height } = viewport();
    pan = clampPan(
      dragFrom.panX + (e.clientX - dragFrom.x),
      dragFrom.panY + (e.clientY - dragFrom.y),
      zoom,
      width,
      height,
    );
  }

  function onpointerup(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
  }
</script>

<svelte:window {onkeydown} onmousedown={onbackbutton} onclick={closeMenu} />

<!-- The pan handlers live here rather than on the stage below: this element already carries
     a role, and dragging anywhere in the viewer is easier to hit than the photo alone. -->
<!-- Dark in both themes, so a photo is always judged against the same ground. tokens.css's
     theme blocks match any element, so this subtree resolves the dark tokens. -->
<div
  class="viewer focus-container"
  class:quiet={slideshow.idle}
  data-theme="dark"
  role="dialog"
  aria-modal="true"
  aria-label="Photo viewer"
  tabindex="-1"
  {onwheel}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  {oncontextmenu}
>
  {#if error}
    <p class="error">{error}</p>
  {:else if item}
    <div
      class="stage"
      class:grabbable={zoom > MIN_ZOOM}
      class:grabbing={dragging}
      style="transform: translate({pan.x}px, {pan.y}px) scale({zoom})"
      bind:this={stage}
    >
      <!-- The frame is the viewport's size; the photo is `contain`-fitted inside it. Turns
           and crops are not drawn here - the backend renders them into the images. -->
      <div class="frame" bind:clientWidth={frameW} bind:clientHeight={frameH}>
        {#if crop.active && cropBox}
          <!-- The whole turned picture, with the rectangle on it. The box is the picture's
               own, so the rectangle's fractions are percentages of it and the dimming
               shadow is clipped to the photo rather than spilling over the black. -->
          <img class="full" src={mediaUrl(`image/${item.id}/uncropped`) + `?k=${item.thumbKey}`} alt={item.fileName} draggable="false" />
          <div
            class="crop-area"
            style:left="{cropBox.left}px"
            style:top="{cropBox.top}px"
            style:width="{cropBox.width}px"
            style:height="{cropBox.height}px"
          >
            <div
              class="crop-rect"
              role="presentation"
              style:left="{crop.rect.left * 100}%"
              style:top="{crop.rect.top * 100}%"
              style:width="{(crop.rect.right - crop.rect.left) * 100}%"
              style:height="{(crop.rect.bottom - crop.rect.top) * 100}%"
              onpointerdown={(e) => cropPointerDown(e, 'move')}
              onpointermove={cropPointerMove}
              onpointerup={() => crop.endDrag()}
              onpointercancel={() => crop.endDrag()}
            >
              {#each HANDLES as handle (handle)}
                <div
                  class="crop-handle {handle}"
                  role="presentation"
                  onpointerdown={(e) => cropPointerDown(e, handle)}
                  onpointermove={cropPointerMove}
                  onpointerup={() => crop.endDrag()}
                  onpointercancel={() => crop.endDrag()}
                ></div>
              {/each}
            </div>
          </div>
        {:else if isVideo}
          {#if videoIssue}
            <!-- No picture to play, so the poster - the same frame a tile shows - stands in,
                 with the reason lying over it like the face plate does, rather than an empty
                 stage with nothing but text. -->
            <img class="full" src={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)} alt="" draggable="false" />
            <div class="video-issue">
              <p>{videoIssueMessage}</p>
            </div>
          {:else}
            <!-- Plays on open, with sound, as Picasa did. No `controls`: photon draws its own
                 (`VideoControls`), which look the same in all three webviews; a click on the
                 picture plays and pauses. `tabindex="-1"` only takes it out of the tab order -
                 the viewer's own keys live on `<svelte:window>` and work over it regardless. -->
            <!-- A Failed video that is not a crash is offered but not started: its failure was
                 the poster frame's (a decode error, a timeout), which says the platform
                 struggled with the file, so it waits for the user to ask. An error while
                 playing swaps this element for the poster and a message (`playbackFailed`),
                 rather than leave a black player; the element that fired it is checked, as
                 one being unloaded on the way out is no longer the one on screen. -->
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              class="full"
              src={videoFullSrc}
              poster={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)}
              autoplay={item.thumbState !== 'failed'}
              preload="metadata"
              crossorigin="anonymous"
              tabindex="-1"
              bind:this={videoEl}
              onclick={() => player.toggle()}
              onerror={(e) => {
                if (e.currentTarget === videoEl && videoFullSrc) playbackFailed = true;
              }}
            ></video>
          {/if}
        {:else}
          <img class="preview" src={mediaUrl(`thumb/${item.id}/preview/${item.thumbKey}`)} alt="" draggable="false" class:hidden={!!fullSrc} />
          {#if fullSrc}
            <img class="full" src={fullSrc} alt={item.fileName} draggable="false" />
          {/if}
        {/if}
        {#each faceBoxes as face, i (i)}
          <div
            class="face"
            style:left="{face.box.left}px"
            style:top="{face.box.top}px"
            style:width="{face.box.width}px"
            style:height="{face.box.height}px"
          >
            <span class="face-name">{face.name}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
  {#if outgoing}
    <!-- Keyed, so each photo leaving gets an element of its own: reusing one would fade the
         next outgoing photo *in* from the opacity the last one ended on. -->
    {#key outgoing.src}
      <img class="outgoing" class:fading={outgoing.fading} src={outgoing.src} alt="" draggable="false" />
    {/key}
  {/if}
  {#if info && item}
    <aside class="info" aria-label="Photo information">
      <h2 class="info-title">{item.fileName}</h2>
      <p class="info-path" title={item.path}>{item.path}</p>
      {#if item.caption?.trim()}
        <h3>Caption</h3>
        <p class="info-caption">{item.caption.trim()}</p>
      {/if}
      {#if camera.length || (item.kind === 'video' && item.durationMs !== null)}
        <dl>
          {#each camera as row (row.label)}
            <dt>{row.label}</dt>
            <dd>
              {#if row.search}
                {@const query = row.search}
                <button class="info-link" onclick={() => onsearch(query)} title="Show every photo with this {row.label.toLowerCase()}">{row.value}</button>
              {:else}
                {row.value}
              {/if}
            </dd>
          {/each}
          {#if item.kind === 'video' && item.durationMs !== null}
            <dt>Length</dt>
            <dd>{formatDuration(item.durationMs)}</dd>
          {/if}
        </dl>
      {:else}
        <p class="info-muted">No camera data.</p>
      {/if}
      <h3>People</h3>
      {#if item.faces.length}
        <ul class="chips">
          {#each item.faces as face, i (i)}
            <li>{face.name}</li>
          {/each}
        </ul>
      {:else}
        <p class="info-muted">No faces named in Picasa.</p>
      {/if}
      <h3>Keywords</h3>
      {#if tags.list.length}
        <ul class="chips">
          {#each tags.list as tag (tag)}
            <li>
              {tag}
              <button
                type="button"
                class="chip-remove"
                aria-label="Remove {tag}"
                disabled={tags.busy(tag)}
                onclick={() => removeTag(tag)}><Icon name="x" size={10} /></button
              >
            </li>
          {/each}
        </ul>
      {:else}
        <p class="info-muted">No keywords yet.</p>
      {/if}
      <input
        class="tag-input"
        list="tag-suggestions"
        placeholder="Add a keyword"
        bind:value={tags.draft}
        onkeydown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            addTag();
          }
        }}
      />
      <datalist id="tag-suggestions">
        {#each tags.suggestions(library.tags.map((t) => t.tag)) as name (name)}
          <option value={name}></option>
        {/each}
      </datalist>
      <h3>Albums</h3>
      {#if ownAlbumList.length}
        <ul class="albums">
          {#each ownAlbumList as album (album.id)}
            <li>
              <label>
                <input
                  type="checkbox"
                  checked={membership.has(album.id)}
                  disabled={membership.busy(album.id)}
                  onchange={() => toggleAlbum(album.id)}
                />
                {album.name}
              </label>
            </li>
          {/each}
        </ul>
      {:else if !picasaHere.length}
        <p class="info-muted">No albums of your own yet. Create one in the sidebar.</p>
      {/if}
      {#if picasaHere.length}
        <ul class="albums picasa-albums">
          {#each picasaHere as album (album.id)}
            <li title="From Picasa. Change it in Picasa."><Icon name="images" size={12} />{album.name}</li>
          {/each}
        </ul>
      {/if}
      {#each copies as group (group.kind)}
        <!-- Absent rather than "none": nearly every photo has no copy, and the panel is
             long enough. A click locates the copy in the grid, as the menu's Locate does.
             Dimensions are shown only for a look-alike: an identical copy has the same
             dimensions by definition, but for a look-alike "which one is the big one?" is
             almost always the next question. -->
        <h3>{group.label}</h3>
        <ul class="copies">
          {#each group.copies as copy (copy.id)}
            <li>
              <button class="info-link" onclick={() => onlocate(copy.id)} title="Locate in photon">{copy.path}</button>
              {#if copy.kind === 'similar'}
                <span class="info-muted">{formatDimensions(copy.width, copy.height)}</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/each}
      {#if item && item.copies.length > 0}
        <!-- The same count and words as the tile menu's item, so the two read as one way in.
             The list above locates one copy at a time; this shows them all together. -->
        {@const id = item.id}
        <p class="all-copies">
          <button class="info-link" onclick={() => onshowcopies(id)}>{showCopiesLabel(item.copies.length)} in the grid</button>
        </p>
      {/if}
    </aside>
  {/if}
  <!-- The photo's own caption, not the file-name line in the bar. A sibling of .bar, not
       inside it: the slideshow's quiet state fades the bar, and the caption is what a
       slideshow is watched for. Hidden while cropping, when the space is the crop tool's,
       and hidden while the info panel is open: the panel shows the caption in full at its
       top, and the strip would otherwise cross the panel. -->
  {#if captionLine && !crop.active && !info}
    <p class="photo-caption" title={item?.caption ?? ''}>{captionLine}</p>
  {/if}
  <!-- The star and the caption share one bottom-centred row, so the star sits where the
       eye already is for the file name rather than in a corner on its own. -->
  {#if crop.active}
    <div class="bar">
      <select class="aspect" aria-label="Crop ratio" value={crop.aspect} onchange={(e) => crop.setAspect(Number(e.currentTarget.value))}>
        {#each ASPECTS as aspect, i (aspect.label)}
          <option value={i}>{aspect.label}</option>
        {/each}
      </select>
      <button class="tool wide" onclick={() => crop.clear()} title="Select the whole photo, which removes the crop">Whole photo</button>
      <button class="tool wide" onclick={() => crop.cancel()} title="Cancel (Esc)">Cancel</button>
      <button class="tool wide primary" onclick={applyCrop} title="Apply (Enter)">Apply</button>
    </div>
  {:else}
  {#if isVideo && videoFullSrc}
    <VideoControls {player} />
  {/if}
  <div class="bar">
    <button
      class="star"
      onclick={toggleStar}
      disabled={!item || star.busy}
      aria-pressed={star.starred}
      aria-label={star.starred ? 'Unstar' : 'Star'}
      title={star.starred ? 'Unstar' : 'Star'}
    >
      <Icon name="star" size={16} filled={star.starred} />
    </button>
    {#if !isVideo}
      <span class="sep" aria-hidden="true"></span>
      <button class="tool" onclick={() => rotate('ccw')} disabled={!editable} aria-label="Rotate left" title="Rotate left (Shift+R)"><Icon name="rotate-ccw" size={16} /></button>
      <button class="tool" onclick={() => rotate('cw')} disabled={!editable} aria-label="Rotate right" title="Rotate right (R)"><Icon name="rotate-cw" size={16} /></button>
      <button class="tool" onclick={startCrop} disabled={!editable} aria-label="Crop" title="Crop (C)"><Icon name="crop" size={16} /></button>
      {#if item?.edit}
        <button class="tool wide" onclick={resetEdit} disabled={!editable} title="Undo every turn and crop. The file was never changed.">Original</button>
      {/if}
    {/if}
    <span class="sep" aria-hidden="true"></span>
    <button
      class="tool"
      onclick={() => (slideshow.active ? slideshow.toggle() : startSlideshow())}
      disabled={!item && !slideshow.active}
      aria-label={!slideshow.active ? 'Start slideshow' : slideshow.playing ? 'Pause slideshow' : 'Resume slideshow'}
      title={!slideshow.active ? 'Slideshow (S)' : slideshow.playing ? 'Pause (Space)' : 'Resume (Space)'}
    >
      <Icon name={slideshow.active && slideshow.playing ? 'pause' : 'play'} size={16} />
    </button>
    <button
      class="tool"
      onclick={() => (info = !info)}
      disabled={!item}
      aria-pressed={info}
      aria-label="Photo information"
      title="Photo information (I)"
    >
      <Icon name="info" size={16} />
    </button>
    <span class="sep" aria-hidden="true"></span>
    <!-- A button, because a click copies the file name. The confirmation replaces the whole
         line for a moment rather than appending to it, so the line does not jump in width. -->
    <button class="caption" onclick={copyName} disabled={!item} title="Click to copy the file name">
      {copy.copied ? 'Copied' : caption}
    </button>
  </div>
  {/if}
  {#if menu && item}
    <div
      class="menu focus-container"
      role="menu"
      tabindex="-1"
      bind:this={menuEl}
      style:left="{menu.x}px"
      style:top="{menu.y}px"
    >
      <button role="menuitem" onclick={locate}>Locate in photon</button>
      <button role="menuitem" onclick={reveal}>Reveal in file manager</button>
      <button role="menuitem" title="Opens the file itself; photon's turns and crop are not applied." onclick={openInApp}>Open in default app</button>
      <button role="menuitem" onclick={toggleHidden}>{item.hidden ? 'Unhide photo (H)' : 'Hide photo (H)'}</button>
    </div>
  {/if}
  {#if !isVideo}
    <div class="zoom" class:hidden={crop.active}>
      <input
        type="range"
        min={MIN_ZOOM}
        max={MAX_ZOOM}
        step="0.05"
        value={zoom}
        oninput={onzoom}
        aria-label="Zoom"
      />
      <span class="level">{Math.round(zoom * 100)}%</span>
    </div>
  {/if}
  <button class="close" onclick={close} aria-label="Close viewer"><Icon name="x" size={16} /></button>
</div>

<style>
  /* #000 is the one colour literal in the UI: the ground a photo is judged against is not
     a theme decision. `color` is set here because it is inherited and was computed on :root,
     in the app's theme, before this subtree turned dark; app.css's `button { color: inherit }`
     is why menu buttons and labels depend on it. accent-color needs no override here:
     tokens.css declares it on `[data-theme]`, which this element already matches. */
  .viewer { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #000; overflow: hidden; color: var(--text); }
  .stage { position: absolute; inset: 0; transform-origin: center; will-change: transform; }
  .frame { position: absolute; left: 50%; top: 50%; width: 100vw; height: 100vh; translate: -50% -50%; }
  .face { position: absolute; border: 2px solid var(--photo-line); border-radius: var(--r-1); box-shadow: 0 0 0 1px var(--shadow-ink); pointer-events: none; }
  .face-name { position: absolute; left: -2px; top: 100%; margin-top: 2px; padding: 1px 6px; background: var(--scrim); border-radius: var(--r-1); color: var(--text); font-size: var(--t-2); white-space: nowrap; }
  .grabbable { cursor: grab; }
  .grabbing { cursor: grabbing; }
  /* `draggable="false"` covers the drag itself; these stop WebKit — which is the webview on
     both Linux and macOS — from starting its own image drag or selecting the image instead
     of panning. */
  img, video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; image-orientation: from-image; user-select: none; -webkit-user-drag: none; }
  /* The video stops above photon's own controls (`VideoControls`, at 64px) and the bar below
     them, so neither ever covers the picture - a subtitle burned in at the bottom edge, say.
     `height: auto` lets the bottom inset actually take effect: `inset: 0` above also sets an
     explicit height, and a positioned box honours only one of a competing height/top+bottom
     pair. */
  video.full { bottom: 112px; height: auto; }
  .hidden { visibility: hidden; }
  /* After the stage in the document and before the controls, so it paints between them
     without a z-index. The duration is `CROSSFADE_MS`. */
  .outgoing { opacity: 1; transition: opacity 600ms ease; pointer-events: none; }
  .outgoing.fading { opacity: 0; }
  /* A resting pointer during a slideshow: everything but the photo gets out of the way. */
  .quiet { cursor: none; }
  .quiet .bar, .quiet .zoom, .quiet .close { opacity: 0; pointer-events: none; }
  .bar, .zoom, .close { transition: opacity 200ms ease; }
  /* Glass: 90% opaque on its own, so it reads where backdrop-filter is slow or missing
     (some Linux GPUs); the blur is an enhancement on top. The opacity is set by contrast,
     not taste: dim text on it must still reach 4.5:1 over a white photo (tokens.test.ts). */
  .bar, .zoom, .close, .info, .photo-caption {
    background: var(--glass);
    box-shadow: 0 0 0 1px var(--glass-line), var(--shadow-menu);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
  }
  /* Above the bar, centred like it, and clear of the zoom control the same way. Two lines
     at most: a long caption must not climb over the photo; the info panel has it whole. */
  .photo-caption {
    position: absolute; bottom: 64px; left: 50%; transform: translateX(-50%);
    max-width: calc(100% - 428px); margin: 0; padding: var(--s-1) var(--s-3);
    border-radius: var(--r-3); color: var(--text); font-size: var(--t-2); text-align: center;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden;
  }
  /* The Camera section has no heading of its own, so unlike every other section - whose
     gap above comes from its h3's margin-top - the caption has to supply that spacing
     itself: the same var(--s-3) an h3 puts above the section that follows it. */
  .info-caption { margin: 0 0 var(--s-3); white-space: pre-line; }
  /* Centred, with the zoom control's side kept clear on BOTH sides so it stays centred:
     the control measures 194px at 12px from the edge, and a little air after it makes 214.
     Without this the bar simply grows through it - the caption's old cap bounded the overlap
     rather than removing it - and at the 800px minimum width the tools went under the
     slider. What is left is the caption's; it ellipsises into it. */
  .bar { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 2px; max-width: calc(100% - 428px); padding: var(--s-1); border-radius: var(--r-4); }
  .sep { width: 1px; height: 18px; margin: 0 var(--s-1); background: var(--glass-line); }
  /* The file name is the only part of the toolbar that can be any length, so it is the part
     that gives way: `min-width: 0` is what lets a flex item shrink below its content and
     ellipsise, and the tools keep their intrinsic width. */
  .caption { flex: 0 1 auto; min-width: 0; padding: 0 10px; border: 0; background: none; color: var(--text-dim); font-size: var(--t-2); white-space: nowrap; cursor: pointer; overflow: hidden; text-overflow: ellipsis; }
  .caption:hover:not(:disabled) { color: var(--text); }
  .caption:disabled { cursor: default; }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button { padding: 6px 10px; border: 0; border-radius: var(--r-2); background: none; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--hover); }
  .zoom { position: absolute; bottom: 12px; right: 12px; display: flex; align-items: center; gap: var(--s-2); padding: 6px var(--s-3); border-radius: var(--r-4); }
  .zoom input { width: 120px; }
  .level { color: var(--text-dim); font-size: var(--t-2); min-width: 38px; text-align: right; font-variant-numeric: tabular-nums; }
  .close { position: absolute; top: 12px; right: 12px; display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 0; border-radius: 50%; color: var(--text-dim); cursor: pointer; }
  .close:hover { color: var(--text); }
  .star, .tool { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: var(--r-3); background: none; color: var(--text-dim); font-size: var(--t-2); line-height: 1; cursor: pointer; transition: background-color 120ms ease-out; }
  .star:hover:not(:disabled), .tool:hover:not(:disabled) { color: var(--text); background: var(--hover); }
  /* Spelled out with :hover, like .tool.primary below: the generic hover rule otherwise
     out-specifies these, so a starred star or a pressed tool loses its colour under the
     pointer - exactly while it is, right after the click that set aria-pressed. */
  .star[aria-pressed='true'], .star[aria-pressed='true']:hover:not(:disabled) { color: var(--star); }
  .tool[aria-pressed='true'], .tool[aria-pressed='true']:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  .star:disabled, .tool:disabled { cursor: default; opacity: 0.4; }
  .tool.wide { width: auto; padding: 0 var(--s-3); color: var(--text); }
  .tool.primary, .tool.primary:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  .aspect { height: 30px; padding: 0 var(--s-2); border: 0; border-radius: var(--r-3); background: var(--field); color: var(--text); font: inherit; font-size: var(--t-2); }
  @media (prefers-reduced-motion: reduce) { .star, .tool { transition: none; } }
  /* The crop rectangle. The shadow is the dimming: one element, clipped by the area to the
     photo's own box. Handles are larger than they look, so they can be caught. */
  .crop-area { position: absolute; overflow: hidden; touch-action: none; }
  .crop-rect { position: absolute; box-sizing: border-box; border: 1px solid var(--photo-line); box-shadow: 0 0 0 9999px var(--scrim); cursor: move; }
  .crop-handle { position: absolute; width: 22px; height: 22px; }
  .crop-handle::after { content: ''; position: absolute; inset: 7px; background: var(--photo-line); border-radius: 1px; box-shadow: 0 0 0 1px var(--shadow-ink); }
  .crop-handle.n, .crop-handle.s { left: 50%; margin-left: -11px; cursor: ns-resize; }
  .crop-handle.e, .crop-handle.w { top: 50%; margin-top: -11px; cursor: ew-resize; }
  .crop-handle.n, .crop-handle.ne, .crop-handle.nw { top: -11px; }
  .crop-handle.s, .crop-handle.se, .crop-handle.sw { bottom: -11px; }
  .crop-handle.w, .crop-handle.nw, .crop-handle.sw { left: -11px; }
  .crop-handle.e, .crop-handle.ne, .crop-handle.se { right: -11px; }
  .crop-handle.nw, .crop-handle.se { cursor: nwse-resize; }
  .crop-handle.ne, .crop-handle.sw { cursor: nesw-resize; }
  .error { color: var(--text-dim); }
  /* Lies over the poster the way the face plate lies over a photo: the same glass and scrim
     technique as `.bar`, so the reason reads over a bright frame as well as a dark one. */
  .video-issue {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: var(--s-4);
    background: var(--scrim);
  }
  .video-issue p { max-width: 32em; margin: 0; padding: var(--s-2) var(--s-3); border-radius: var(--r-3); background: var(--glass); box-shadow: 0 0 0 1px var(--glass-line); color: var(--text); font-size: var(--t-3); text-align: center; }
  .info {
    position: absolute;
    top: 12px;
    right: 56px;
    bottom: 56px;
    width: 280px;
    overflow: auto;
    padding: 14px var(--s-4);
    border-radius: var(--r-4);
    color: var(--text);
    font-size: var(--t-3);
    user-select: text;
  }
  .info-title { margin: 0 0 2px; font-size: var(--t-4); font-weight: 600; overflow-wrap: anywhere; }
  .info-path { margin: 0 0 10px; color: var(--text-dim); font-size: var(--t-1); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .info h3 { margin: var(--s-3) 0 var(--s-1); color: var(--text-dim); font-size: var(--t-1); font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .info dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 10px; margin: 0; }
  .info dt { color: var(--text-dim); }
  .info dd { margin: 0; overflow-wrap: anywhere; }
  .info-link { padding: 0; border: 0; background: none; color: var(--accent-glass); font: inherit; text-align: left; cursor: pointer; overflow-wrap: anywhere; }
  .info-link:hover { text-decoration: underline; }
  .info-muted { margin: 0; color: var(--text-dim); }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .chips li { display: inline-flex; align-items: center; padding: 2px var(--s-2); background: var(--field); border-radius: 999px; font-size: var(--t-2); }
  .albums { margin: 0; padding: 0; list-style: none; }
  .copies { margin: 0; padding: 0; list-style: none; font-size: var(--t-2); }
  .copies li { display: flex; align-items: baseline; gap: 6px; padding: 2px 0; }
  .all-copies { margin: var(--s-2) 0 0; font-size: var(--t-2); }
  .albums label { display: flex; align-items: center; gap: 8px; padding: 2px 0; cursor: pointer; }
  .picasa-albums li { display: flex; align-items: center; gap: 8px; padding: 2px 0; color: var(--text-dim); }
  .chip-remove {
    display: grid;
    place-items: center;
    margin-left: var(--s-1);
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    opacity: 0.6;
  }
  .chip-remove:hover:not(:disabled) { opacity: 1; }
  .chip-remove:disabled { cursor: default; opacity: 0.3; }
  /* A hairline on the bare glass rather than the usual --field film. --field is white at 7%,
     so on glass it lightens the ground and takes the placeholder's dim text to 3.71; the
     glass itself keeps it at 4.61 (lib/tokens.test.ts). */
  .tag-input { width: 100%; margin-top: 6px; box-sizing: border-box; padding: 5px var(--s-2); border: 0; border-radius: var(--r-2); background: none; box-shadow: inset 0 0 0 1px var(--glass-line); color: var(--text); font: inherit; }
  .tag-input::placeholder { color: var(--text-dim); }
</style>
