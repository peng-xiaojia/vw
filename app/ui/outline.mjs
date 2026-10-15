export function renderOutline(items, target, navigate) {
  target.replaceChildren();
  const roots = document.createDocumentFragment();
  const stack = [];
  const toggles = [];
  for (const item of items) {
    const level = Math.max(1, Math.min(6, Number(item.level) || 1));
    while (stack.length && stack.at(-1).level >= level) stack.pop();
    const li = document.createElement('li');
    const row = document.createElement('div'); row.className = 'outline-row';
    const jump = document.createElement('button'); jump.type = 'button'; jump.className = 'outline-link';
    jump.textContent = item.text; jump.title = item.text;
    jump.addEventListener('click', () => navigate(item.block));
    row.append(jump); li.append(row);
    if (stack.length) {
      const parent = stack.at(-1);
      if (!parent.children) {
        parent.children = document.createElement('ul');
        parent.li.append(parent.children);
        const toggle = document.createElement('button');
        toggle.type = 'button'; toggle.className = 'outline-toggle';
        toggle.addEventListener('click', () => setExpanded(toggle, parent.children, toggle.getAttribute('aria-expanded') !== 'true'));
        parent.li.firstChild.prepend(toggle);
        toggles.push([toggle, parent.children]);
        setExpanded(toggle, parent.children, true);
      }
      parent.children.append(li);
    } else roots.append(li);
    stack.push({ level, li, children: null });
  }
  target.append(roots);
  return { setAll(expanded) { for (const [toggle, children] of toggles) setExpanded(toggle, children, expanded); } };
}

function setExpanded(toggle, children, expanded) {
  children.hidden = !expanded;
  toggle.setAttribute('aria-expanded', String(expanded));
  toggle.setAttribute('aria-label', expanded ? '折叠章节' : '展开章节');
  toggle.title = expanded ? '折叠章节' : '展开章节';
  toggle.textContent = expanded ? '▾' : '▸';
}
