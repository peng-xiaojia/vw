import test from 'node:test';
import assert from 'node:assert/strict';
import { renderInline, renderBlocks } from '../ui/render-dom.js';

// Small DOM adapter for the renderer boundary: no network or native window is needed.
class Node {
  constructor(tag = '#fragment') { this.tagName = tag; this.children = []; this.dataset = {}; this.style = {}; this.listeners = {}; this.attributes = {}; this._text = ''; }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text + this.children.map(n => n.textContent).join(''); }
  append(...nodes) { for (const node of nodes) if (node.tagName === '#fragment') this.children.push(...node.children); else this.children.push(node); }
  addEventListener(name, fn) { this.listeners[name] = fn; }
  setAttribute(name, value) { this.attributes[name] = String(value); }
}
globalThis.document = { createElement: tag => new Node(tag), createDocumentFragment: () => new Node() };
const text = value => ({ kind: 'text', text: value });
test('unsafe links never get navigable hrefs; safe links invoke the external opener', async () => {
  const opened = [];
  const bad = renderInline({ kind: 'link', href: 'javascript:alert(1)', text: [text('bad')] }, { openExternal: url => opened.push(url) });
  assert.notEqual(bad.tagName, 'a'); assert.equal(bad.href, undefined);
  const good = renderInline({ kind: 'link', href: 'https://example.com/', text: [text('good')] }, { openExternal: url => opened.push(url) });
  let prevented = false; await good.listeners.click({ preventDefault() { prevented = true; } });
  assert.equal(prevented, true); assert.deepEqual(opened, ['https://example.com/']);
});
test('images use a controlled session URL, never a plugin-supplied URL', () => {
  const target = new Node('section');
  renderBlocks([{ kind: 'image', blob: 'https://evil.test/a', alt: 'Image' }], target, { handle: 'h1', platform: 'Win32', start: 8 });
  const image = target.children[0];
  assert.equal(image.tagName, 'img'); assert.equal(image.src, 'http://vw-blob.localhost/h1/https%3A%2F%2Fevil.test%2Fa');
  assert.equal(image.dataset.block, '8');
});
test('protocol table cells are inline objects and render without map errors', () => {
  const target = new Node('section');
  renderBlocks([{ kind: 'table', head: [text('Name')], rows: [[text('member.txt')]], align: ['left'] }], target);
  assert.equal(target.textContent, 'Namemember.txt');
});
