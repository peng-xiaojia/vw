import { externalUrl } from './security.mjs';

// Keep each frame attached while hidden so page navigation and scroll survive tab switches.
export class HtmlPreviews {
  constructor({ host, invoke, onError, onKey }) {
    this.host = host; this.invoke = invoke; this.onError = onError; this.onKey = onKey;
    this.host.classList.remove('hidden');
    this.frames = new Map(); this.active = null;
    this.linkPrompt = document.createElement('div'); this.linkPrompt.className = 'html-link-prompt hidden';
    this.linkText = document.createElement('span');
    const open = document.createElement('button'); open.textContent = '打开外部链接'; open.type = 'button';
    const cancel = document.createElement('button'); cancel.textContent = '取消'; cancel.type = 'button';
    open.addEventListener('click', event => {
      if (!event.isTrusted || !this.pendingLink) return;
      const { tab, url } = this.pendingLink; this.dismissLink();
      this.invoke('vw_open_external', { url }).catch(error => this.onError(tab, error));
    });
    cancel.addEventListener('click', () => this.dismissLink());
    this.linkPrompt.append(this.linkText, open, cancel); this.host.append(this.linkPrompt);
    window.addEventListener('message', event => {
      const entry = this.frames.get(this.active);
      if (!entry || this.host.classList.contains('html-preview-inactive') || event.source !== entry.frame.contentWindow || event.data?.type !== 'vw-html') return;
      const { action, value } = event.data;
      if (action === 'zoom' && (value === 1 || value === -1)) {
        entry.zoom = Math.max(0.5, Math.min(3, Math.round((entry.zoom + value * 0.1) * 10) / 10)); this.zoom(entry);
      }
      if (action === 'ready') this.zoom(entry);
      if (action === 'key' && value?.key === 'tab') this.onKey?.(value);
      if (action === 'external') {
        const url = externalUrl(value);
        if (url && !this.pendingLink) {
          this.pendingLink = { tab: entry.tab, url }; this.linkText.textContent = url;
          this.linkPrompt.classList.remove('hidden');
        }
      }
    });
  }
  dismissLink() { this.pendingLink = null; this.linkPrompt.classList.add('hidden'); }
  zoom(entry) { entry.frame.contentWindow?.postMessage({ type: 'vw-html-zoom', value: entry.zoom }, '*'); }
  show(tab, state) {
    if (this.active !== tab?.id || state.editing) this.dismissLink();
    this.active = tab?.id ?? null;
    const visible = Boolean(tab && state.doc?.viewerId === 'html' && !state.editing);
    this.host.classList.toggle('html-preview-inactive', !visible);
    this.host.setAttribute('aria-hidden', String(!visible)); this.host.inert = !visible;
    for (const [id, entry] of this.frames) entry.frame.classList.toggle('html-preview-inactive', !visible || id !== this.active);
    if (!visible) return;
    let entry = this.frames.get(tab.id);
    if (!entry) {
      const frame = document.createElement('iframe'); frame.title = state.doc.title || 'HTML 原页预览';
      frame.setAttribute('sandbox', 'allow-scripts'); frame.referrerPolicy = 'no-referrer';
      entry = { tab, frame, source: undefined, revision: -1, zoom: 1, generation: 0 };
      this.frames.set(tab.id, entry); this.host.append(frame);
      frame.addEventListener('load', () => this.zoom(entry));
    }
    entry.frame.classList.remove('html-preview-inactive');
    const source = state.snapshot === null ? null : state.draft;
    if (entry.source === source && entry.revision === state.revision) return;
    entry.source = source; entry.revision = state.revision;
    const generation = ++entry.generation;
    this.invoke('vw_html_preview', { handle: state.doc.handle, text: source, generation }).then(url => {
      if (this.frames.get(tab.id) !== entry || entry.generation !== generation) return;
      entry.frame.src = url;
    }).catch(error => { if (this.frames.get(tab.id) === entry && entry.generation === generation) this.onError(tab, error); });
  }
  remove(tab) { this.frames.get(tab.id)?.frame.remove(); this.frames.delete(tab.id); }
}
