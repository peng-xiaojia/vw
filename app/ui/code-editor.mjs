// CodeMirror owns editor history, selections and scroll position per document tab.
export class CodeEditor {
  constructor(textarea, { onChange, onFind, onFormat, onZoom }) {
    this.textarea = textarea; this.docs = new Map(); this.syncing = false; this.tab = null;
    try { this.options = { lineNumbers: true, lineWrapping: false, ...JSON.parse(localStorage.getItem('vw.editor') || '{}') }; }
    catch { this.options = { lineNumbers: true, lineWrapping: false }; }
    this.cm = window.CodeMirror.fromTextArea(textarea, {
      ...this.options, tabSize: 4, indentUnit: 2, matchBrackets: true, screenReaderLabel: '文档源码',
      extraKeys: { 'Ctrl-F': () => onFind(false), 'Cmd-F': () => onFind(false), 'Ctrl-H': () => onFind(true),
        'Shift-Alt-F': onFormat, 'Ctrl-Tab': false, 'Shift-Ctrl-Tab': false },
    });
    this.wrapper = this.cm.getWrapperElement(); this.wrapper.classList.add('hidden');
    this.cm.on('change', () => {
      if (this.syncing) return;
      textarea.value = this.cm.getValue(); onChange(this.cm.getValue());
    });
    this.wrapper.addEventListener('wheel', event => {
      if (!event.ctrlKey || !event.deltaY) return;
      event.preventDefault(); onZoom(-event.deltaY); this.cm.refresh();
    }, { passive: false });
    new ResizeObserver(() => { if (!this.wrapper.classList.contains('hidden')) this.cm.refresh(); }).observe(this.wrapper);
    new MutationObserver(() => { if (!this.wrapper.classList.contains('hidden')) this.cm.refresh(); }).observe(document.documentElement, { attributes: true, attributeFilter: ['style', 'data-document-font', 'data-theme'] });
  }
  sync(tab, state) {
    const visible = Boolean(state.doc && (state.editing || state.doc.viewerId === 'text'));
    const wasHidden = this.wrapper.classList.contains('hidden');
    this.wrapper.classList.toggle('hidden', !visible);
    this.textarea.classList.add('hidden'); this.textarea.style.display = 'none';
    if (!visible) return;
    const text = state.editing ? state.draft : state.blocks.map(block => block.text ?? '').join('');
    this.syncing = true;
    try {
      const key = `${tab.id}:${state.editing ? 'edit' : 'preview'}`;
      if (!this.docs.has(key)) this.docs.set(key, new window.CodeMirror.Doc(text, this.mode(state.doc)));
      const next = this.docs.get(key);
      const switched = this.tab !== key;
      if (switched) { this.cm.swapDoc(next); this.tab = key; }
      if (next.getValue() !== text) {
        this.cm.operation(() => next.replaceRange(text, { line: 0, ch: 0 }, next.posFromIndex(next.getValue().length), 'framework'));
      }
      this.cm.setOption('mode', this.mode(state.doc));
      this.cm.setOption('firstLineNumber', state.editing ? 1 : state.viewStart * 100 + 1);
      this.cm.setOption('readOnly', !state.editing || state.saving || state.opening || state.locked);
      if (switched || wasHidden) this.cm.refresh();
    } finally { this.syncing = false; }
  }
  mode(doc) {
    const extension = String(doc?.sourcePath || doc?.path || '').split('.').at(-1).toLowerCase();
    if (['bat', 'cmd'].includes(extension)) return 'text/x-sh';
    return window.CodeMirror.findModeByExtension(extension)?.mime || 'text/plain';
  }
  toggle(option) {
    this.options[option] = !this.options[option]; this.cm.setOption(option, this.options[option]);
    localStorage.setItem('vw.editor', JSON.stringify(this.options)); return this.options[option];
  }
  focus() { this.cm.focus(); }
  remove(tab) { this.docs.delete(`${tab.id}:edit`); this.docs.delete(`${tab.id}:preview`); }
  navigate(offset, length = 0) {
    const doc = this.cm.getDoc();
    const from = doc.posFromIndex(offset), to = doc.posFromIndex(offset + length);
    doc.setSelection(from, to); this.cm.scrollIntoView({ from, to }, 80); this.cm.focus();
  }
  find(query, matchCase = false, replace = null, all = false) {
    if (!query) return { count: 0 };
    const doc = this.cm.getDoc();
    const selected = doc.getSelection();
    const same = matchCase ? selected === query : selected.toLowerCase() === query.toLowerCase();
    if (replace !== null && !all && same) {
      doc.replaceSelection(replace, 'end', '+replace'); return { count: 1 };
    }
    let cursor = doc.getSearchCursor(query, all ? { line: 0, ch: 0 } : doc.getCursor('to'), { caseFold: !matchCase });
    let count = 0;
    this.cm.operation(() => {
      if (all) {
        while (cursor.findNext()) { cursor.replace(replace); count++; }
      } else {
        if (!cursor.findNext()) { cursor = doc.getSearchCursor(query, { line: 0, ch: 0 }, { caseFold: !matchCase }); if (!cursor.findNext()) return; }
        const from = cursor.from(), to = cursor.to();
        doc.setSelection(from, to); this.cm.scrollIntoView({ from, to }, 60);
        if (replace !== null) { cursor.replace(replace); doc.setCursor(cursor.to()); }
        count = 1;
      }
    });
    return { count };
  }
}
