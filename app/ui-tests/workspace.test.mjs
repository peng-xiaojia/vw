import test from 'node:test';
import assert from 'node:assert/strict';
const module = await import('../ui/workspace.mjs').catch(() => ({}));
function create(invoke) { assert.equal(typeof module.WorkspaceTree, 'function'); return new module.WorkspaceTree({ invoke }); }
test('expanding a child lazily loads it while preserving the workspace root', async () => {
  const calls = [];
  const tree = create(async (_, { path }) => { calls.push(path); return { path, parent: path === 'root' ? null : 'root', entries: path === 'root' ? [{ name: 'child', path: 'root/child', isDir: true }] : [] }; });
  await tree.setRoot('root'); assert.deepEqual(calls, ['root']);
  await tree.toggle('root/child'); assert.equal(tree.state.root, 'root'); assert.deepEqual(calls, ['root', 'root/child']);
  await tree.toggle('root/child'); await tree.toggle('root/child'); assert.equal(calls.length, 2);
});
test('failed folder selection leaves the previous workspace usable', async () => {
  const tree = create(async (_, { path }) => { if (path === 'bad') throw new Error('denied'); return { path, parent: null, entries: [] }; });
  await tree.setRoot('good'); assert.equal(await tree.setRoot('bad'), false); assert.equal(tree.state.root, 'good');
});
test('an obsolete folder listing cannot replace the latest root', async () => {
  let resolve; const pending = new Promise(r => { resolve = r; });
  const tree = create(async (_, { path }) => path === 'slow' ? pending : { path, parent: null, entries: [] });
  const slow = tree.setRoot('slow'); await tree.setRoot('fast'); resolve({ path: 'slow', parent: null, entries: [] }); await slow;
  assert.equal(tree.state.root, 'fast');
});

test('Windows extended paths display normally and locate the containing folder', () => {
  assert.equal(module.displayPath('\\\\?\\G:\\auto\\notes\\draft.txt'), 'G:\\auto\\notes\\draft.txt');
  assert.equal(module.parentPath('\\\\?\\G:\\auto\\notes\\draft.txt'), 'G:\\auto\\notes');
  assert.equal(module.parentPath('G:\\draft.txt'), 'G:\\');
  assert.equal(module.pathWithin('\\\\?\\G:\\auto', 'G:\\auto\\notes\\draft.txt'), true);
  assert.equal(module.pathWithin('G:\\', 'G:\\auto\\notes\\draft.txt'), true);
  assert.equal(module.pathWithin('/', '/home/draft.txt'), true);
  assert.equal(module.displayPath('\\\\?\\UNC\\server\\share\\draft.txt'), '\\\\server\\share\\draft.txt');
  assert.equal(module.pathWithin('G:\\auto', 'G:\\other\\draft.txt'), false);
});
