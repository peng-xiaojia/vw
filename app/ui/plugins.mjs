import { icon } from './icons.mjs';

export class PluginCenter {
  constructor({ invoke, onVisibility }) {
    this.invoke = invoke; this.onVisibility = onVisibility; this.entries = []; this.marketEntries = []; this.busy = false; this.visible = false; this.operation = null;
    try { this.savedRepositories = JSON.parse(localStorage.getItem('vw.market.repositories') || '[]'); }
    catch { this.savedRepositories = []; }
    if (!Array.isArray(this.savedRepositories)) this.savedRepositories = [];
    this.savedRepositories = this.savedRepositories.filter(repo => typeof repo === 'string' && /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo) && !repo.includes('..'));
  }
  get(id) { return document.getElementById(id); }
  status(text) { this.get('plugin-status').textContent = text; }
  setBusy(value) {
    this.busy = value;
    for (const element of this.get('plugin-center').querySelectorAll('button, input')) element.disabled = value;
    this.get('plugin-center').setAttribute('aria-busy', String(value));
  }
  section(name) {
    for (const element of this.get('plugin-center').querySelectorAll('[data-plugin-section]')) element.classList.toggle('hidden', element.dataset.pluginSection !== name);
    for (const button of this.get('plugin-center').querySelectorAll('[data-section]')) {
      if (button.dataset.section === name) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current');
    }
  }
  show() {
    this.visible = true; this.onVisibility(true); this.section('market'); this.renderMarket(); this.searchMarket();
    this.get('btn-close-plugins').focus();
  }
  hide() { if (!this.busy) { this.visible = false; this.onVisibility(false); this.get('btn-plugins').focus(); } }
  async refresh() {
    if (this.busy) return;
    this.setBusy(true); this.status('正在读取插件…');
    try {
      const result = await this.invoke('vw_plugins');
      this.entries = result.entries; this.get('plugin-directory').textContent = result.directory; this.get('plugin-platform').textContent = `当前平台：${result.platform}`;
      this.render(); this.status(`${this.entries.length} 个插件 · 安装或卸载后重启生效`);
    } catch (error) { this.status(`读取插件失败：${error.message || error}`); }
    finally { this.setBusy(false); }
  }
  async searchMarket() {
    if (this.marketBusy) return;
    this.marketBusy = true; this.get('btn-market-search').disabled = true; this.status('正在读取插件市场…');
    try {
      this.marketEntries = await this.invoke('vw_plugin_market', { query: this.get('market-query').value.trim() });
      this.renderMarket(); this.status(`${this.marketEntries.length} 个公开插件仓库 · ${this.savedRepositories.length} 个自选仓库`);
    } catch (error) { this.status(`读取市场失败：${error.message || error}`); }
    finally { this.marketBusy = false; this.get('btn-market-search').disabled = false; }
  }
  renderMarket() {
    const list = this.get('market-list'); list.replaceChildren();
    const known = new Set(this.marketEntries.map(entry => entry.repository.toLowerCase()));
    const entries = [...this.marketEntries, ...this.savedRepositories.filter(repo => !known.has(repo.toLowerCase())).map(repo => ({ repository: repo, name: repo.split('/')[1], description: '手动添加的插件仓库', stars: 0, url: `https://github.com/${repo}` }))];
    for (const entry of entries) {
      const card = document.createElement('article'); card.className = 'plugin-card';
      const heading = document.createElement('h3'); heading.append(icon('blocks'), document.createTextNode(entry.name));
      const description = document.createElement('p'); description.textContent = entry.description || entry.repository;
      const identity = document.createElement('p'); identity.className = 'plugin-id'; identity.textContent = `${entry.repository} · ${entry.stars} stars`;
      const actions = document.createElement('div'); actions.className = 'market-actions';
      const install = document.createElement('button'); install.textContent = '下载并安装…';
      install.addEventListener('click', () => this.prepare(`github:${entry.repository}`));
      const source = document.createElement('button'); source.textContent = '查看仓库';
      source.addEventListener('click', () => this.invoke('vw_open_external', { url: entry.url }).catch(error => this.status(`无法打开仓库：${error.message || error}`)));
      actions.append(install, source); card.append(heading, description, identity, actions); list.append(card);
    }
    if (!list.childElementCount) { const empty = document.createElement('p'); empty.textContent = '没有找到已发布的插件仓库。'; list.append(empty); }
  }
  addRepository() {
    const input = this.get('market-repository');
    const repository = input.value.trim().replace(/^github:/i, '');
    if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository) || repository.includes('..')) { this.status('请输入 owner/repo 格式的 GitHub 仓库。'); return; }
    if (!this.savedRepositories.some(item => item.toLowerCase() === repository.toLowerCase())) this.savedRepositories.push(repository);
    localStorage.setItem('vw.market.repositories', JSON.stringify(this.savedRepositories));
    input.value = ''; this.renderMarket(); this.status(`已添加 ${repository}。`);
  }
  async publish() {
    const repository = this.get('publish-repository').value.trim();
    if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository) || repository.includes('..')) {
      this.get('publish-error').textContent = '请输入 owner/repo 格式的 GitHub 仓库。'; return;
    }
    try {
      await this.invoke('vw_open_external', { url: `https://github.com/${repository}/releases/new` });
      this.get('publish-dialog').close();
    } catch (error) { this.get('publish-error').textContent = `无法打开上传页面：${error.message || error}`; }
  }
  render() {
    const list = this.get('plugin-list'); list.replaceChildren();
    const filter = this.get('plugin-filter').value.trim().toLowerCase();
    for (const entry of this.entries) {
      const m = entry.manifest;
      const formats = m.contributes.viewers.flatMap(v => v.extensions);
      const builtinNames = { text: '文本', markdown: 'Markdown', json: 'JSON', csv: 'CSV / TSV', image: '图片', office: 'Office 文档', pdf: 'PDF', archive: 'ZIP 归档', dir: '文件夹', hex: '十六进制' };
      const name = entry.builtin ? builtinNames[m.contributes.viewers[0]?.id] || m.name : m.name;
      if (![name, m.name, m.id, ...formats].join(' ').toLowerCase().includes(filter)) continue;
      const card = document.createElement('article'); card.className = 'plugin-card';
      const heading = document.createElement('h3'); heading.append(icon('blocks'), document.createTextNode(name));
      const version = document.createElement('span'); version.className = 'badge'; version.textContent = `v${m.version}`; heading.append(version);
      const description = document.createElement('p'); description.textContent = formats.length ? formats.map(ext => `.${ext}`).join(' · ') : '目录或二进制文件';
      const identity = document.createElement('p'); identity.className = 'plugin-id'; identity.textContent = m.id;
      const state = document.createElement('p'); state.textContent = entry.error ? `不可用：${entry.error}` : entry.builtin ? '内置 · 已加载' : entry.loaded ? '已安装 · 已加载' : '已安装 · 重启后加载';
      const permissions = document.createElement('p'); permissions.className = 'plugin-id'; permissions.textContent = `权限：${m.permissions.length ? m.permissions.join('、') : '无额外权限声明'}`;
      card.append(heading, description, identity, state, permissions);
      if (!entry.builtin) {
        const remove = document.createElement('button'); remove.textContent = '卸载…';
        remove.addEventListener('click', () => this.confirmRemove(m)); card.append(remove);
      }
      list.append(card);
    }
    if (!list.childElementCount) { const text = document.createElement('p'); text.textContent = '没有匹配的插件。'; list.append(text); }
  }
  async prepare(spec) {
    if (this.busy) return;
    this.setBusy(true); this.status('正在获取并校验插件…');
    try {
      const result = await this.invoke('vw_plugin_prepare', { spec });
      if (!result) { this.status('已取消选择。'); return; }
      const m = result.manifest;
      this.operation = { kind: 'install', token: result.token };
      this.dialog('安装插件？', [ `${m.name} · v${m.version}`, m.id, `格式：${m.contributes.viewers.flatMap(v => v.extensions).join('、') || '由插件探测'}`, `权限：${m.permissions.join('、') || '无额外权限声明'}`, 'SHA-256 校验已通过。外部插件是普通本机进程，权限声明目前不由操作系统强制隔离。', '安装后重启生效。' ], '安装');
      this.status('校验通过，请确认插件信息。');
    } catch (error) { this.status(`获取插件失败：${error.message || error}`); }
    finally { this.setBusy(false); }
  }
  dialog(title, lines, action) {
    this.get('plugin-confirm-title').textContent = title;
    const detail = this.get('plugin-confirm-detail'); detail.replaceChildren();
    for (const text of lines) { const p = document.createElement('p'); p.textContent = text; detail.append(p); }
    this.get('btn-plugin-confirm').textContent = action; this.get('plugin-dialog').showModal(); this.get('btn-plugin-cancel').focus();
  }
  confirmRemove(manifest) {
    if (this.busy) return;
    this.operation = { kind: 'remove', id: manifest.id };
    this.dialog('卸载插件？', [`${manifest.name} · ${manifest.id}`, '不会删除文档。当前已加载的插件将在重启后移除。'], '卸载');
  }
  async cancel() {
    this.get('plugin-dialog').close(); this.operation = null;
    try { await this.invoke('vw_plugin_discard'); } catch (error) { this.status(`清理安装预览失败：${error.message || error}`); }
  }
  async confirm() {
    const operation = this.operation;
    if (!operation || this.busy) return;
    this.operation = null; this.get('plugin-dialog').close(); this.setBusy(true);
    let message;
    try {
      await this.invoke(operation.kind === 'install' ? 'vw_plugin_install' : 'vw_plugin_remove', operation.kind === 'install' ? { token: operation.token } : { id: operation.id });
      message = `${operation.kind === 'install' ? '安装' : '卸载'}成功。请关闭程序后重新打开以生效。`;
    } catch (error) { message = `操作失败：${error.message || error}`; }
    finally { this.setBusy(false); }
    await this.refresh(); this.section('installed'); this.status(message);
  }
  wire() {
    this.get('btn-plugins').addEventListener('click', () => this.visible ? this.hide() : this.show());
    this.get('btn-close-plugins').addEventListener('click', () => this.hide());
    for (const button of this.get('plugin-center').querySelectorAll('[data-section]')) button.addEventListener('click', () => { this.section(button.dataset.section); if (button.dataset.section === 'installed') this.refresh(); });
    this.get('btn-plugin-refresh').addEventListener('click', () => this.refresh());
    this.get('btn-market-search').addEventListener('click', () => this.searchMarket());
    this.get('market-add-form').addEventListener('submit', event => { event.preventDefault(); this.addRepository(); });
    this.get('market-query').addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); this.searchMarket(); } });
    this.get('btn-market-publish').addEventListener('click', () => { this.get('publish-error').textContent = ''; this.get('publish-dialog').showModal(); });
    this.get('btn-publish-cancel').addEventListener('click', () => this.get('publish-dialog').close());
    this.get('btn-publish-open').addEventListener('click', () => this.publish());
    this.get('plugin-filter').addEventListener('input', () => this.render());
    this.get('plugin-install-form').addEventListener('submit', event => { event.preventDefault(); this.prepare(this.get('plugin-spec').value.trim()); });
    this.get('btn-plugin-local').addEventListener('click', () => this.prepare(null));
    this.get('btn-plugin-confirm').addEventListener('click', () => this.confirm());
    this.get('btn-plugin-cancel').addEventListener('click', () => this.cancel());
    this.get('plugin-dialog').addEventListener('cancel', event => { event.preventDefault(); this.cancel(); });
  }
}
