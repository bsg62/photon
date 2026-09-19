// Sets the theme before the first paint. The user's choice lives in library.db, which
// answers only after the app has started, so a Light choice on a dark desktop would paint
// dark first; app-theme.svelte.ts mirrors the choice into localStorage for this script.
// A file rather than an inline script because the CSP has no 'unsafe-inline' for scripts,
// and a classic script rather than part of the bundle because a module is deferred.
(function () {
  var choice = null;
  try {
    choice = localStorage.getItem('photon.theme');
  } catch (e) {
    // Storage can be blocked or cleared; the desktop's scheme is the fallback.
  }
  var dark = choice === 'dark' || (choice !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
})();
