// A pretend Tauri backend, so the built UI can be rendered in a plain headless browser by
// `cargo run -p xtask -- screenshots`. Served as /mock.js and loaded before theme-boot.js.
//
// The query string drives it: ?theme=system|light|dark is what the `theme` command answers,
// ?view=<GridView> the grid's view, and ?do=<action> what to click once the app has settled.
//
// Two lists below are read by a test in screenshots.rs, which fails when api.ts gains a
// command that is in neither: `canned` (keys at four spaces' indent) and SILENT.
(function () {
  // The mock has no real video file to serve, so a `<video src>` here always fails to load,
  // and which state the viewer shows would otherwise depend on whether *this* Chromium claims
  // MP4 support. Both states worth seeing are forced instead, so every machine renders the
  // same shots: `?do=videoplay` claims support and swallows the load error - the error event
  // is stopped on its way down, in the capture phase at the window, before the viewer's own
  // listener would swap the player for a message - which leaves the player and photon's own
  // controls on screen over the poster; every other shot denies support, so `?do=video` shows
  // the "can't be played here" message.
  const PLAYABLE = new URLSearchParams(location.search).get('do') === 'videoplay';
  HTMLMediaElement.prototype.canPlayType = () => (PLAYABLE ? 'maybe' : '');
  if (PLAYABLE) {
    window.addEventListener(
      'error',
      (e) => {
        if (e.target instanceof HTMLMediaElement) e.stopImmediatePropagation();
      },
      true,
    );
  }

  const P = new URLSearchParams(location.search);
  const day = (y, m, d) => Date.UTC(y, m - 1, d) / 1000;

  const folders = [
    { id: 1, watchedId: 1, parentId: null, path: '/home/ada/Pictures', name: 'Pictures', hidden: false, alias: null },
    { id: 2, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2026/Summer hike', name: 'Summer hike', hidden: false, alias: 'Up the Hohe Tauern' },
    { id: 3, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2026/Birthday', name: 'Birthday', hidden: false, alias: null },
    { id: 4, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2025/Christmas', name: 'Christmas', hidden: false, alias: null },
    { id: 5, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2025/Lisbon', name: 'Lisbon', hidden: false, alias: 'Lisbon with the Silvas' },
  ];
  const sections = [
    { folderId: 2, offset: 0, count: 23, takenAtMin: day(2026, 7, 14) },
    { folderId: 3, offset: 23, count: 11, takenAtMin: day(2026, 3, 2) },
    { folderId: 4, offset: 34, count: 17, takenAtMin: day(2025, 12, 24) },
    { folderId: 5, offset: 51, count: 40, takenAtMin: day(2025, 5, 9) },
  ];
  let len = 91;

  const HUGE = Number(P.get('huge')) || 0;
  const HUGE_FOLDERS = Number(P.get('folders')) || 1;
  if (HUGE > 0) {
    // A library past every engine's layout cap (`scroll-probe`): HUGE photos in HUGE_FOLDERS
    // folders of equal size, newest first, one folder a day.
    const per = Math.ceil(HUGE / HUGE_FOLDERS);
    folders.length = 0;
    sections.length = 0;
    for (let f = 0; f < HUGE_FOLDERS; f++) {
      const id = 100 + f;
      folders.push({ id, watchedId: 1, parentId: null, path: `/p/${f}`, name: `Folder ${f}`, hidden: false, alias: null });
      sections.push({ folderId: id, offset: f * per, count: Math.min(per, HUGE - f * per), takenAtMin: day(2026, 1, 1) - f * 86_400 });
    }
    len = HUGE;
  }

  function sectionOf(i) {
    let lo = 0;
    let hi = sections.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (sections[mid].offset <= i) lo = mid;
      else hi = mid - 1;
    }
    return sections[lo];
  }

  function entry(i) {
    const section = sectionOf(i);
    return {
      id: i + 1,
      folderId: section.folderId,
      takenAt: section.takenAtMin + i * 600,
      aspect: [1.5, 0.67, 1.33, 1][i % 4],
      kind: i % 9 === 4 ? 'video' : 'image',
      durationMs: i % 9 === 4 ? 83_000 : null,
      thumbKey: 'k' + i,
      starred: i % 7 === 0,
      // A different stride from the star, so some tiles carry each mark and some both.
      hasCopies: i % 5 === 2,
    };
  }

  function viewerItem(id) {
    // Varies by id so a compare screenshot with several panes actually shows
    // `differingFacts` at work - a fixed size/time made every prior compare shot render the
    // always-blank case, the one `differingFacts` never gets exercised by a screenshot.
    const width = id % 2 === 0 ? 5472 : 4032;
    const height = id % 2 === 0 ? 3648 : 3024;
    return {
      id,
      path: `/home/ada/Pictures/2026/Summer hike/IMG_48${id}.jpg`,
      fileName: `IMG_48${id}.jpg`,
      width,
      height,
      orientation: 1,
      takenAt: day(2026, 7, 14) + 70000 + id * 3600,
      size: 8123456,
      thumbKey: 'k' + (id - 1),
      thumbState: 'ready',
      thumbError: null,
      starred: true,
      hidden: false,
      make: 'Canon',
      model: 'Canon EOS R6',
      lens: 'RF24-70mm F2.8 L IS USM',
      focalMm: 35,
      aperture: 4,
      exposureS: 0.004,
      iso: 200,
      tags: ['alps', 'sunset'],
      caption:
        id === 3
          ? "Grandma's 80th, on the terrace in Lisbon, everyone gathered right before sunset for cake and the last of the summer light over the river."
          : null,
      faces: [{ hash: 'a', name: 'Anna', left: 0.3, top: 0.25, right: 0.42, bottom: 0.5 }],
      kind: id === 5 ? 'video' : 'image',
      durationMs: id === 5 ? 83_000 : null,
      videoCrashed: false,
      albums: [1, 3],
      copies: [
        { id: 501, path: `/home/ada/Pictures/2026/Summer hike/IMG_48${id} copy.jpg`, kind: 'identical', width: 5472, height: 3648 },
        { id: 502, path: `/home/ada/Pictures/2026/Summer hike/IMG_48${id} small.jpg`, kind: 'similar', width: 1600, height: 1067 },
      ],
      uncroppedWidth: width,
      uncroppedHeight: height,
      edit: null,
      // Taken and digitized agree, as a camera writes them, so the panel's merged row shows.
      dates: {
        taken: day(2026, 7, 14) + 70000 + id * 3600,
        digitized: day(2026, 7, 14) + 70000 + id * 3600,
        edited: day(2026, 7, 20) + 30000,
        fileCreatedMs: (day(2026, 7, 21) + 40000) * 1000,
        fileModifiedMs: (day(2026, 7, 21) + 40000) * 1000,
      },
      gps: { lat: 46.5388, lon: 12.1373 },
    };
  }

  // The query the box has sent, echoed back by grid_info the way the real backend does.
  // Without the echo the search box adopts the empty string the moment its send settles and
  // wipes what was typed - which is the box working correctly against a mock that was not.
  let searchQuery = '';
  // The sort the UI last set, echoed back the same way: the control reads it from grid_info.
  let sort = { key: 'date', reverse: false };

  // Commands whose answer the UI draws.
  //
  // The view setters (set_sort and set_search_query here, the rest in SILENT) answer with the
  // grid version that shows their view, and null means "no version: refetch". The mock's
  // version never moves from 1, which the store already holds, so any number would tell it
  // the grid it has is the new one and the echoed sort or query would never be read back.
  const canned = {
    list_folders: () => ({ watched: [{ id: 1, path: '/home/ada/Pictures', online: true }], folders }),
    set_sort: (a) => {
      sort = a.sort;
      return null;
    },
    grid_info: () => ({
      version: 1,
      len,
      // Always sent: a backend may, and the mock's layout never changes anyway.
      layout: {
        generation: 1,
        sections,
        folders: sections.map(({ folderId, count, takenAtMin }) => ({ folderId, count, takenAtMin, bytes: count * 4_000_000, modifiedMs: takenAtMin * 1000 })),
      },
      starredCount: 13,
      duplicateCount: 4,
      hiddenCount: 7,
      videoCount: 10,
      view: searchQuery === '' ? P.get('view') || 'all' : 'search',
      sort,
      searchQuery,
      person: null,
      album: null,
      tag: null,
      copiesOf: null,
      buildError: null,
    }),
    grid_rows: (a) => ({
      version: 1,
      rows: Array.from({ length: Math.max(0, Math.min(a.count, len - a.offset)) }, (_, k) => entry(a.offset + k)),
    }),
    grid_offset_of_item: (a) => a.itemId - 1,
    grid_offset_of_folder: () => 0,
    grid_folder_ids_at: () => null,
    neighbours: () => [],
    viewer_item: (a) => viewerItem(a.id ?? a.itemId ?? 3),
    list_people: () => [
      { hash: 'a', name: 'Anna', count: 212 },
      { hash: 'b', name: 'Jonas', count: 87 },
    ],
    list_tags: () => [
      { tag: 'alps', count: 134, total: 134 },
      { tag: 'family', count: 310, total: 310 },
      { tag: 'sunset', count: 41, total: 41 },
    ],
    list_tag_rules: () => [{ tag: 'Alpen', target: 'alps' }],
    list_albums: () => [
      { id: 1, name: 'Best of 2025', count: 96, picasa: false },
      { id: 3, name: 'Holiday 2009', count: 63, picasa: true },
      { id: 2, name: 'Lisbon', count: 48, picasa: false },
    ],
    list_saved_searches: () => [
      { id: 1, name: 'Canon, 2019', query: 'camera:canon 2019', createdMs: 0 },
      { id: 2, name: 'Lakes', query: 'lake OR pond', createdMs: 0 },
    ],
    // The bookmark button reports the row it made; nothing draws it, but a null would make
    // the store's refetch race an answer it cannot read.
    save_search: (args) => ({ id: 3, name: args.name, query: args.query, createdMs: 0 }),
    watched_folder_stats: () => [{ watchedId: 1, photoCount: 12480 }],
    app_info: () => ({ version: '0.0.0', libraryPath: '/home/ada/.local/share/photon/library.db', licence: 'MIT' }),
    library_stats: () => ({
      photos: 12034,
      videos: 310,
      bytes: 48.2 * 1024 ** 3,
      oldest: day(2004, 3, 2),
      newest: day(2026, 7, 14),
      years: [[2026, 1310], [2025, 2204], [2024, 1876], [2023, 960], [2019, 2950], [2012, 1480], [2004, 1564]].map(
        ([year, count]) => ({ year, count }),
      ),
      cameras: [
        { make: 'Canon', model: 'Canon EOS R6', count: 5120 },
        { make: 'Apple', model: 'iPhone 15 Pro', count: 3980 },
        { make: 'FUJIFILM', model: 'X100V', count: 1444 },
        { make: 'NIKON CORPORATION', model: 'NIKON D750', count: 890 },
      ],
      noCamera: 910,
      lenses: [
        { lens: 'RF24-70mm F2.8 L IS USM', count: 3010 },
        { lens: 'RF50mm F1.8 STM', count: 1620 },
        { lens: 'iPhone 15 Pro back triple camera 6.765mm f/1.78', count: 2890 },
      ],
    }),
    memory_usage: () => ({ bytes: 412_000_000, processes: 4, includesWebview: true }),
    last_folder: () => null,
    slideshow_interval: () => 4,
    slideshow_shuffle: () => false,
    similar_distance: () => 7,
    set_similar_distance: (a) => a.distance,
    set_search_query: (args) => {
      searchQuery = args.query || '';
      return null;
    },
    set_stars: (args) => (args.ids || []).length,
    set_items_hidden: (args) => (args.ids || []).length,
    set_folder_hidden: () => 1,
    set_folder_alias: () => true,
    // The keyword dialog reports what landed, so these answer rather than staying silent.
    add_items_tag: (args) => ({ tag: args.tag, count: (args.ids || []).length }),
    export_apply_edits: () => true,
    export_items: (args) => ({ written: (args.ids || []).length, failed: 0, reason: null }),
    remove_items_tag: (args) => ({ tag: args.tag, count: (args.ids || []).length }),
    theme: () => P.get('theme') || 'system',
    grid_tile: () => P.get('tile') || 'medium',
    set_grid_tile: () => null,
    copy_count: () => 2,
    media_base: () => 'http://127.0.0.1:9/0000',
    next_video_job: () => null,
  };

  // Commands that change something: a screenshot never needs their answer, so they get null.
  const SILENT = [
    'add_folder', 'add_item_tag', 'add_to_album', 'copy_photo', 'create_album', 'delete_album', 'hide_tag',
    'open_in_default_app', 'open_in_map',
    'remove_folder', 'remove_from_album', 'remove_item_tag', 'rename_album', 'rename_tag',
    'delete_saved_search', 'rename_saved_search',
    'rescan_folder', 'restore_tag_rule', 'reveal_folder', 'reveal_in_file_manager',
    'reveal_library', 'reveal_watched', 'rotate_item', 'set_album_view', 'set_grid_view',
    'set_item_edit', 'set_last_folder', 'set_person_view',
    'check_export_dest', 'set_export_apply_edits', 'set_slideshow_interval', 'set_slideshow_shuffle', 'set_star', 'set_tag_view', 'set_theme',
    'set_visible', 'set_copies_view',
    'put_video_frame', 'video_frame_failed', 'video_session_start',
  ];

  let callbacks = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback: (f) => {
      const id = ++callbacks;
      window['_' + id] = f;
      return id;
    },
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      // Window and event plugins: not fullscreen, and a listener id for every `listen`.
      if (cmd.startsWith('plugin:')) return cmd.includes('is_fullscreen') ? false : cmd.includes('listen') ? ++callbacks : null;
      if (canned[cmd]) return canned[cmd](args || {});
      if (!SILENT.includes(cmd)) console.warn('mock.js has no answer for', cmd);
      return null;
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };

  const click = (selector) => document.querySelector(selector)?.click();
  const tile = (n) => document.querySelectorAll('.tile')[n];
  const open = (n) => tile(n)?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  const later = (ms, f) => setTimeout(f, ms);

  function probeReport() {
    const v = document.querySelector('.viewport');
    // Only tiles overlapping the viewport's visible box count. A tile mounted but drawn off
    // screen proves nothing about reachability: with every row drawn at `row.top` instead of
    // `row.top - shift`, the last rows are mounted below the capped canvas where no scroll
    // can reach them, and counting every mounted tile passed that.
    const box = v?.getBoundingClientRect();
    const ids = [...document.querySelectorAll('.canvas img')]
      .filter((img) => {
        const r = img.getBoundingClientRect();
        return box !== undefined && r.bottom > box.top && r.top < box.bottom;
      })
      .map((img) => /thumb\/(\d+)\/grid\//.exec(img.getAttribute('src') ?? '')?.[1])
      .filter(Boolean)
      .map(Number);
    const cols = Math.max(0, ...[...document.querySelectorAll('.canvas .row')].map((r) => r.children.length));
    document.title =
      'PROBE ' +
      JSON.stringify({
        last: ids.length ? Math.max(...ids) - 1 : -1,
        cols,
        canvas: document.querySelector('.canvas')?.getBoundingClientRect().height ?? -1,
        scrollHeight: v?.scrollHeight ?? -1,
      });
  }

  // How long a probe action waits for its scroll to render before reading the result. A
  // library of 300,000 photos in 20,000 folders takes well past the ordinary shot's 400ms
  // to finish its first layout pass, so this is longer than the screenshots' own delays.
  const PROBE_DELAY = 3_000;

  // Waits for `.canvas`'s height to stop changing (three reads, 200ms apart, all equal)
  // before running `action`. A jump made while the canvas is still growing - grid_info,
  // then the folder sidebar, then the first page of rows, all take a while against 20,000
  // folders - lands against a shorter canvas than the final one and reads as stuck partway;
  // the browser never re-clamps a scrollTop back down when the content it was measured
  // against later grows underneath it.
  function whenSettled(action) {
    let lastHeight = null;
    let stableReads = 0;
    const check = () => {
      const height = document.querySelector('.canvas')?.getBoundingClientRect().height ?? null;
      if (height !== null && height === lastHeight) {
        stableReads += 1;
        if (stableReads >= 3) {
          action();
          return;
        }
      } else {
        stableReads = 0;
        lastHeight = height;
      }
      setTimeout(check, 200);
    };
    check();
  }

  const actions = {
    select: () => tile(7)?.click(),
    // `scroll-probe`: End through the grid's own key handling (a write from code), once the
    // layout has settled, then read what got mounted. The grid's `onscroll` is what turns a
    // written `scrollTop` into rendered rows, and headless Chromium under
    // `--virtual-time-budget` does not reliably fire the native scroll event for a
    // script-written `scrollTop` - `End`'s own internal `viewport.scrollTop = row.top` is no
    // exception - so this dispatches one itself; without it the DOM's scroll position moves
    // but the grid never hears about it and keeps rendering the top of the library.
    'probe-end': () =>
      whenSettled(() => {
        const v = document.querySelector('.viewport');
        v?.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
        v?.dispatchEvent(new Event('scroll'));
        later(PROBE_DELAY, probeReport);
      }),
    // A scrollbar-sized jump straight to the bottom of the DOM range, once the layout has
    // settled; the explicit scroll event is the same fix as `probe-end`'s.
    'probe-bottom': () =>
      whenSettled(() => {
        const v = document.querySelector('.viewport');
        if (v) {
          v.scrollTop = v.scrollHeight;
          v.dispatchEvent(new Event('scroll'));
        }
        later(PROBE_DELAY, probeReport);
      }),
    video: () => open(4),
    menu: () => {
      tile(7)?.click();
      tile(7)?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 700, clientY: 300 }));
    },
    // The sidebar's last row, where the menu has to open upward to stay on screen, and an
    // aliased folder (folder 5), so the menu holds Rename in photon… and Use folder name.
    foldermenu: () => {
      const row = [...document.querySelectorAll('nav .node')].find((b) => b.textContent.includes('Lisbon with the Silvas'));
      const box = row?.getBoundingClientRect();
      row?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: box?.left + 60, clientY: box?.top + 10 }));
    },
    band: () => {
      // A drag across the second and third rows of tiles, in three moves so the threshold
      // is passed and the band is drawn. A synthetic pointer is not one the browser knows,
      // so `setPointerCapture` throws for it - the grid reports that and carries on, which
      // is what lets this shot exercise the real path.
      const view = document.querySelector('.viewport');
      const send = (type, x, y) =>
        view.dispatchEvent(new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0, buttons: 1, pointerId: 1 }));
      send('pointerdown', 300, 250);
      send('pointermove', 320, 270);
      send('pointermove', 700, 480);
    },
    export: () => {
      actions.menu();
      later(100, () =>
        [...document.querySelectorAll('[role="menuitem"]')]
          .find((b) => b.textContent.trim().startsWith('Export'))
          ?.click(),
      );
    },
    keyword: () => {
      actions.menu();
      later(100, () =>
        [...document.querySelectorAll('[role="menuitem"]')]
          .find((b) => b.textContent.trim().startsWith('Add keyword'))
          ?.click(),
      );
    },
    // Types a query the canned list already holds, so the bookmark is drawn filled - the
    // state that says "saved", which is also the state that cannot be clicked.
    savedsearch: () => {
      const box = document.querySelector('input.search');
      if (!box) return;
      box.value = 'lake OR pond';
      box.dispatchEvent(new Event('input', { bubbles: true }));
    },
    // Three tiles selected with Ctrl-click (mirrors what a real multi-select looks like),
    // then the context menu's own Compare item - not a synthetic `C` keydown, so this
    // exercises the same path a person clicking through the menu takes.
    compare: () => {
      tile(3)?.click();
      tile(4)?.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
      tile(5)?.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }));
      tile(5)?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 700, clientY: 300 }));
      later(100, () =>
        [...document.querySelectorAll('[role="menuitem"]')]
          .find((b) => b.textContent.trim().startsWith('Compare'))
          ?.click(),
      );
    },
    viewer: () => open(2),
    videoplay: () => open(4),
    info: () => {
      open(2);
      later(500, () => click('button[aria-label="Photo information"]'));
    },
    crop: () => {
      open(2);
      later(500, () => click('button[aria-label="Crop"]'));
    },
    // photon's own dropdowns, open: the list is drawn by photon, so this is what it looks like.
    sortmenu: () => click('[role="combobox"][aria-label="Sort by"]'),
    cropmenu: () => {
      actions.crop();
      later(900, () => click('[role="combobox"][aria-label="Crop ratio"]'));
    },
    settings: () => click('button[aria-label="Settings"]'),
    appearance: () => settingsSection('Appearance'),
    about: () => settingsSection('About'),
    statistics: () => settingsSection('Statistics'),
  };

  function settingsSection(name) {
    click('button[aria-label="Settings"]');
    later(100, () =>
      [...document.querySelectorAll('nav[aria-label="Settings sections"] button')]
        .find((b) => b.textContent.trim() === name)
        ?.click(),
    );
  }

  // After the first grid page has rendered; the xtask gives the page a virtual-time budget
  // well past these delays.
  window.addEventListener('load', () => later(400, () => actions[P.get('do')]?.()));
})();
