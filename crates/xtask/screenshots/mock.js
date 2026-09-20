// A pretend Tauri backend, so the built UI can be rendered in a plain headless browser by
// `cargo run -p xtask -- screenshots`. Served as /mock.js and loaded before theme-boot.js.
//
// The query string drives it: ?theme=system|light|dark is what the `theme` command answers,
// ?view=<GridView> the grid's view, and ?do=<action> what to click once the app has settled.
//
// Two lists below are read by a test in screenshots.rs, which fails when api.ts gains a
// command that is in neither: `canned` (keys at four spaces' indent) and SILENT.
(function () {
  const P = new URLSearchParams(location.search);
  const day = (y, m, d) => Date.UTC(y, m - 1, d) / 1000;

  const folders = [
    { id: 1, watchedId: 1, parentId: null, path: '/home/ada/Pictures', name: 'Pictures' },
    { id: 2, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2026/Summer hike', name: 'Summer hike' },
    { id: 3, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2026/Birthday', name: 'Birthday' },
    { id: 4, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2025/Christmas', name: 'Christmas' },
    { id: 5, watchedId: 1, parentId: 1, path: '/home/ada/Pictures/2025/Lisbon', name: 'Lisbon' },
  ];
  const sections = [
    { folderId: 2, offset: 0, count: 23, takenAtMin: day(2026, 7, 14) },
    { folderId: 3, offset: 23, count: 11, takenAtMin: day(2026, 3, 2) },
    { folderId: 4, offset: 34, count: 17, takenAtMin: day(2025, 12, 24) },
    { folderId: 5, offset: 51, count: 40, takenAtMin: day(2025, 5, 9) },
  ];
  const len = 91;

  function entry(i) {
    const section = [...sections].reverse().find((s) => i >= s.offset);
    return {
      id: i + 1,
      folderId: section.folderId,
      takenAt: section.takenAtMin + i * 600,
      aspect: [1.5, 0.67, 1.33, 1][i % 4],
      kind: 'image',
      thumbKey: 'k' + i,
      starred: i % 7 === 0,
    };
  }

  function viewerItem(id) {
    return {
      id,
      path: `/home/ada/Pictures/2026/Summer hike/IMG_48${id}.jpg`,
      fileName: `IMG_48${id}.jpg`,
      width: 5472,
      height: 3648,
      orientation: 1,
      takenAt: day(2026, 7, 14) + 70000,
      size: 8123456,
      thumbKey: 'k' + (id - 1),
      thumbState: 'ready',
      thumbError: null,
      starred: true,
      make: 'Canon',
      model: 'Canon EOS R6',
      lens: 'RF24-70mm F2.8 L IS USM',
      focalMm: 35,
      aperture: 4,
      exposureS: 0.004,
      iso: 200,
      tags: ['alps', 'sunset'],
      faces: [{ hash: 'a', name: 'Anna', left: 0.3, top: 0.25, right: 0.42, bottom: 0.5 }],
      albums: [1],
      copies: [],
      uncroppedWidth: 5472,
      uncroppedHeight: 3648,
      edit: null,
    };
  }

  // Commands whose answer the UI draws.
  const canned = {
    list_folders: () => ({ watched: [{ id: 1, path: '/home/ada/Pictures', online: true }], folders }),
    grid_info: () => ({
      version: 1,
      len,
      sections,
      starredCount: 13,
      duplicateCount: 4,
      view: P.get('view') || 'all',
      searchQuery: '',
      person: null,
      album: null,
      tag: null,
    }),
    grid_rows: (a) => ({
      version: 1,
      rows: Array.from({ length: Math.max(0, Math.min(a.count, len - a.offset)) }, (_, k) => entry(a.offset + k)),
    }),
    grid_offset_of_item: (a) => a.itemId - 1,
    grid_offset_of_folder: () => 0,
    neighbours: () => [],
    viewer_item: (a) => viewerItem(a.id ?? a.itemId ?? 3),
    list_people: () => [
      { hash: 'a', name: 'Anna', count: 212 },
      { hash: 'b', name: 'Jonas', count: 87 },
    ],
    list_tags: () => [
      { tag: 'alps', count: 134 },
      { tag: 'family', count: 310 },
      { tag: 'sunset', count: 41 },
    ],
    list_tag_rules: () => [{ tag: 'Alpen', target: 'alps' }],
    list_albums: () => [
      { id: 1, name: 'Best of 2025', count: 96 },
      { id: 2, name: 'Lisbon', count: 48 },
    ],
    watched_folder_stats: () => [{ watchedId: 1, photoCount: 12480 }],
    app_info: () => ({ version: '0.0.0', libraryPath: '/home/ada/.local/share/photon/library.db', licence: 'MIT' }),
    last_folder: () => null,
    slideshow_interval: () => 4,
    set_stars: (args) => (args.ids || []).length,
    // The keyword dialog reports what landed, so these answer rather than staying silent.
    add_items_tag: (args) => ({ tag: args.tag, count: (args.ids || []).length }),
    export_apply_edits: () => true,
    export_items: (args) => ({ written: (args.ids || []).length, failed: 0, reason: null }),
    remove_items_tag: (args) => ({ tag: args.tag, count: (args.ids || []).length }),
    theme: () => P.get('theme') || 'system',
  };

  // Commands that change something: a screenshot never needs their answer, so they get null.
  const SILENT = [
    'add_folder', 'add_item_tag', 'add_to_album', 'create_album', 'delete_album', 'hide_tag',
    'remove_folder', 'remove_from_album', 'remove_item_tag', 'rename_album', 'rename_tag',
    'rescan_folder', 'restore_tag_rule', 'reveal_folder', 'reveal_in_file_manager',
    'reveal_library', 'reveal_watched', 'rotate_item', 'set_album_view', 'set_grid_view',
    'set_item_edit', 'set_last_folder', 'set_person_view', 'set_search_query',
    'set_export_apply_edits', 'set_slideshow_interval', 'set_star', 'set_tag_view', 'set_theme',
    'set_visible',
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

  const actions = {
    select: () => tile(7)?.click(),
    menu: () => {
      tile(7)?.click();
      tile(7)?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 700, clientY: 300 }));
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
    viewer: () => open(2),
    info: () => {
      open(2);
      later(500, () => click('button[aria-label="Photo information"]'));
    },
    crop: () => {
      open(2);
      later(500, () => click('button[aria-label="Crop"]'));
    },
    settings: () => click('button[aria-label="Settings"]'),
    appearance: () => {
      click('button[aria-label="Settings"]');
      later(100, () =>
        [...document.querySelectorAll('nav[aria-label="Settings sections"] button')]
          .find((b) => b.textContent.trim() === 'Appearance')
          ?.click(),
      );
    },
  };

  // After the first grid page has rendered; the xtask gives the page a virtual-time budget
  // well past these delays.
  window.addEventListener('load', () => later(400, () => actions[P.get('do')]?.()));
})();
