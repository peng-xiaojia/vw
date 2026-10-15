export function renderJsonTree(text) {
  const value = JSON.parse(text);
  const root = document.createElement('div');
  root.className = 'json-tree';
  appendValue(root, value, null, true);
  return root;
}

function appendValue(parent, value, key, expanded = false) {
  const row = document.createElement('div');
  row.className = 'json-row';
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value);
    const details = document.createElement('details');
    details.open = expanded;
    const summary = document.createElement('summary');
    if (key !== null) {
      const label = document.createElement('span'); label.className = 'json-key'; label.textContent = JSON.stringify(key);
      summary.append(label, document.createTextNode(': '));
    }
    const bracket = Array.isArray(value) ? ['[', ']'] : ['{', '}'];
    summary.append(document.createTextNode(`${bracket[0]} ${entries.length} ${Array.isArray(value) ? '项' : '个字段'} ${bracket[1]}`));
    const children = document.createElement('div'); children.className = 'json-children';
    const fill = () => {
      if (children.childElementCount) return;
      for (const [name, child] of entries) appendValue(children, child, Array.isArray(value) ? Number(name) : name);
    };
    if (expanded) fill(); else details.addEventListener('toggle', () => { if (details.open) fill(); }, { once: true });
    details.append(summary, children); row.append(details);
  } else {
    if (key !== null) {
      const label = document.createElement('span'); label.className = 'json-key'; label.textContent = JSON.stringify(key);
      row.append(label, document.createTextNode(': '));
    }
    const scalar = document.createElement('span');
    scalar.className = value === null ? 'json-null' : `json-${typeof value}`;
    scalar.textContent = JSON.stringify(value);
    row.append(scalar);
  }
  parent.append(row);
}
