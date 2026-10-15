//! render/dom：IR → DOM 的唯一前端渲染器（设计规格 §4.3 / §5.2 纪律 2）。
//! 每种块类型都必须有定义好的降级输出；未知块走 Error 形态。
import { externalUrl, blobUrl } from './security.mjs';
import { renderJsonTree } from './json-tree.mjs';

export function renderInline(v, options = {}) {
  // v: IR Inline（serde tag "kind"）
  const el = document.createElement('span');
  switch (v.kind) {
    case 'text': el.textContent = v.text; return el;
    case 'bold': { const b = document.createElement('strong'); b.append(renderInlineChildren(v.inner, options)); return b; }
    case 'italic': { const i = document.createElement('em'); i.append(renderInlineChildren(v.inner, options)); return i; }
    case 'code': { const c = document.createElement('code'); c.className = 'inline-code'; c.textContent = v.text; return c; }
    case 'link': {
      const url = externalUrl(v.href);
      const a = document.createElement(url ? 'a' : 'span');
      a.append(renderInlineChildren(v.text, options));
      if (url) {
        a.href = url;
        a.title = url;
        a.rel = 'noopener noreferrer';
        a.draggable = false;
        a.addEventListener('click', (event) => {
          event.preventDefault();
          return options.openExternal?.(url);
        });
        a.addEventListener('auxclick', event => event.preventDefault());
        // Disable WebView context-menu navigation; normal clicks use the native opener.
        a.addEventListener('contextmenu', event => event.preventDefault());
      }
      return a;
    }
    case 'break': el.append(document.createElement('br')); return el;
    default: el.textContent = JSON.stringify(v); return el;
  }
}

function renderInlineChildren(list, options = {}) {
  const frag = document.createDocumentFragment();
  for (const item of list ?? []) frag.append(renderInline(item, options));
  return frag;
}

export function renderBlocks(blocks, target, options = {}) {
  const frag = document.createDocumentFragment();
  for (const [index, b] of (blocks ?? []).entries()) {
    const element = renderBlock(b, options);
    element.dataset.block = String((options.start ?? 0) + index);
    frag.append(element);
  }
  target.append(frag);
  applyHeadingFolds(target);
}

function applyHeadingFolds(target) {
  const blocks = [...target.children];
  for (const element of blocks) {
    if (element.dataset.irKind !== 'heading' || element.querySelector('.heading-fold')) continue;
    const button = document.createElement('button'); button.className = 'heading-fold';
    button.type = 'button'; button.setAttribute('aria-label', '折叠章节'); button.setAttribute('aria-expanded', 'true');
    button.textContent = '▾';
    button.addEventListener('click', () => {
      const expanded = button.getAttribute('aria-expanded') !== 'true';
      button.setAttribute('aria-expanded', String(expanded));
      button.setAttribute('aria-label', expanded ? '折叠章节' : '展开章节');
      button.textContent = expanded ? '▾' : '▸';
      updateHeadingFolds(target);
    });
    element.prepend(button);
  }
  updateHeadingFolds(target);
}

function updateHeadingFolds(target) {
  const stack = [];
  for (const element of target.children) {
    if (element.dataset.irKind === 'heading') {
      const level = Number(element.tagName.slice(1));
      while (stack.length && stack.at(-1).level >= level) stack.pop();
      element.hidden = stack.some(item => item.folded);
      stack.push({ level, folded: element.querySelector('.heading-fold')?.getAttribute('aria-expanded') === 'false' });
    } else element.hidden = stack.some(item => item.folded);
  }
}

function renderBlock(b, options = {}) {
  const element = createBlock(b, options);
  element.dataset.irKind = b.kind;
  return element;
}

function createBlock(b, options = {}) {
  switch (b.kind) {
    case 'heading': {
      const h = document.createElement(`h${Math.min(Math.max(b.level, 1), 6)}`);
      h.append(renderInlineChildren(b.text, options));
      return h;
    }
    case 'para': {
      const p = document.createElement('p');
      p.append(renderInlineChildren(b.text, options));
      return p;
    }
    case 'list': {
      const ol = b.ordered;
      const l = document.createElement(ol ? 'ol' : 'ul');
      l.className = 'ir-list';
      for (const item of b.items ?? []) {
        const li = document.createElement('li');
        if (item.kind === 'para') li.append(renderInlineChildren(item.text, options));
        else li.append(renderBlock(item, options));
        l.append(li);
      }
      return l;
    }
    case 'table': return renderTable(b, options);
    case 'code': {
      if (b.lang === 'json') {
        try { return renderJsonTree(b.text); } catch { /* Preserve source if a plugin supplied invalid JSON. */ }
      }
      const pre = document.createElement('pre');
      if (b.lang) pre.dataset.lang = b.lang;
      pre.textContent = b.text;
      return pre;
    }
    case 'key_value': {
      const t = document.createElement('table');
      for (const [k, v] of b.pairs ?? []) {
        const tr = document.createElement('tr');
        const td1 = document.createElement('td'); td1.textContent = k; td1.style.fontWeight = '600';
        const td2 = document.createElement('td'); td2.append(renderInline(v, options));
        tr.append(td1, td2); t.append(tr);
      }
      return t;
    }
    case 'quote': {
      const q = document.createElement('blockquote');
      for (const inner of b.blocks ?? []) q.append(renderBlock(inner, options));
      return q;
    }
    case 'image': {
      const img = document.createElement('img');
      const src = blobUrl(options.handle, b.blob, options.platform ?? globalThis.navigator?.platform ?? '');
      img.alt = b.alt ?? '';
      img.loading = 'lazy';
      img.decoding = 'async';
      img.referrerPolicy = 'no-referrer';
      if (src) img.src = src;
      if (Number.isSafeInteger(b.w) && b.w > 0) img.width = b.w;
      if (Number.isSafeInteger(b.h) && b.h > 0) img.height = b.h;
      img.addEventListener('error', () => {
        const fallback = document.createElement('p');
        fallback.textContent = `图片无法加载${b.alt ? `：${b.alt}` : ''}`;
        fallback.className = 'error-block';
        fallback.dataset.block = img.dataset.block;
        img.replaceWith(fallback);
      });
      return img;
    }
    case 'page_break': {
      const hr = document.createElement('hr');
      hr.className = 'pagebreak';
      return hr;
    }
    case 'canvas': {
      const p = document.createElement('p');
      p.textContent = '[矢量绘图（暂缓类型，占位显示）]';
      p.style.color = 'var(--muted)';
      return p;
    }
    case 'preformatted': {
      const pre = document.createElement('pre');
      pre.textContent = b.text;
      return pre;
    }
    case 'error': {
      const div = document.createElement('div');
      div.className = 'error-block';
      div.textContent = `⚠ ${b.message}`;
      return div;
    }
    default: {
      // 未知块（前向兼容）：与 Block::Error 同形的兜底
      const div = document.createElement('div');
      div.className = 'error-block';
      div.textContent = `⚠ 未知块类型（前向兼容降级）：${JSON.stringify(b).slice(0, 200)}`;
      return div;
    }
  }
}

function renderTable(b, options = {}) {
  const t = document.createElement('table');
  const aligns = b.align ?? [];
  if (b.head) {
    const thead = document.createElement('thead');
    const tr = document.createElement('tr');
    b.head.forEach((c, i) => {
      const th = document.createElement('th');
      th.append(renderInlineChildren(Array.isArray(c) ? c : [c], options));
      applyAlign(th, aligns[i]);
      tr.append(th);
    });
    thead.append(tr);
    t.append(thead);
  }
  const tbody = document.createElement('tbody');
  for (const row of b.rows ?? []) {
    const tr = document.createElement('tr');
    row.forEach((c, i) => {
      const td = document.createElement('td');
      td.append(renderInlineChildren(Array.isArray(c) ? c : [c], options));
      applyAlign(td, aligns[i]);
      tr.append(td);
    });
    tbody.append(tr);
  }
  t.append(tbody);
  return t;
}

function applyAlign(el, align) {
  if (align === 'right') el.style.textAlign = 'right';
  else if (align === 'center') el.style.textAlign = 'center';
}
