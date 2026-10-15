import test from 'node:test';
import assert from 'node:assert/strict';
const navigation = await import('../ui/navigation.mjs').catch(() => ({}));
const text = value => ({ nodeType: 3, data: value });
const el = (tag, ...children) => ({ nodeType: 1, tagName: tag.toUpperCase(), childNodes: children, dataset: {} });
function find(root, offset, query) { assert.equal(typeof navigation.matchPositions, 'function'); return navigation.matchPositions(root, offset, query); }
test('search highlighting matches case-insensitive Unicode and character offsets', () => {
  const node = text('😀 first NEEDLE then needle'); const result = find(el('pre', node), 19, 'needle');
  assert.equal(result.startNode, node); assert.equal(result.startOffset, 21); assert.equal(result.endOffset, 27);
});
test('table character offsets include row and cell separators', () => {
  const first = text('needle'); const second = text('needle');
  const root = el('table', el('tbody', el('tr', el('td', first), el('td', second)))); root.dataset.irKind = 'table';
  const result = find(root, 7, 'needle'); assert.equal(result.startNode, second); assert.equal(result.startOffset, 0);
});
test('an inline match may cross nested text runs', () => {
  const first = text('nee'); const second = text('dle'); const root = el('p', first, el('strong', second));
  const result = find(root, 0, 'NEEDLE'); assert.equal(result.startNode, first); assert.equal(result.endNode, second); assert.equal(result.endOffset, 3);
});
