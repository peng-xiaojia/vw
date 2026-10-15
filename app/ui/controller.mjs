// Document state has one owner. The bridge is injected so races can be tested without a WebView.
const normalized = text => String(text).replace(/\r\n?/gu, '\n');

function storageText(draft, snapshot, explicit = null) {
  if (explicit && explicit !== 'preserve') return normalized(draft).replace(/\n/gu, { lf: '\n', crlf: '\r\n', cr: '\r' }[explicit]);
  const endings = new Set(String(snapshot).match(/\r\n|\r|\n/gu) ?? []);
  if (endings.size > 1) throw new Error('此文件混用多种换行符，当前编辑器无法无损保存；请保留原文件并使用支持混合换行符的编辑器。');
  const ending = endings.values().next().value || '\n';
  return normalized(draft).replace(/\n/gu, ending);
}

function externalStatus(action, appName, dirty) {
  const saved = dirty ? ' · 使用的是已保存版本，当前草稿仍保留' : '';
  if (action === 'terminal') return '已在终端打开所在目录';
  if (action === 'reveal') return '已在文件资源管理器中显示';
  if (action === 'app') return `已用 ${appName || '所选软件'} 打开${saved}`;
  return `已用其他软件打开${saved}`;
}

export class DocumentController {
  constructor({ invoke, confirmDirty = async () => 'cancel', onChange = () => {}, pageSize = 40 }) {
    this.invoke = invoke;
    this.confirmDirty = confirmDirty;
    this.onChange = onChange;
    this.pageSize = pageSize;
    this.state = {
      doc: null, blocks: [], viewStart: 0, viewGeneration: 0, loaded: 0, total: null, hasMore: false, outline: [],
      draft: '', snapshot: null, dirty: false, editing: false, hits: [], query: '',
      searchLimited: false, requestGeneration: 0, revision: 0,
      opening: false, loading: false, saving: false, searching: false, exporting: false,
      formatting: false, externalOpening: false, saveEncoding: null, lineEnding: 'preserve',
      externalApps: [],
      error: '', status: '打开文件或文件夹，也可以将文件拖到窗口中。',
    };
    this.pageRequest = null;
    this.saveRequest = null;
    this.guardRequest = null;
    this.externalAppsRequested = false;
    this.searchGeneration = 0;
    this.navigationGeneration = 0;
  }

  emit() { this.onChange(this.state); }
  fail(prefix, error) {
    this.state.error = `${prefix}：${error instanceof Error ? error.message : String(error)}`;
    this.state.status = this.state.error;
    this.emit();
  }
  matches(doc, revision) { return this.state.doc === doc && this.state.revision === revision; }
  async release(handle) {
    if (!handle) return;
    try { await this.invoke('vw_close', { handle }); }
    catch (error) { this.fail('释放文档失败', error); }
  }

  async guard({ skipReopen = false, skipExternal = false } = {}) {
    if (this.externalRequest && !skipExternal) await this.externalRequest;
    if (this.formatRequest) await this.formatRequest;
    if (this.reopenRequest && !skipReopen) await this.reopenRequest;
    if (this.saveRequest) await this.saveRequest;
    if (!this.state.dirty) return true;
    if (this.guardRequest) return this.guardRequest;
    const doc = this.state.doc;
    this.guardRequest = (async () => {
      try {
        const choice = await this.confirmDirty(doc);
        if (this.state.doc !== doc) return false;
        if (choice === 'save') return this.save();
        return choice === 'discard';
      } catch (error) { this.fail('无法确认未保存修改', error); return false; }
    })();
    try { return await this.guardRequest; }
    finally { this.guardRequest = null; }
  }

  async open(path, { inner = null, asViewer = null } = {}) {
    path = String(path ?? '').trim();
    if (!path) return false;
    const generation = ++this.state.requestGeneration;
    this.state.opening = false;
    if (!(await this.guard()) || generation !== this.state.requestGeneration) { this.emit(); return false; }
    this.state.opening = true;
    this.state.error = '';
    this.state.status = '正在打开…';
    this.emit();
    let info;
    try {
      info = await this.invoke('vw_open', { path, asViewer, inner });
      if (generation !== this.state.requestGeneration) { await this.release(info.handle); return false; }
      // Prepare a complete first view before replacing the visible document.
      const [page, outline, htmlSource] = await Promise.all([
        this.invoke('vw_page', { handle: info.handle, start: 0, count: this.pageSize }),
        this.invoke('vw_outline', { handle: info.handle }),
        info.viewerId === 'html' && info.editable && inner === null ? this.invoke('vw_snapshot', { handle: info.handle }) : null,
      ]);
      if (generation !== this.state.requestGeneration) { await this.release(info.handle); return false; }
      const old = this.state.doc;
      this.state.revision++;
      this.state.viewGeneration++;
      this.searchGeneration++;
      this.pageRequest = null;
      this.state.doc = { ...info, path: info.path || path, sourcePath: path, inner, editable: Boolean(info.editable) && inner === null && info.viewerId !== 'archive' };
      Object.assign(this.state, {
        blocks: [], viewStart: 0, loaded: 0, total: null, outline: outline ?? [],
        snapshot: htmlSource, draft: htmlSource === null ? '' : normalized(htmlSource), dirty: false, editing: false, hits: [], query: '',
        searchLimited: false, loading: false, searching: false, error: '',
        formatting: false, saveEncoding: null, lineEnding: 'preserve',
      });
      this.acceptPage(page);
      this.state.status = this.pageStatus();
      this.emit();
      if (old && old.handle !== info.handle) await this.release(old.handle);
      if (this.state.doc?.editable && this.state.doc.viewerId === 'text' && /\.txt$/iu.test(this.state.doc.sourcePath)) await this.edit();
      return true;
    } catch (error) {
      if (info) await this.release(info.handle);
      if (generation === this.state.requestGeneration) this.fail('打开失败', error);
      return false;
    } finally {
      if (generation === this.state.requestGeneration) { this.state.opening = false; this.emit(); }
    }
  }

  openMember(inner) {
    if (!this.state.doc) return Promise.resolve(false);
    return this.open(this.state.doc.sourcePath, { inner });
  }
  cancelOpen() {
    ++this.state.requestGeneration;
    this.state.opening = false;
    this.state.status = '已取消打开';
    this.emit();
  }

  acceptPage(page) {
    const blocks = Array.isArray(page?.blocks) ? page.blocks : [];
    this.state.blocks.push(...blocks);
    this.state.loaded = this.state.viewStart + this.state.blocks.length;
    this.state.total = Number.isFinite(page?.total) ? page.total : null;
    this.state.hasMore = blocks.length > 0 && (this.state.total === null || this.state.loaded < this.state.total);
  }
  pageStatus() {
    if (this.state.viewStart) return `第 ${this.state.viewStart + 1}–${this.state.loaded} 块${this.state.total === null ? '' : ` · 共 ${this.state.total} 块`}`;
    return this.state.total === null ? `已加载 ${this.state.loaded} 块` : `已加载 ${this.state.loaded}/${this.state.total} 块`;
  }

  loadMore() {
    if (!this.state.doc || !this.state.hasMore || this.state.saving) return Promise.resolve(false);
    if (this.pageRequest) return this.pageRequest.promise;
    if (this.state.loading) return Promise.resolve(false);
    const doc = this.state.doc;
    const revision = this.state.revision;
    const viewGeneration = this.state.viewGeneration;
    const start = this.state.loaded;
    this.state.loading = true;
    this.emit();
    const request = { promise: null };
    request.promise = (async () => {
      try {
        const page = await this.invoke('vw_page', { handle: doc.handle, start, count: this.pageSize });
        if (!this.matches(doc, revision) || viewGeneration !== this.state.viewGeneration) return false;
        this.acceptPage(page);
        this.state.status = this.pageStatus();
        this.state.error = '';
        this.emit();
        return this.state.loaded > start;
      } catch (error) {
        if (this.matches(doc, revision) && viewGeneration === this.state.viewGeneration) this.fail('加载失败', error);
        return false;
      } finally {
        if (this.pageRequest === request) {
          this.pageRequest = null;
          this.state.loading = false;
          this.emit();
        }
      }
    })();
    this.pageRequest = request;
    return request.promise;
  }

  async ensureBlock(index) {
    if (!Number.isSafeInteger(index) || index < 0 || !this.state.doc) return false;
    const doc = this.state.doc;
    const revision = this.state.revision;
    const navigationGeneration = ++this.navigationGeneration;
    if (index < this.state.viewStart || index >= this.state.loaded + this.pageSize * 3) {
      const viewGeneration = ++this.state.viewGeneration;
      this.pageRequest = null;
      this.state.loading = true;
      this.emit();
      try {
        const page = await this.invoke('vw_page', { handle: doc.handle, start: index, count: this.pageSize });
        if (!this.matches(doc, revision) || viewGeneration !== this.state.viewGeneration || navigationGeneration !== this.navigationGeneration) return false;
        if (!page?.blocks?.length) { this.state.status = '目标块不存在'; return false; }
        Object.assign(this.state, { blocks: [], viewStart: index, loaded: index, error: '' });
        this.acceptPage(page);
        this.state.status = this.pageStatus();
        return true;
      } catch (error) {
        if (this.matches(doc, revision) && viewGeneration === this.state.viewGeneration) this.fail('无法跳转', error);
        return false;
      } finally {
        if (this.matches(doc, revision) && viewGeneration === this.state.viewGeneration) { this.state.loading = false; this.emit(); }
      }
    }
    while (this.matches(doc, revision) && navigationGeneration === this.navigationGeneration && this.state.loaded <= index && this.state.hasMore) {
      if (!(await this.loadMore())) break;
    }
    return this.matches(doc, revision) && navigationGeneration === this.navigationGeneration && index >= this.state.viewStart && index < this.state.loaded;
  }

  async search(query) {
    const doc = this.state.doc;
    if (!doc || this.state.saving) return false;
    const revision = this.state.revision;
    const generation = ++this.searchGeneration;
    this.state.query = String(query ?? '').trim();
    this.state.hits = [];
    this.state.searchLimited = false;
    if (!this.state.query) { this.state.searching = false; this.emit(); return true; }
    this.state.searching = true;
    this.emit();
    try {
      const result = await this.invoke('vw_search', { handle: doc.handle, query: this.state.query });
      if (!this.matches(doc, revision) || generation !== this.searchGeneration) return false;
      const hits = Array.isArray(result) ? result : result?.hits ?? [];
      this.state.hits = hits.slice(0, 500);
      this.state.searchLimited = hits.length >= 500 || Boolean(result?.next);
      this.state.status = `找到 ${this.state.hits.length} 处${this.state.searchLimited ? '（已达显示上限，请缩小搜索范围）' : ''}`;
      this.state.error = '';
      return true;
    } catch (error) {
      if (this.matches(doc, revision) && generation === this.searchGeneration) this.fail('搜索失败', error);
      return false;
    } finally {
      if (this.matches(doc, revision) && generation === this.searchGeneration) { this.state.searching = false; this.emit(); }
    }
  }

  async edit() {
    const doc = this.state.doc;
    const revision = this.state.revision;
    if (!doc?.editable || this.state.saving) return false;
    try {
      if (this.state.snapshot === null) {
        const text = await this.invoke('vw_snapshot', { handle: doc.handle });
        if (!this.matches(doc, revision)) return false;
        this.state.snapshot = text;
        this.state.draft = normalized(text);
      }
      this.state.editing = true;
      this.state.error = '';
      this.state.status = this.state.dirty ? '编辑中 · 有未保存修改' : '编辑中';
      this.emit();
      return true;
    } catch (error) { if (this.matches(doc, revision)) this.fail('无法编辑', error); return false; }
  }
  setDraft(text) {
    if (!this.state.doc?.editable || this.state.saving || this.state.opening) return;
    this.state.draft = normalized(text);
    this.state.dirty = this.state.draft !== normalized(this.state.snapshot) || this.state.saveEncoding !== null || this.state.lineEnding !== 'preserve';
    this.state.status = this.state.dirty ? '有未保存修改' : '编辑中';
    this.emit();
  }

  setTextOptions({ encoding, lineEnding } = {}) {
    if (!this.state.doc?.editable || this.state.snapshot === null || this.state.saving || this.state.opening) return false;
    if (encoding !== undefined) this.state.saveEncoding = encoding || null;
    if (lineEnding !== undefined && ['preserve', 'lf', 'crlf', 'cr'].includes(lineEnding)) this.state.lineEnding = lineEnding;
    this.setDraft(this.state.draft);
    return true;
  }

  format(formatter) {
    if (!this.state.doc?.format || this.formatRequest || this.state.saving || this.state.opening) return Promise.resolve(false);
    const request = (async () => {
      if (!(await this.edit())) return false;
      const doc = this.state.doc, revision = this.state.revision, draft = this.state.draft;
      this.state.formatting = true; this.emit();
      try {
        const text = await formatter(doc, draft);
        if (!this.matches(doc, revision) || this.state.draft !== draft) return false;
        this.setDraft(text); this.state.status = '格式化完成 · 尚未保存';
        return true;
      } catch (error) { if (this.matches(doc, revision)) this.fail('格式化失败', error); return false; }
      finally { this.state.formatting = false; this.emit(); }
    })();
    this.formatRequest = request;
    request.finally(() => { if (this.formatRequest === request) this.formatRequest = null; });
    return request;
  }

  reopenEncoding(encoding) {
    if (!this.state.doc || this.reopenRequest || this.state.saving || this.state.opening) return Promise.resolve(false);
    const request = (async () => {
      if (!(await this.guard({ skipReopen: true }))) return false;
      const doc = this.state.doc, revision = this.state.revision;
      this.state.saving = true; this.emit();
      try {
        const info = await this.invoke('vw_reopen_encoding', { handle: doc.handle, encoding });
        const [page, outline, text] = await Promise.all([
          this.invoke('vw_page', { handle: doc.handle, start: 0, count: this.pageSize }),
          this.invoke('vw_outline', { handle: doc.handle }), this.invoke('vw_snapshot', { handle: doc.handle }),
        ]);
        if (!this.matches(doc, revision)) return false;
        Object.assign(doc, info); ++this.state.revision; ++this.state.viewGeneration; ++this.searchGeneration;
        this.pageRequest = null;
        Object.assign(this.state, { blocks: [], loaded: 0, viewStart: 0, outline: outline ?? [], snapshot: text, draft: normalized(text),
          saveEncoding: null, lineEnding: 'preserve', dirty: false, editing: true, hits: [], query: '', loading: false, error: '', status: `已使用 ${encoding} 重新解码` });
        this.acceptPage(page); return true;
      } catch (error) { if (this.matches(doc, revision)) this.fail('重新解码失败', error); return false; }
      finally { this.state.saving = false; this.emit(); }
    })();
    this.reopenRequest = request;
    request.finally(() => { if (this.reopenRequest === request) this.reopenRequest = null; });
    return request;
  }
  async preview() {
    if (!this.state.editing) return true;
    const doc = this.state.doc;
    if (doc?.viewerId === 'markdown') {
      const revision = this.state.revision, draft = this.state.draft;
      try {
        const preview = await this.invoke('vw_preview_markdown', { text: draft });
        if (!this.matches(doc, revision) || this.state.draft !== draft || !this.state.editing) return false;
        const blocks = Array.isArray(preview.blocks) ? preview.blocks : [];
        Object.assign(this.state, {
          blocks, outline: preview.outline ?? [], viewStart: 0, loaded: blocks.length,
          total: blocks.length, hasMore: false, hits: [], query: '',
        });
        ++this.state.viewGeneration;
      } catch (error) {
        if (this.matches(doc, revision)) this.fail('无法展示 Markdown 草稿', error);
        return false;
      }
    }
    this.state.editing = false;
    if (doc) this.state.status = this.state.dirty ? '展示当前修改 · 尚未保存' : this.pageStatus();
    this.emit();
    return true;
  }

  save() {
    if (this.saveRequest) return this.saveRequest;
    if (this.state.saving) return Promise.resolve(false);
    const doc = this.state.doc;
    if (!doc?.editable || this.state.snapshot === null || this.state.opening) return Promise.resolve(false);
    if (!this.state.dirty) { this.preview(); return Promise.resolve(true); }
    let text;
    try { text = storageText(this.state.draft, this.state.snapshot, this.state.lineEnding); }
    catch (error) { this.fail('无法无损保存', error); return Promise.resolve(false); }
    this.state.saving = true;
    this.state.status = '正在保存…';
    this.state.error = '';
    // Invalidate pages/search already in flight: they belong to the old core revision.
    const revision = ++this.state.revision;
    ++this.state.viewGeneration;
    ++this.searchGeneration;
    this.pageRequest = null;
    this.state.loading = false;
    this.state.searching = false;
    this.emit();
    this.saveRequest = (async () => {
      let wrote = false;
      try {
        const result = await this.invoke('vw_save', { handle: doc.handle, text, ...(this.state.saveEncoding ? { encoding: this.state.saveEncoding } : {}) });
        wrote = true;
        if (!this.matches(doc, revision)) return false;
        // Never reuse blocks or outline from before a successful write.
        Object.assign(this.state, { blocks: [], viewStart: 0, loaded: 0, total: null, hasMore: false, outline: [], hits: [], query: '' });
        this.emit();
        const [page, outline, snapshot] = await Promise.all([
          this.invoke('vw_page', { handle: doc.handle, start: 0, count: this.pageSize }),
          this.invoke('vw_outline', { handle: doc.handle }),
          this.invoke('vw_snapshot', { handle: doc.handle }),
        ]);
        if (!this.matches(doc, revision)) return false;
        if (result.info) Object.assign(doc, result.info, { editable: Boolean(result.info.editable) && doc.inner === null && result.info.viewerId !== 'archive' });
        this.acceptPage(page);
        Object.assign(this.state, { outline: outline ?? [], snapshot, draft: normalized(snapshot), dirty: false, editing: false, saveEncoding: null, lineEnding: 'preserve' });
        this.state.status = `已保存 ${result.bytes} 字节${result.backup ? ' · 已创建备份' : ''}`;
        return true;
      } catch (error) {
        if (this.matches(doc, revision)) this.fail(wrote ? '文件已写入，刷新失败；请重试保存以刷新' : '保存失败', error);
        return false;
      } finally {
        this.saveRequest = null;
        this.state.saving = false;
        this.emit();
      }
    })();
    return this.saveRequest;
  }

  async close() {
    const generation = ++this.state.requestGeneration;
    if (!(await this.guard()) || generation !== this.state.requestGeneration) return false;
    this.cancelOpen();
    const doc = this.state.doc;
    ++this.state.revision;
    ++this.searchGeneration;
    this.state.doc = null;
    this.state.dirty = false;
    this.emit();
    if (doc) await this.release(doc.handle);
    return true;
  }

  openElsewhere(action, canProceed = () => true, extras = {}) {
    const doc = this.state.doc;
    if (!doc || !['other_app', 'terminal', 'reveal', 'app'].includes(action) || this.externalRequest || this.state.saving || this.state.opening) return Promise.resolve(false);
    if (doc.inner != null) { this.fail('无法外部打开', '请先导出归档内文件，再用其他软件或终端打开副本。'); return Promise.resolve(false); }
    if (action === 'other_app' && doc.viewerId === 'dir') return Promise.resolve(false);
    this.state.externalOpening = true; this.emit();
    const request = (async () => {
      try {
        if (!(await this.guard({ skipExternal: true })) || this.state.doc !== doc || !canProceed()) return false;
        const previousStatus = this.state.status;
        const opened = action === 'app'
          ? await this.invoke('vw_launch_external_app', { handle: doc.handle, appId: extras.appId })
          : await this.invoke('vw_open_elsewhere', { handle: doc.handle, action });
        if (this.state.doc !== doc) return false;
        this.state.status = opened ? externalStatus(action, extras.appName, this.state.dirty) : previousStatus;
        if (opened) this.state.error = '';
        return Boolean(opened);
      } catch (error) { if (this.state.doc === doc) this.fail('外部打开失败', error); return false; }
      finally { this.state.externalOpening = false; this.emit(); }
    })();
    this.externalRequest = request;
    request.finally(() => { if (this.externalRequest === request) this.externalRequest = null; });
    return request;
  }

  openInApp(appId, appName = '', canProceed = () => true) {
    return this.openElsewhere('app', canProceed, { appId, appName });
  }

  async ensureExternalApps() {
    if (this.externalAppsRequested) return;
    this.externalAppsRequested = true;
    try { this.state.externalApps = await this.invoke('vw_external_apps'); }
    catch { this.state.externalApps = []; }
  }

  async exportCopy() {
    const doc = this.state.doc;
    if (!doc || this.state.exporting || this.state.opening || this.state.saving) return false;
    this.state.exporting = true;
    this.emit();
    try {
      const bytes = await this.invoke('vw_export', { handle: doc.handle });
      if (this.state.doc !== doc) return false;
      if (bytes === null || bytes === undefined) return false;
      this.state.error = '';
      this.state.status = `已导出 ${bytes} 字节${this.state.dirty ? ' · 导出的是已保存版本，当前修改仍未保存' : ''}`;
      return true;
    } catch (error) {
      if (this.state.doc === doc) this.fail('导出失败（目标已存在时请另选新文件名）', error);
      return false;
    } finally { this.state.exporting = false; this.emit(); }
  }
}
