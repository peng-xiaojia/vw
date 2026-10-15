import { DocumentTabs } from './tabs.mjs';
import { WorkspaceTree, displayPath, parentPath, pathWithin } from './workspace.mjs';
import { renderBlocks } from './render-dom.js';
import { externalUrl } from './security.mjs';
import { matchPositions } from './navigation.mjs';
import { bindNativeDrop } from './desktop.mjs';
import { icon, fileIcon } from './icons.mjs';
import { menuStates } from './menu.mjs';
import { PluginCenter } from './plugins.mjs';
import { AssociationDialog } from './associations.mjs';
import { ThemeSettings } from './theme.mjs';
import { bindPaneResizer } from './resizer.mjs';
import { renderOutline } from './outline.mjs';
import { HtmlPreviews } from './html-preview.mjs';
import { CodeEditor } from './code-editor.mjs';
import { registerFormatter, formatDocument } from './formatters.mjs';
import { prettierFormatter } from './plugin-assets/text-formatter.mjs';
registerFormatter('prettier', prettierFormatter);

const $ = id => document.getElementById(id);
const tauri = window.__TAURI__;
const invoke = (command, args = {}) => {
  if (!tauri?.core?.invoke) return Promise.reject(new Error('无法连接桌面服务，请从桌面应用打开查看器。'));
  return tauri.core.invoke(command, args);
};
let renderedDoc = null;
let renderedRevision = -1;
let renderedBlocks = 0;
let renderedViewGeneration = -1;
let renderedViewStart = -1;
let outlineSignature = '';
let outlineTree = null;
let searchSignature = '';
let selectedFile = null;
let routeGeneration = 0;
let closing = false;
let navigationPanel = 'files';
let outlineOpen = false;
const pluginCenter = new PluginCenter({ invoke, onVisibility: visible => {
  $('plugin-center').classList.toggle('hidden', !visible); $('document-area').classList.toggle('hidden', visible);
  $('btn-plugins').classList.toggle('active', visible);
} });
const associationDialog = new AssociationDialog({ invoke });
const themeSettings = new ThemeSettings();

function confirmDirty(doc) {
  return new Promise(resolve => {
    const dialog = $('dirty-dialog');
    $('dirty-description').textContent = `“${doc?.title || doc?.sourceDisplay || doc?.path || '当前文档'}”有未保存修改。`;
    const finish = choice => {
      dialog.removeEventListener('click', click); dialog.removeEventListener('cancel', cancel);
      dialog.close(); resolve(choice);
    };
    const click = event => {
      const choice = event.target.closest('button[data-choice]')?.dataset.choice;
      if (choice) finish(choice);
    };
    const cancel = event => { event.preventDefault(); finish('cancel'); };
    dialog.addEventListener('click', click); dialog.addEventListener('cancel', cancel);
    dialog.showModal(); dialog.querySelector('[data-choice="save"]').focus();
  });
}

const htmlPreviews = new HtmlPreviews({ host: $('html-preview-host'), invoke,
  onError: (tab, error) => tab.controller.fail('无法展示 HTML', error),
  onKey: ({ key, shift }) => {
    if (document.querySelector('dialog[open]') || closing) return;
    if (key === 'tab') controller.cycle(shift ? -1 : 1);
  },
});
const controller = new DocumentTabs({ invoke, confirmDirty, onChange: render,
  beforeActivate: stashView, onRemove: tab => { htmlPreviews.remove(tab); codeEditor.remove(tab); },
});
const codeEditor = new CodeEditor($('editor'), { onChange: value => controller.setDraft(value),
  onFind: replace => showEditorFind(replace), onFormat: () => runFormat(), onZoom: step => themeSettings.adjustFontSize(step) });
const workspace = new WorkspaceTree({ invoke, onChange: renderTree });

async function runFormat() { return controller.format((doc, text) => formatDocument(doc, text, invoke)); }
async function showEditorFind(replace = true) {
  if (!controller.state.doc) return;
  if (replace && !(await controller.edit())) return;
  if (!controller.state.editing && controller.state.doc.viewerId !== 'text') { showNavigation('search'); $('query').focus(); return; }
  $('editor-find').classList.remove('hidden');
  $('replace-row').classList.toggle('hidden', !replace);
  $('edit-query').focus(); $('edit-query').select();
}
function alternateView(asViewer) {
  const doc = controller.state.doc;
  if (doc && !controller.closing && !controller.state.saving && !controller.state.externalOpening) openDocument(doc.sourcePath, { inner: doc.inner, asViewer });
}
function editorOption(option) {
  const enabled = codeEditor.toggle(option);
  document.body.classList.toggle('wrap-document', codeEditor.options.lineWrapping);
  document.querySelector(`[data-action="${option}"]`).setAttribute('aria-checked', String(enabled));
}

function viewKey(state) { return JSON.stringify([state.doc?.handle, state.revision, state.viewGeneration, state.viewStart, state.blocks.length]); }
function stashView(tab) {
  if (!tab) return;
  const view = tab.view;
  Object.assign(view, { key: viewKey(tab.controller.state), scroll: $('content').scrollTop,
    editorScroll: $('editor').scrollTop, selectionStart: $('editor').selectionStart,
    selectionEnd: $('editor').selectionEnd, query: $('query').value, outlineScroll: $('outline-panel').scrollTop,
    outlineSignature, outlineTree,
  });
  view.content = document.createDocumentFragment(); view.content.append(...$('content').childNodes);
  view.outline = document.createDocumentFragment(); view.outline.append(...$('outline').childNodes);
}

function renderTabs() {
  const target = $('document-tabs');
  const existing = new Map([...target.children].map(node => [node.dataset.tabId, node]));
  for (const tab of controller.tabs) {
    let item = existing.get(tab.id);
    if (!item) {
      item = document.createElement('div'); item.className = 'document-tab'; item.dataset.tabId = tab.id;
      const activate = document.createElement('button'); activate.type = 'button'; activate.className = 'tab-label'; activate.setAttribute('role', 'tab');
      activate.id = `tab-${tab.id}`;
      activate.addEventListener('click', () => { if (!pluginCenter.busy) pluginCenter.hide(); controller.activate(tab.id); });
      const close = document.createElement('button'); close.type = 'button'; close.className = 'tab-close'; close.textContent = '×';
      close.addEventListener('click', () => controller.close(tab.id));
      item.addEventListener('auxclick', event => { if (event.button === 1) { event.preventDefault(); controller.close(tab.id); } });
      item.append(activate, close); target.append(item);
    }
    existing.delete(tab.id);
    const state = tab.controller.state;
    const title = (state.doc?.title || displayPath(tab.path).split(/[\\/]/u).at(-1)) + (state.doc?.viewerId === 'hex' ? ' · 二进制' : '');
    const active = controller.active === tab;
    item.title = displayPath(tab.path); item.classList.toggle('active', active);
    const [activate, close] = item.children;
    activate.textContent = `${state.opening || !state.doc ? '◌ ' : state.dirty ? '● ' : ''}${title}`;
    activate.setAttribute('aria-selected', String(active)); activate.tabIndex = active ? 0 : -1;
    close.setAttribute('aria-label', `关闭 ${title}${state.dirty ? '（未保存）' : ''}`);
  }
  for (const node of existing.values()) node.remove();
  const active = controller.active?.id ?? '';
  if (target.dataset.active !== active) {
    target.dataset.active = active;
    target.querySelector('.active')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
}

function showNavigation(name) {
  if (pluginCenter.visible && !pluginCenter.busy) pluginCenter.hide();
  navigationPanel = name;
  $('sidebar').classList.remove('hidden');
  $('sidebar-resizer').classList.remove('hidden');
  $('panel-files').classList.toggle('hidden', name !== 'files');
  $('panel-search').classList.toggle('hidden', name !== 'search');
  $('sidebar-title').textContent = name === 'search' ? '搜索' : '文件';
  syncSidebar();
}

function syncOutline() {
  const visible = outlineOpen && Boolean(controller.state.doc);
  $('outline-panel').classList.toggle('hidden', !visible);
  $('outline-resizer').classList.toggle('hidden', !visible);
  $('btn-outline').classList.toggle('active', visible);
  $('btn-outline').setAttribute('aria-expanded', String(visible));
}

function toggleOutline() { outlineOpen = !outlineOpen; syncOutline(); }

function syncSidebar() {
  $('sidebar-resizer').classList.toggle('hidden', $('sidebar').classList.contains('hidden'));
  for (const button of $('activity-bar').querySelectorAll('[data-panel]')) {
    const active = !$('sidebar').classList.contains('hidden') && button.dataset.panel === navigationPanel;
    button.classList.toggle('active', active); button.setAttribute('aria-expanded', String(active));
  }
}

function toggleNavigation(name = navigationPanel) {
  if (name === navigationPanel && !$('sidebar').classList.contains('hidden')) { $('sidebar').classList.add('hidden'); syncSidebar(); }
  else showNavigation(name);
}

function render(state) {
  renderTabs();
  for (const [action, enabled] of menuStates(state)) {
    for (const button of document.querySelectorAll(`#top-menu [data-action="${action}"]`)) button.disabled = !enabled;
  }
  const doc = state.doc;
  const changed = renderedDoc !== doc;
  if (changed || renderedRevision !== state.revision || renderedViewGeneration !== state.viewGeneration || renderedViewStart !== state.viewStart || renderedBlocks > state.blocks.length) {
    $('content').replaceChildren(); renderedBlocks = 0; renderedRevision = state.revision; renderedViewGeneration = state.viewGeneration; renderedViewStart = state.viewStart;
  }
  if (changed && doc) {
    selectedFile = doc.sourcePath;
    $('content').scrollTop = 0; $('query').value = controller.active?.view.query ?? state.query; renderTree(workspace.state);
  }
  const view = controller.active?.view;
  if (changed && view?.content && view.key === viewKey(state)) {
    $('content').append(view.content); renderedBlocks = state.blocks.length;
    $('outline').replaceChildren(view.outline); outlineSignature = view.outlineSignature; outlineTree = view.outlineTree;
  }
  renderedDoc = doc;
  document.body.classList.toggle('text-document', Boolean(doc?.viewerId === 'text' && /\.txt$/iu.test(doc.sourcePath)));
  if (doc?.viewerId === 'html') {
    renderedBlocks = state.blocks.length;
  } else if (doc && renderedBlocks < state.blocks.length) {
    renderBlocks(state.blocks.slice(renderedBlocks), $('content'), {
      start: state.viewStart + renderedBlocks, handle: doc.handle, platform: navigator.platform,
      openExternal: async url => {
        const allowed = externalUrl(url);
        if (!allowed) return;
        try { await invoke('vw_open_external', { url: allowed }); }
        catch (error) { controller.fail('无法打开链接', error); }
      },
    });
    renderedBlocks = state.blocks.length;
  }
  if ($('editor').value !== state.draft) $('editor').value = state.draft;
  $('editor').disabled = state.saving || state.opening || state.externalOpening || controller.closing;
  $('content').classList.toggle('hidden', state.editing || doc?.viewerId === 'html' || doc?.viewerId === 'text');
  codeEditor.sync(controller.active, { ...state, locked: controller.closing || state.externalOpening });
  $('editor-find').classList.toggle('hidden', changed || (!state.editing && doc?.viewerId !== 'text') || $('editor-find').classList.contains('hidden'));
  $('edit-replace').disabled = $('edit-replace-all').disabled = !state.editing || state.saving || controller.closing;
  $('mode-binary').disabled = !doc || doc.viewerId === 'dir' || state.saving || controller.closing;
  $('mode-binary').textContent = doc?.viewerId === 'hex' ? '默认视图' : '二进制';
  const textCapable = doc && (doc.editOptions?.kind === 'text' || ['text', 'markdown', 'html', 'json', 'csv'].includes(doc.viewerId));
  $('text-tools').classList.toggle('hidden', !textCapable);
  $('current-encoding').textContent = doc?.editOptions?.encoding ? `${doc.editOptions.encoding.toUpperCase()}${doc.editOptions.has_bom ? ' BOM' : ''}` : '编码未确认';
  $('save-encoding').value = state.saveEncoding || ''; $('line-ending').value = state.lineEnding;
  for (const id of ['save-encoding', 'line-ending']) $(id).disabled = !doc?.editable || state.saving || controller.closing;
  $('reopen-encoding').disabled = !textCapable || doc.inner !== null || state.saving || controller.closing;
  htmlPreviews.show(controller.active, state);
  if (changed && view) {
    $('content').scrollTop = view.scroll ?? 0; $('editor').scrollTop = view.editorScroll ?? 0;
    $('editor').setSelectionRange(view.selectionStart ?? 0, view.selectionEnd ?? 0);
  }
  $('empty-state').classList.toggle('hidden', Boolean(doc)); $('document-body').classList.toggle('hidden', !doc);
  $('page-tools').classList.toggle('hidden', !doc || state.editing || doc.viewerId === 'html'); $('btn-more').classList.toggle('hidden', !state.hasMore);
  $('btn-more').disabled = state.loading || state.saving;
  $('btn-more').textContent = state.loading ? '加载中…' : state.error ? '重试加载' : '加载更多';
  $('page-progress').textContent = doc ? controller.pageStatus() : '';
  const sourceModes = doc?.viewerId === 'markdown' || doc?.viewerId === 'html';
  $('btn-edit').textContent = sourceModes ? '源码模式' : state.dirty ? '继续编辑' : '编辑文档';
  $('btn-preview').textContent = sourceModes ? '展示模式' : '预览文档';
  $('document-mode').classList.toggle('hidden', !sourceModes || !doc.editable);
  $('mode-preview').setAttribute('aria-pressed', String(!state.editing));
  $('mode-source').setAttribute('aria-pressed', String(state.editing));
  $('mode-source').disabled = !doc?.editable || state.saving || state.opening || state.externalOpening || controller.closing;
  $('mode-preview').disabled = !doc || state.saving || state.opening || state.externalOpening || controller.closing;
  const externalFlags = Object.fromEntries(menuStates(state));
  $('btn-other-app').disabled = controller.closing || !externalFlags.otherApp;
  $('btn-terminal').disabled = controller.closing || !externalFlags.terminal;
  for (const id of ['mode-binary', 'edit-replace', 'edit-replace-all', 'save-encoding', 'line-ending', 'reopen-encoding']) {
    if (state.externalOpening) $(id).disabled = true;
  }
  $('btn-save').innerHTML = state.saving ? '保存中…' : '保存 <kbd>Ctrl+S</kbd>';
  for (const id of ['btn-file', 'btn-folder']) $(id).disabled = state.saving;
  $('btn-search').disabled = !doc || state.saving;
  $('btn-export').textContent = state.exporting ? '导出中…' : '导出副本…';
  $('badge').textContent = doc ? `${doc.viewerId}${doc.editable ? '' : ' · 只读'}${state.dirty ? ' · 未保存' : ''}` : '未打开文档';
  $('badge').title = doc?.mime || '';
  $('document-info').textContent = doc ? `${displayPath(doc.sourceDisplay || doc.sourcePath)}${doc.inner ? ` › ${doc.inner}` : ''}` : '选择文件开始阅读';
  $('document-info').title = $('document-info').textContent;
  $('window-title').textContent = doc ? doc.title || displayPath(doc.path) : '文件查看器';
  $('btn-outline').disabled = !doc;
  syncOutline();
  document.title = doc ? `${state.dirty ? '* ' : ''}${doc.title || doc.path} · vw` : 'vw · 文件查看器';
  $('status').textContent = state.status; $('error-banner').classList.toggle('hidden', !state.error); $('error-text').textContent = state.error;
  $('document-area').setAttribute('aria-busy', String(state.opening || state.saving));
  $('empty-state').querySelector('h1').textContent = !doc && state.opening ? '正在打开…' : '打开文件，开始阅读';

  const outlineKey = JSON.stringify([doc?.handle, state.outline]);
  if (outlineSignature !== outlineKey) {
    outlineSignature = outlineKey;
    $('outline-empty').classList.toggle('hidden', state.outline.length > 0);
    $('outline-empty').textContent = doc ? '此文档没有目录。' : '打开文档后显示目录。';
    outlineTree = renderOutline(state.outline, $('outline'), block => navigateBlock(block, 0));
  }
  if (changed) $('outline-panel').scrollTop = view?.outlineScroll ?? 0;
  const searchKey = JSON.stringify([doc?.handle, state.hits]);
  if (searchSignature !== searchKey) {
    searchSignature = searchKey; $('hits').replaceChildren();
    for (const hit of state.hits) {
      const li = document.createElement('li'); const button = document.createElement('button');
      button.textContent = hit.excerpt; button.title = `第 ${hit.block + 1} 块 · ${hit.excerpt}`;
      button.addEventListener('click', () => navigateBlock(hit.block, hit.start, state.query)); li.append(button); $('hits').append(li);
    }
  }
  $('search-summary').textContent = state.searching ? '搜索中…' : state.query ? `${state.hits.length} 处命中${state.searchLimited ? ' · 最多显示 500 处，请缩小范围' : ''}` : '输入关键词搜索当前文档。';
}

function renderTree(state) {
  $('workspace-path').textContent = state.root ? displayPath(state.root) : '尚未选择工作文件夹'; $('workspace-path').title = $('workspace-path').textContent;
  $('tree-message').textContent = state.error || (state.busy ? '正在读取文件夹…' : !state.root ? '选择文件夹后，点击文件打开；点击文件夹展开。' : '');
  $('btn-parent').disabled = !state.parent || state.busy; $('btn-refresh').disabled = !state.root || state.busy;
  $('file-tree').replaceChildren();
  function appendChildren(path, target) {
    const node = state.nodes.get(path);
    for (const entry of node?.entries ?? []) {
      const li = document.createElement('li'); const button = document.createElement('button');
      const pending = state.nodes.get(entry.path)?.loading;
      const chevron = document.createElement('span'); chevron.className = 'tree-chevron'; chevron.setAttribute('aria-hidden', 'true');
      chevron.textContent = entry.isDir ? pending ? '…' : state.expanded.has(entry.path) ? '▾' : '▸' : '';
      const label = document.createElement('span'); label.className = 'tree-name'; label.textContent = entry.name;
      button.append(chevron, icon(fileIcon(entry, state.expanded.has(entry.path))), label);
      button.title = displayPath(entry.path); button.classList.toggle('selected', pathWithin(entry.path, selectedFile) && pathWithin(selectedFile, entry.path));
      if (entry.isDir) button.setAttribute('aria-expanded', String(state.expanded.has(entry.path)));
      button.disabled = Boolean(pending);
      button.addEventListener('click', () => entry.isDir ? workspace.toggle(entry.path) : openDocument(entry.path)); li.append(button);
      if (entry.isDir && state.expanded.has(entry.path)) { const children = document.createElement('ul'); appendChildren(entry.path, children); li.append(children); }
      target.append(li);
    }
  }
  if (state.root) appendChildren(state.root, $('file-tree'));
}

async function openDocument(path, options) {
  ++routeGeneration;
  const opened = await controller.open(path, options);
  if (opened && pluginCenter.visible && !pluginCenter.busy) pluginCenter.hide();
  if (opened && controller.state.doc?.sourcePath === path && !pathWithin(workspace.state.root, path)) { const parent = parentPath(path); if (parent) await workspace.setRoot(parent); }
  return opened;
}

async function routePath(path) {
  if (!path) return;
  const generation = ++routeGeneration;
  // A directory listing is also the native path-type check. Files get the regular open guard.
  try {
    await invoke('vw_list_dir', { path });
    if (generation !== routeGeneration) return;
    await workspace.setRoot(path); showNavigation('files');
  } catch { if (generation === routeGeneration) await openDocument(path); }
}

async function pickPath(directory) {
  const generation = ++routeGeneration;
  try {
    const selected = directory ? await invoke('vw_pick_path', { directory: true }) : await invoke('vw_pick_files');
    if (generation !== routeGeneration) return;
    if (directory && selected) { await workspace.setRoot(selected); showNavigation('files'); }
    else if (Array.isArray(selected)) for (const path of selected) await openDocument(path);
  } catch (error) { controller.fail('无法选择路径', error); }
}

async function navigateBlock(index, offset = 0, query = '') {
  const tab = controller.active;
  if (!tab || !(await tab.controller.preview()) || controller.active !== tab) return;
  if (!(await tab.controller.ensureBlock(index)) || controller.active !== tab) return;
  if (tab.controller.state.doc?.viewerId === 'text') {
    const state = tab.controller.state;
    const relative = index - state.viewStart;
    const before = state.blocks.slice(0, relative).map(block => block.text ?? '').join('');
    const inBlock = Array.from(state.blocks[relative]?.text ?? '').slice(0, offset).join('');
    codeEditor.navigate((before + inBlock).replace(/\r\n?/gu, '\n').length, query.length);
    return;
  }
  const element = $('content').querySelector(`[data-block="${index}"]`);
  if (!element) return;
  if (element.hidden) {
    for (const folded of $('content').querySelectorAll('.heading-fold[aria-expanded="false"]')) folded.click();
  }
  for (const marked of $('content').querySelectorAll('.active-block')) marked.classList.remove('active-block');
  element.classList.add('active-block');
  if (query) {
    const match = matchPositions(element, offset, query);
    if (match) {
      const range = document.createRange(); range.setStart(match.startNode, match.startOffset); range.setEnd(match.endNode, match.endOffset);
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    }
  }
  element.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

async function incomingPaths(paths) {
  if (!Array.isArray(paths)) return;
  for (const path of paths.filter(path => typeof path === 'string' && path.trim())) await routePath(path);
}

function wire() {
  for (const menu of document.querySelectorAll('.top-menu')) menu.addEventListener('toggle', () => {
    if (menu.open) for (const other of document.querySelectorAll('.top-menu')) if (other !== menu) other.open = false;
  });
  document.addEventListener('pointerdown', event => { if (!event.target.closest('.top-menu')) for (const menu of document.querySelectorAll('.top-menu')) menu.open = false; });
  $('top-menu').addEventListener('click', event => {
    const button = event.target.closest('button[data-action]');
    if (!button) return;
    button.closest('.top-menu').open = false;
    if (button.disabled) return;
    const action = button.dataset.action;
    if (action === 'sidebar') toggleNavigation();
    if (action === 'files' || action === 'search') { showNavigation(action); if (action === 'search') $('query').focus(); }
    if (action === 'outline') toggleOutline();
    if (action === 'plugins') pluginCenter.show();
    if (action === 'associations') associationDialog.show();
    if (action === 'about') $('about-dialog').showModal();
    if (action === 'appearance') $('appearance-dialog').showModal();
    if (action === 'format') runFormat();
    if (action === 'replace') showEditorFind();
    if (action === 'binary') alternateView('hex');
    if (action === 'normal') alternateView(null);
    if (action === 'text') alternateView('text');
    if (action === 'otherApp') controller.openElsewhere('other_app');
    if (action === 'terminal') controller.openElsewhere('terminal');
    if (['lineNumbers', 'lineWrapping'].includes(action)) editorOption(action);
    if (action === 'close') { ++routeGeneration; controller.close(); }
    if (action === 'quit') tauri?.window?.getCurrentWindow().close().catch(error => controller.fail('无法关闭窗口', error));
  });
  const currentWindow = tauri?.window?.getCurrentWindow();
  $('btn-minimize').addEventListener('click', () => currentWindow?.minimize().catch(error => controller.fail('最小化失败', error)));
  $('btn-maximize').addEventListener('click', () => currentWindow?.toggleMaximize().catch(error => controller.fail('最大化失败', error)));
  $('btn-window-close').addEventListener('click', () => currentWindow?.close().catch(error => controller.fail('无法关闭窗口', error)));
  $('window-title').addEventListener('dblclick', () => currentWindow?.toggleMaximize().catch(error => controller.fail('最大化失败', error)));
  $('btn-file').addEventListener('click', () => pickPath(false)); $('btn-folder').addEventListener('click', () => pickPath(true));
  $('btn-parent').addEventListener('click', () => workspace.parent()); $('btn-refresh').addEventListener('click', () => workspace.refresh());
  for (const button of $('activity-bar').querySelectorAll('[data-panel]')) button.addEventListener('click', () => { toggleNavigation(button.dataset.panel); if (button.dataset.panel === 'search' && !$('sidebar').classList.contains('hidden')) $('query').focus(); });
  $('btn-hide-sidebar').addEventListener('click', () => { $('sidebar').classList.add('hidden'); syncSidebar(); $('btn-sidebar').focus(); });
  for (const [id, name] of [['btn-sidebar', 'folder'], ['btn-outline', 'list-tree'], ['btn-find', 'search'], ['btn-plugins', 'blocks']]) $(id).append(icon(name));
  $('btn-outline').addEventListener('click', toggleOutline);
  $('outline-collapse-all').addEventListener('click', () => outlineTree?.setAll(false));
  $('outline-expand-all').addEventListener('click', () => outlineTree?.setAll(true));
  pluginCenter.wire();
  associationDialog.wire();
  themeSettings.wire();
  bindPaneResizer($('sidebar-resizer'), $('sidebar'), { key: 'vw.sidebar.width', side: 'left', min: 170, max: 520 });
  bindPaneResizer($('outline-resizer'), $('outline-panel'), { key: 'vw.outline.width', side: 'right', min: 170, max: 520 });
  $('search-form').addEventListener('submit', event => { event.preventDefault(); showNavigation('search'); controller.search($('query').value); });
  $('query').addEventListener('input', () => { if (!$('query').value) controller.search(''); });
  $('btn-edit').addEventListener('click', async () => { if (await controller.edit()) codeEditor.focus(); });
  $('btn-preview').addEventListener('click', () => controller.preview()); $('btn-save').addEventListener('click', () => controller.save());
  $('mode-source').addEventListener('click', async () => { if (await controller.edit()) codeEditor.focus(); });
  $('mode-preview').addEventListener('click', () => controller.preview());
  $('mode-binary').addEventListener('click', () => alternateView(controller.state.doc?.viewerId === 'hex' ? null : 'hex'));
  $('btn-other-app').addEventListener('click', () => controller.openElsewhere('other_app'));
  $('btn-terminal').addEventListener('click', () => controller.openElsewhere('terminal'));
  const codecs = [['utf-8', 'UTF-8'], ['utf-8-bom', 'UTF-8 BOM'], ['utf-16le', 'UTF-16 LE'], ['utf-16be', 'UTF-16 BE'], ['gbk', 'GBK'], ['gb18030', 'GB18030'], ['shift_jis', 'Shift-JIS'], ['big5', 'Big5'], ['windows-1252', 'Windows-1252']];
  for (const id of ['save-encoding', 'reopen-encoding']) for (const [value, label] of codecs) $(id).add(new Option(label, value));
  $('save-encoding').addEventListener('change', event => controller.setTextOptions({ encoding: event.target.value }));
  $('line-ending').addEventListener('change', event => controller.setTextOptions({ lineEnding: event.target.value }));
  $('reopen-encoding').addEventListener('change', async event => { const encoding = event.target.value; event.target.value = ''; if (encoding) await controller.reopenEncoding(encoding); });
  $('edit-find-close').addEventListener('click', () => { $('editor-find').classList.add('hidden'); codeEditor.focus(); });
  for (const [id, replace, all] of [['edit-find-next', false, false], ['edit-replace', true, false], ['edit-replace-all', true, true]]) $(id).addEventListener('click', () => {
    if (replace && (!controller.state.editing || controller.state.saving || controller.state.externalOpening || controller.closing)) return;
    const result = codeEditor.find($('edit-query').value, $('edit-case').checked, replace ? $('edit-replacement').value : null, all);
    $('edit-find-result').textContent = all ? `已替换 ${result.count} 处 · 尚未保存` : result.count ? replace ? '已替换 1 处' : '已定位匹配项' : '未找到匹配项';
  });
  $('edit-query').addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); $('edit-find-next').click(); } });
  for (const option of ['lineNumbers', 'lineWrapping']) document.querySelector(`[data-action="${option}"]`).setAttribute('aria-checked', String(codeEditor.options[option]));
  document.body.classList.toggle('wrap-document', codeEditor.options.lineWrapping);
  $('btn-export').addEventListener('click', () => controller.exportCopy());
  $('editor').addEventListener('input', event => controller.setDraft(event.target.value));
  for (const area of [$('content'), $('editor')]) area.addEventListener('wheel', event => {
    if (!event.ctrlKey || event.deltaY === 0) return;
    event.preventDefault();
    themeSettings.adjustFontSize(-event.deltaY);
  }, { passive: false });
  $('btn-cancel').addEventListener('click', () => { ++routeGeneration; controller.cancelOpen(); }); $('btn-more').addEventListener('click', () => controller.loadMore());
  $('btn-dismiss-error').addEventListener('click', () => { controller.state.error = ''; controller.emit(); });
  $('content').addEventListener('scroll', () => {
    const area = $('content');
    if (area.scrollTop + area.clientHeight >= area.scrollHeight - 100 && controller.state.hasMore && !controller.state.error) controller.loadMore();
  });
  $('content').addEventListener('dblclick', event => {
    if (controller.state.doc?.viewerId !== 'archive') return;
    const row = event.target.closest('tbody tr'); const member = row?.querySelector('td')?.textContent;
    if (member) { ++routeGeneration; controller.openMember(member); }
  });
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape') { for (const menu of document.querySelectorAll('.top-menu')) menu.open = false; $('editor-find').classList.add('hidden'); return; }
    if (!document.querySelector('dialog[open]') && event.altKey && event.shiftKey && event.key.toLowerCase() === 'f') { event.preventDefault(); runFormat(); return; }
    if (!document.querySelector('dialog[open]') && event.altKey && event.key.toLowerCase() === 'z') { event.preventDefault(); editorOption('lineWrapping'); return; }
    if (document.querySelector('dialog[open]') || !(event.ctrlKey || event.metaKey) || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === 'tab') { event.preventDefault(); controller.cycle(event.shiftKey ? -1 : 1); }
    if (key === 'o') { event.preventDefault(); pickPath(event.shiftKey); }
    if (key === 's') { event.preventDefault(); controller.save(); }
    if (key === 'h') { event.preventDefault(); showEditorFind(); }
    if (key === 'f') { event.preventDefault(); if (controller.state.editing || controller.state.doc?.viewerId === 'text') showEditorFind(false); else { showNavigation('search'); $('query').focus(); $('query').select(); } }
    if (key === 'b') { event.preventDefault(); toggleNavigation(); }
    if (key === 'w') { event.preventDefault(); ++routeGeneration; controller.close(); }
  });
  document.addEventListener('dragover', event => event.preventDefault()); document.addEventListener('drop', event => event.preventDefault());
  $('document-tabs').addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    if (event.key === 'Home') controller.activate(controller.tabs[0]?.id);
    else if (event.key === 'End') controller.activate(controller.tabs.at(-1)?.id);
    else controller.cycle(event.key === 'ArrowLeft' ? -1 : 1);
    $('document-tabs').querySelector('[aria-selected="true"]')?.focus();
  });
  window.addEventListener('beforeunload', event => { if (controller.tabs.some(tab => tab.controller.state.dirty) && !closing) { event.preventDefault(); event.returnValue = ''; } });
}

async function wireDesktop() {
  if (!tauri) { controller.fail('桌面服务不可用', '请启动桌面应用'); return; }
  try {
    if (tauri.event?.listen) await tauri.event.listen('vw-menu', event => {
      if (document.querySelector('dialog[open]')) return;
      const action = event.payload;
      if (['file', 'folder', 'save', 'export', 'edit', 'preview', 'cancel'].includes(action)) { const button = $(`btn-${action}`); if (!button.disabled) button.click(); }
      if (action === 'sidebar') toggleNavigation();
      if (action === 'outline') toggleOutline();
      if (['files', 'search'].includes(action)) { showNavigation(action); if (action === 'search') { $('query').focus(); $('query').select(); } }
      if (action === 'plugins') pluginCenter.show();
      if (action === 'associations') associationDialog.show();
      if (action === 'about') $('about-dialog').showModal();
      if (action === 'appearance') $('appearance-dialog').showModal();
      if (action === 'format') runFormat();
      if (action === 'replace') showEditorFind();
      if (action === 'binary') alternateView('hex');
      if (action === 'normal') alternateView(null);
      if (action === 'text') alternateView('text');
      if (['lineNumbers', 'lineWrapping'].includes(action)) editorOption(action);
      if (action === 'close') { ++routeGeneration; controller.close(); }
      if (action === 'quit') tauri.window?.getCurrentWindow().close().catch(error => controller.fail('无法关闭窗口', error));
    });
  } catch (error) { controller.fail('原生菜单初始化失败', error); }
  try {
    const currentWindow = tauri.window?.getCurrentWindow();
    if (currentWindow) {
      await currentWindow.onCloseRequested(async event => {
        event.preventDefault(); if (closing) return; closing = true; ++routeGeneration;
        try { if (await controller.closeAll()) await currentWindow.destroy(); else closing = false; }
        catch (error) { closing = false; controller.fail('无法关闭窗口', error); }
      });
    }
  } catch (error) { controller.fail('窗口关闭保护初始化失败', error); }
  try {
    await bindNativeDrop(tauri, {
      onPaths: incomingPaths,
      onHover: active => $('drop-overlay').classList.toggle('hidden', !active),
      onError: error => controller.fail('拖放打开失败', error),
    });
  } catch (error) { controller.fail('拖放初始化失败', error); }
  try {
    if (tauri.event?.listen) await tauri.event.listen('vw-open-paths', event => incomingPaths(event.payload));
    await incomingPaths(await invoke('vw_startup_paths'));
  } catch (error) { controller.fail('启动文件路由初始化失败', error); }
}

wire(); render(controller.state); renderTree(workspace.state); wireDesktop();
export { controller, workspace, pluginCenter, codeEditor, openDocument, routePath, navigateBlock };
