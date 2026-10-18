// The colour theme a visitor chose, over the system's setting: System, Light or Dark, kept
// in this browser alone. Loaded in each page's head, before anything is drawn, so a chosen
// theme is there from the first paint and not a moment after.
(function () {
  var KEY = 'photon-theme';
  var ORDER = ['system', 'light', 'dark'];
  var NAMES = { system: 'System', light: 'Light', dark: 'Dark' };

  // Storage can be switched off or throw (a private window, blocked site data): then the
  // choice lasts for the page, and the system's setting is the default again on the next.
  function stored() {
    try {
      var theme = localStorage.getItem(KEY);
      return theme === 'light' || theme === 'dark' ? theme : 'system';
    } catch (e) {
      return 'system';
    }
  }

  function store(theme) {
    try {
      if (theme === 'system') localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, theme);
    } catch (e) {}
  }

  var current = stored();

  function apply() {
    var root = document.documentElement;
    if (current === 'system') root.removeAttribute('data-theme');
    else root.setAttribute('data-theme', current);
    // A picture's dark source follows the system's setting by itself, through its media
    // query; a chosen theme decides it here.
    var sources = document.querySelectorAll('source[data-dark]');
    for (var i = 0; i < sources.length; i++) {
      sources[i].media = current === 'system' ? '(prefers-color-scheme: dark)'
        : current === 'dark' ? 'all' : 'not all';
    }
    var button = document.getElementById('theme');
    if (button) {
      button.textContent = NAMES[current];
      button.setAttribute('aria-label', 'Colour theme: ' + NAMES[current]);
    }
  }

  apply();
  document.addEventListener('DOMContentLoaded', function () {
    apply();
    var button = document.getElementById('theme');
    if (!button) return;
    // Hidden in the page itself, so that without this script there is no button that
    // does nothing.
    button.hidden = false;
    button.addEventListener('click', function () {
      current = ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
      store(current);
      apply();
    });
  });
})();
