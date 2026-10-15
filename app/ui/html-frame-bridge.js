// Runs inside the sandbox. It has no application APIs or access to the parent DOM.
(() => {
  const send = (action, value) => parent.postMessage({ type: 'vw-html', action, value }, '*');
  window.addEventListener('wheel', event => {
    if (!event.ctrlKey || !event.deltaY) return;
    event.preventDefault(); send('zoom', event.deltaY < 0 ? 1 : -1);
  }, { passive: false });
  window.addEventListener('keydown', event => {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key !== 'tab') return;
    event.preventDefault(); send('key', { key, shift: event.shiftKey });
  });
  window.addEventListener('message', event => {
    if (event.source !== parent || event.data?.type !== 'vw-html-zoom') return;
    const value = Number(event.data.value);
    if (Number.isFinite(value) && value >= 0.5 && value <= 3) document.documentElement.style.zoom = String(value);
  });
  document.addEventListener('click', event => {
    const anchor = event.target.closest?.('a[href]');
    if (!anchor) return;
    const url = new URL(anchor.href, location.href);
    if (url.origin !== location.origin || url.protocol === 'mailto:') {
      event.preventDefault(); send('external', url.href);
    } else if (anchor.target && anchor.target !== '_self') {
      event.preventDefault(); location.href = url.href;
    }
  }, true);
  send('ready');
})();
