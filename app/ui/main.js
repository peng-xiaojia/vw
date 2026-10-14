// vw GUI 主逻辑：打开 → 分页渲染 → 搜索 → 编辑保存。
// 全部经由 Tauri 命令（无特权路径，与 CLI/MCP 同一 Session API）。

import { renderBlocks } from './render-dom.js';

const invoke = window.__TAURI__?.core?.invoke;

const $ = (id) => document.getElementById(id);
const state = {
  handle: null,
  editable: false,
  loaded: 0,
  total: 0,
  pageSize: 500,
};

function setStatus(text) {
  $('status').textContent = text;
}

async function openPath() {
  const path = $('path').value.trim();
  if (!path) return;
  try {
    setStatus('打开中…');
    const info = await invoke('vw_open', { path, asViewer: null, inner: null });
    state.handle = info.handle;
    state.editable = info.editable;
    state.loaded = 0;
    state.total = 0;
    $('badge').textContent = `${info.viewerId}${info.mime ? ' · ' + info.mime : ''}`;
    $('badge').title = info.sourceDisplay;
    $('content').replaceChildren();
    $('editor').classList.add('hidden');
    $('btn-edit').classList.toggle('hidden', !info.editable);
    $('btn-save').classList.add('hidden');
    $('sidebar').classList.add('hidden');
    await loadMore();
  } catch (e) {
    $('content').replaceChildren();
    const div = document.createElement('div');
    div.className = 'error-block';
    div.textContent = `打开失败：${e}`;
    $('content').append(div);
    setStatus('打开失败');
  }
}

async function loadMore() {
  if (!state.handle) return;
  const resp = await invoke('vw_page', { handle: state.handle, start: state.loaded, count: state.pageSize });
  renderBlocks(resp.blocks, $('content'));
  state.loaded += resp.blocks.length;
  state.total = resp.total ?? state.loaded;
  setStatus(`已加载 ${state.loaded}/${state.total} 块`);
  // 目录/归档成员表：行点击打开成员（§5.1 的 GUI 交互）
  hookMemberRows();
}

function hookMemberRows() {
  // 约定：archive 插件的成员表第一列是成员名；点击后以 --inner 方式打开
  for (const table of $('content').querySelectorAll('table')) {
    if (table.dataset.hooked) continue;
    table.dataset.hooked = '1';
    table.addEventListener('dblclick', (ev) => {
      const tr = ev.target.closest('tr');
      const first = tr?.querySelector('td');
      if (!first) return;
      const name = first.textContent.trim();
      const badge = $('badge').textContent;
      if (!badge.startsWith('archive')) return;
      invoke('vw_open', { path: $('path').value.trim(), asViewer: null, inner: name })
        .then((info) => {
          state.handle = info.handle;
          state.editable = info.editable;
          state.loaded = 0;
          state.total = 0;
          $('badge').textContent = `${info.viewerId}${info.mime ? ' · ' + info.mime : ''}`;
          $('badge').title = info.sourceDisplay;
          $('content').replaceChildren();
          return loadMore();
        })
        .catch((e) => setStatus(`打开成员失败：${e}`));
    });
  }
}

async function doSearch() {
  const q = $('query').value.trim();
  if (!state.handle || !q) return;
  const hits = await invoke('vw_search', { handle: state.handle, query: q });
  $('sidebar').classList.remove('hidden');
  const ul = $('hits');
  ul.replaceChildren();
  for (const h of hits) {
    const li = document.createElement('li');
    li.textContent = h.excerpt;
    li.addEventListener('click', () => scrollToBlock(h.block, h.start));
    ul.append(li);
  }
  setStatus(`搜索「${q}」：${hits.length} 处命中`);
}

function scrollToBlock(blockIdx, offset) {
  const children = $('content').children;
  const el = children[Math.min(blockIdx, children.length - 1)];
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    el.style.background = '#fff8c5';
    setTimeout(() => (el.style.background = ''), 1200);
  }
}

async function enterEditor() {
  if (!state.handle || !state.editable) return;
  const text = await invoke('vw_snapshot', { handle: state.handle });
  $('editor').value = text;
  $('editor').classList.remove('hidden');
  $('content').classList.add('hidden');
  $('btn-save').classList.remove('hidden');
  setStatus('编辑中（浏览器原生 undo/redo；保存走核心冲突检测 + 原子写管线）');
}

async function saveEditor() {
  if (!state.handle) return;
  try {
    const r = await invoke('vw_save', { handle: state.handle, text: $('editor').value });
    setStatus(`已保存 ${r.bytes} 字节${r.backup ? `（备份 ${r.backup}）` : ''}`);
    $('btn-save').classList.add('hidden');
    $('editor').classList.add('hidden');
    $('content').classList.remove('hidden');
    state.loaded = 0;
    $('content').replaceChildren();
    await loadMore();
  } catch (e) {
    setStatus(`保存失败：${e}`);
  }
}

function wire() {
  $('btn-open').addEventListener('click', openPath);
  $('path').addEventListener('keydown', (e) => { if (e.key === 'Enter') openPath(); });
  $('btn-search').addEventListener('click', doSearch);
  $('query').addEventListener('keydown', (e) => { if (e.key === 'Enter') doSearch(); });
  $('btn-edit').addEventListener('click', enterEditor);
  $('btn-save').addEventListener('click', saveEditor);
  // 滚动按需分页（§6.1）
  $('content').addEventListener('scroll', () => {
    const el = $('content');
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40 && state.loaded < state.total) {
      loadMore().catch(() => {});
    }
  });
}

wire();
export { openPath, loadMore, doSearch };
