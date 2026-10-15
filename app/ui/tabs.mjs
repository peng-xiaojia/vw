import { DocumentController } from './controller.mjs';

function documentKey(path, { inner = null, asViewer = null } = {}) {
  let name = String(path).trim().replaceAll('\\', '/').replace(/^\/\/\?\//u, '');
  if (/^[a-z]:\//iu.test(name) || name.startsWith('//')) name = name.toLowerCase();
  return JSON.stringify([name, inner, asViewer]);
}

// A tab owns a complete controller, including in-flight requests and its native handle.
export class DocumentTabs {
  constructor({ invoke, confirmDirty, onChange = () => {}, beforeActivate = () => {}, onRemove = () => {} }) {
    this.options = { invoke, confirmDirty };
    this.onChange = onChange; this.beforeActivate = beforeActivate; this.onRemove = onRemove;
    this.tabs = []; this.active = null; this.serial = 0; this.closing = false;
    this.empty = new DocumentController({ invoke, onChange: () => this.emit() });
  }
  get state() { return (this.active?.controller ?? this.empty).state; }
  emit() { this.onChange(this.state); }
  activate(id) {
    if (this.closing) return false;
    const tab = this.tabs.find(item => item.id === id);
    if (!tab || this.active === tab) return false;
    this.beforeActivate(this.active, tab);
    this.active = tab; this.emit(); return true;
  }
  cycle(step = 1) {
    if (this.tabs.length < 2) return;
    const index = this.tabs.indexOf(this.active);
    this.activate(this.tabs[(index + step + this.tabs.length) % this.tabs.length].id);
  }
  open(path, options = {}) {
    if (this.closing || !String(path ?? '').trim()) return Promise.resolve(false);
    const key = documentKey(path, options);
    const existing = this.tabs.find(tab => tab.key === key);
    if (existing) { this.activate(existing.id); return existing.opening ?? Promise.resolve(true); }
    const tab = { id: `doc-${++this.serial}`, key, path, view: {}, controller: null, opening: null };
    tab.controller = new DocumentController({ ...this.options, onChange: () => this.emit() });
    this.tabs.push(tab); this.activate(tab.id);
    tab.opening = (async () => {
      const opened = await tab.controller.open(path, options);
      if (!opened && this.tabs.includes(tab)) {
        const error = tab.controller.state.error;
        this.remove(tab);
        if (error) { (this.active?.controller ?? this.empty).state.error = error; this.emit(); }
      }
      tab.opening = null;
      return opened;
    })();
    return tab.opening;
  }
  openMember(inner) { return this.state.doc ? this.open(this.state.doc.sourcePath, { inner }) : Promise.resolve(false); }
  remove(tab) {
    const index = this.tabs.indexOf(tab);
    if (index < 0) return;
    if (this.active === tab) {
      const next = this.tabs[index + 1] ?? this.tabs[index - 1] ?? null;
      this.beforeActivate(tab, next); this.active = next;
    }
    this.tabs.splice(index, 1); this.onRemove(tab); this.emit();
  }
  async close(id = this.active?.id) {
    if (this.closing) return false;
    const tab = this.tabs.find(item => item.id === id);
    if (!tab) return true;
    this.closing = true;
    this.emit();
    try {
      if (!(await tab.controller.close())) return false;
      this.remove(tab); return true;
    } finally { this.closing = false; this.emit(); }
  }
  async closeAll() {
    if (this.closing) return false;
    this.closing = true;
    this.emit();
    try {
      // Check every draft first: cancelling exit leaves all tabs open.
      for (const tab of this.tabs) if (!(await tab.controller.guard())) return false;
      for (const tab of [...this.tabs]) {
        tab.controller.cancelOpen();
        const doc = tab.controller.state.doc;
        if (doc) await tab.controller.release(doc.handle);
        this.remove(tab);
      }
      return true;
    } finally { this.closing = false; this.emit(); }
  }
  cancelOpen() { return this.close(); }
  fail(...args) { return (this.active?.controller ?? this.empty).fail(...args); }
  pageStatus() { return (this.active?.controller ?? this.empty).pageStatus(); }
  edit() { return this.closing || this.state.externalOpening ? Promise.resolve(false) : (this.active?.controller ?? this.empty).edit(); }
  preview() { return this.closing || this.state.externalOpening ? Promise.resolve(false) : (this.active?.controller ?? this.empty).preview(); }
  setDraft(text) { if (!this.closing && !this.state.externalOpening) return (this.active?.controller ?? this.empty).setDraft(text); }
  async setTextOptions(options) {
    const tab = this.active;
    if (this.closing || !tab || tab.controller.state.externalOpening || !(await tab.controller.edit())) return false;
    if (this.closing || tab.controller.state.externalOpening || !this.tabs.includes(tab)) return false;
    tab.controller.setTextOptions(options); return true;
  }
  reopenEncoding(encoding) { return this.closing || this.state.externalOpening ? Promise.resolve(false) : this.active?.controller.reopenEncoding(encoding); }
  format(formatter) { return this.closing || this.state.externalOpening ? Promise.resolve(false) : this.active?.controller.format(formatter); }
  save() { return this.closing || this.state.externalOpening ? Promise.resolve(false) : (this.active?.controller ?? this.empty).save(); }
  openElsewhere(action) {
    const tab = this.active;
    return this.closing || !tab ? Promise.resolve(false) : tab.controller.openElsewhere(action, () => !this.closing && this.tabs.includes(tab));
  }
  openInApp(appId, appName = '') {
    const tab = this.active;
    return this.closing || !tab ? Promise.resolve(false) : tab.controller.openInApp(appId, appName, () => !this.closing && this.tabs.includes(tab));
  }
  ensureExternalApps() { return (this.active?.controller ?? this.empty).ensureExternalApps(); }
  loadMore() { return (this.active?.controller ?? this.empty).loadMore(); }
  ensureBlock(index) { return (this.active?.controller ?? this.empty).ensureBlock(index); }
  search(query) { return this.closing ? Promise.resolve(false) : (this.active?.controller ?? this.empty).search(query); }
  exportCopy() { return this.closing ? Promise.resolve(false) : (this.active?.controller ?? this.empty).exportCopy(); }
}
