export function clampPaneWidth(width, viewport, min, max) {
  return Math.round(Math.max(min, Math.min(width, max, Math.max(min, viewport - 280))));
}

export function bindPaneResizer(handle, pane, { key, side, min, max }) {
  const setWidth = width => {
    const value = clampPaneWidth(width, window.innerWidth, min, max);
    pane.style.width = `${value}px`;
    pane.style.flexBasis = `${value}px`;
    handle.setAttribute('aria-valuenow', String(value));
    return value;
  };
  handle.setAttribute('aria-valuemin', String(min));
  handle.setAttribute('aria-valuemax', String(max));
  const stored = Number(localStorage.getItem(key));
  if (Number.isFinite(stored) && stored > 0) setWidth(stored);
  let origin, width;
  handle.addEventListener('pointerdown', event => {
    if (event.button !== 0) return;
    origin = event.clientX; width = pane.getBoundingClientRect().width;
    handle.setPointerCapture(event.pointerId);
    document.body.classList.add('resizing-pane');
    event.preventDefault();
  });
  handle.addEventListener('pointermove', event => {
    if (origin === undefined) return;
    setWidth(width + (event.clientX - origin) * (side === 'left' ? 1 : -1));
  });
  const finish = () => {
    if (origin === undefined) return;
    origin = undefined; document.body.classList.remove('resizing-pane');
    localStorage.setItem(key, String(Math.round(pane.getBoundingClientRect().width)));
  };
  handle.addEventListener('pointerup', finish);
  handle.addEventListener('pointercancel', finish);
  handle.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    const step = event.shiftKey ? 40 : 10;
    const delta = (event.key === 'ArrowRight' ? 1 : -1) * (side === 'left' ? 1 : -1);
    localStorage.setItem(key, String(setWidth(pane.getBoundingClientRect().width + delta * step)));
  });
  window.addEventListener('resize', () => {
    if (pane.style.width) setWidth(Number.parseInt(pane.style.width, 10));
  });
}
