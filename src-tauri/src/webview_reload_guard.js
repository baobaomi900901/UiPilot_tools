// Whole-document reload cannot restore the host's active invocation or plugin session.
// Install before page scripts, in every WebView/frame; business refresh and HMR are untouched.
(() => {
  window.addEventListener('keydown', (event) => {
    if (event.altKey || event.metaKey) return;
    const reload = event.key === 'F5' || event.key === 'BrowserRefresh' ||
      (event.ctrlKey && event.key.toLowerCase() === 'r');
    if (!reload) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  }, true);
})();
