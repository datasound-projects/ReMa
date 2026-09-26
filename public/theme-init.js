// Applies the saved theme before the first paint, so a dark launch never
// flashes light. Loaded synchronously from index.html (the CSP allows only
// same-origin scripts). The app keeps the theme in sync afterwards.
(function () {
  try {
    var theme = localStorage.getItem('rema.theme') === 'dark' ? 'dark' : 'light';
    document.documentElement.dataset.theme = theme;
  } catch {
    document.documentElement.dataset.theme = 'light';
  }
})();
