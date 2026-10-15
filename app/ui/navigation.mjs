// Match positions use the same character and separator rules as IR search, then
// convert back to DOM text nodes/UTF-16 offsets for the browser selection API.
export function matchPositions(root, offset, query) {
  const positions = [];
  const separator = value => { for (const character of value) positions.push({ character, node: null }); };
  function joined(nodes, joiner) {
    nodes.forEach((node, index) => { if (index) separator(joiner); visit(node); });
  }
  function visit(node) {
    if (node.nodeType === 3) {
      let index = 0;
      for (const character of node.data) {
        positions.push({ character, node, start: index, end: index + character.length });
        index += character.length;
      }
      return;
    }
    const tag = node.tagName?.toLowerCase();
    const children = Array.from(node.childNodes ?? []);
    if (tag === 'br') { separator('\n'); return; }
    if (tag === 'img') { separator(node.alt ?? ''); return; }
    if (tag === 'table') {
      const rows = [];
      for (const child of children) {
        if (child.tagName?.toLowerCase() === 'tr') rows.push(child);
        else for (const row of Array.from(child.childNodes ?? [])) if (row.tagName?.toLowerCase() === 'tr') rows.push(row);
      }
      rows.forEach((row, index) => {
        if (index) separator('\n');
        joined(Array.from(row.childNodes ?? []), node.dataset?.irKind === 'key_value' ? ': ' : '\t');
      });
      return;
    }
    if (tag === 'ul' || tag === 'ol' || tag === 'blockquote') joined(children, '\n');
    else joined(children, '');
  }
  visit(root);
  const wanted = Array.from(String(query).toLowerCase());
  if (!wanted.length) return null;
  const lowered = positions.map(position => Array.from(position.character.toLowerCase())[0]);
  const search = from => {
    for (let index = from; index + wanted.length <= positions.length; index++) {
      if (wanted.every((character, cursor) => lowered[index + cursor] === character)) return index;
    }
    return -1;
  };
  let found = search(Math.max(0, Math.min(Number(offset) || 0, positions.length)));
  if (found < 0) found = search(0);
  if (found < 0) return null;
  const match = positions.slice(found, found + wanted.length).filter(position => position.node);
  if (!match.length) return null;
  return { startNode: match[0].node, startOffset: match[0].start, endNode: match.at(-1).node, endOffset: match.at(-1).end };
}
