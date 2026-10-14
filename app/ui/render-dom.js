//! render/dom：IR → DOM 的唯一前端渲染器（设计规格 §4.3 / §5.2 纪律 2）。
//! 每种块类型都必须有定义好的降级输出；未知块走 Error 形态。

export function renderInline(v) {
  // v: IR Inline（serde tag "kind"）
  const el = document.createElement('span');
  switch (v.kind) {
    case 'text': el.textContent = v.text; return el;
    case 'bold': { const b = document.createElement('strong'); b.append(renderInlineChildren(v.inner)); return b; }
    case 'italic': { const i = document.createElement('em'); i.append(renderInlineChildren(v.inner)); return i; }
    case 'code': { const c = document.createElement('code'); c.className = 'inline-code'; c.textContent = v.text; return c; }
    case 'link': { const a = document.createElement('a'); a.textContent = renderInlineChildren(v.text).textContent; a.href = v.href; a.title = v.href; return a; }
    case 'break': el.append(document.createElement('br')); return el;
    default: el.textContent = JSON.stringify(v); return el;
  }
}

function renderInlineChildren(list) {
  const frag = document.createDocumentFragment();
  for (const item of list ?? []) frag.append(renderInline(item));
  return frag;
}

export function renderBlocks(blocks, target) {
  const frag = document.createDocumentFragment();
  for (const b of blocks ?? []) frag.append(renderBlock(b));
  target.append(frag);
}

function renderBlock(b) {
  switch (b.kind) {
    case 'heading': {
      const h = document.createElement(`h${Math.min(Math.max(b.level, 1), 6)}`);
      h.append(...b.text.map(renderInline));
      return h;
    }
    case 'para': {
      const p = document.createElement('p');
      p.append(...b.text.map(renderInline));
      return p;
    }
    case 'list': {
      const ol = b.ordered;
      const l = document.createElement(ol ? 'ol' : 'ul');
      l.className = 'ir-list';
      for (const item of b.items ?? []) {
        const li = document.createElement('li');
        if (item.kind === 'para') li.append(...item.text.map(renderInline));
        else if (item.kind === 'list') li.append(renderBlock(item));
        else li.append(renderBlock(item));
        l.append(li);
      }
      return l;
    }
    case 'table': return renderTable(b);
    case 'code': {
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
        const td2 = document.createElement('td'); td2.append(renderInline(v));
        tr.append(td1, td2); t.append(tr);
      }
      return t;
    }
    case 'quote': {
      const q = document.createElement('blockquote');
      for (const inner of b.blocks ?? []) q.append(renderBlock(inner));
      return q;
    }
    case 'image': {
      const p = document.createElement('p');
      // v1：无 blob 通道的 GUI 图像由核心经自定义协议暴露；P2 先显示占位
      const span = document.createElement('span');
      span.textContent = `[图片 ${b.alt ?? ''}]`;
      span.style.color = 'var(--muted)';
      p.append(span);
      return p;
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

function renderTable(b) {
  const t = document.createElement('table');
  const aligns = b.align ?? [];
  if (b.head) {
    const thead = document.createElement('thead');
    const tr = document.createElement('tr');
    b.head.forEach((c, i) => {
      const th = document.createElement('th');
      th.append(...c.map(renderInline));
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
      td.append(...c.map(renderInline));
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
