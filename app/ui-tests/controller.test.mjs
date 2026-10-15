import test from 'node:test';
import assert from 'node:assert/strict';

const controllerModule = await import('../ui/controller.mjs').catch(() => ({}));
const DocumentController = controllerModule.DocumentController;
const deferred = () => { let resolve; let reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const para = (text) => ({ kind: 'para', text: [{ kind: 'text', text }] });
function bridge(overrides = {}) {
  const calls = [];
  let serial = 0;
  const invoke = async (command, args) => {
    calls.push([command, args]);
    if (overrides[command]) return overrides[command](args);
    if (command === 'vw_open') return { handle: `h${++serial}`, path: args.path, viewerId: args.inner ? 'text' : 'text', editable: !args.inner, title: args.path };
    if (command === 'vw_page') return { blocks: [para(`${args.handle}:${args.start}`)], total: 3 };
    if (command === 'vw_outline') return [{ level: 1, text: 'Heading', block: 2 }];
    if (command === 'vw_snapshot') return 'original';
    if (command === 'vw_save') return { bytes: args.text.length, backup: null };
    if (command === 'vw_search') return [];
    if (command === 'vw_close') return;
    throw new Error(`Unexpected bridge command: ${command}`);
  };
  return { invoke, calls };
}
function create(options = {}, overrides = {}) {
  assert.equal(typeof DocumentController, 'function', 'A testable document controller must exist');
  const b = bridge(overrides);
  return { controller: new DocumentController({ invoke: b.invoke, pageSize: 1, ...options }), ...b };
}

test('editable TXT opens directly in edit mode while other text stays in preview', async () => {
  const { controller: c, calls } = create();
  assert.equal(await c.open('notes.txt'), true);
  assert.equal(c.state.editing, true);
  assert.equal(c.state.draft, 'original');
  assert.equal(c.state.dirty, false);
  assert.equal(calls.filter(([name]) => name === 'vw_snapshot').length, 1);
  assert.equal(await c.open('notes.md'), true);
  assert.equal(c.state.editing, false);
});

test('Markdown preview renders the current unsaved draft without saving it', async () => {
  const b = bridge({
    vw_open: ({ path }) => ({ handle: 'markdown', path, viewerId: 'markdown', editable: true }),
    vw_preview_markdown: ({ text }) => ({ blocks: [para(text)], outline: [{ level: 1, text: 'New', block: 0 }] }),
  });
  const c = new DocumentController({ invoke: b.invoke });
  await c.open('note.md'); await c.edit(); c.setDraft('# New');
  await c.preview();
  assert.equal(c.state.editing, false);
  assert.equal(c.state.blocks[0].text[0].text, '# New');
  assert.equal(c.state.outline[0].text, 'New');
  assert.equal(c.state.dirty, true);
  assert.equal(b.calls.filter(([name]) => name === 'vw_save').length, 0);
});

test('failed initial page releases new handle and preserves the current document and draft', async () => {
  const { controller: c, calls } = create({}, { vw_page: ({ handle, start }) => handle === 'h2' ? Promise.reject(new Error('bad page')) : { blocks: [para(`${handle}:${start}`)], total: 3 } });
  await c.open('good'); await c.edit(); c.setDraft('unsaved');
  c.confirmDirty = async () => 'discard';
  assert.equal(await c.open('bad'), false);
  assert.equal(c.state.doc.path, 'good'); assert.equal(c.state.draft, 'unsaved'); assert.equal(c.state.dirty, true);
  assert.deepEqual(calls.filter(([cmd]) => cmd === 'vw_close').map(([, a]) => a.handle), ['h2']);
});

test('cancel dirty guard blocks normal and archive opens and window close', async () => {
  const { controller: c, calls } = create({ confirmDirty: async () => 'cancel' });
  await c.open('good'); await c.edit(); c.setDraft('changed');
  assert.equal(await c.open('other'), false);
  assert.equal(await c.openMember('member.txt'), false);
  assert.equal(await c.close(), false);
  assert.equal(calls.filter(([cmd]) => cmd === 'vw_open').length, 1);
  assert.equal(c.state.dirty, true);
});

test('save decision completes save before opening and releases the old handle', async () => {
  const { controller: c, calls } = create({ confirmDirty: async () => 'save' });
  await c.open('good'); await c.edit(); c.setDraft('changed'); await c.open('next');
  const save = calls.findIndex(([cmd]) => cmd === 'vw_save');
  const open = calls.findIndex(([cmd, a]) => cmd === 'vw_open' && a.path === 'next');
  assert.ok(save >= 0 && save < open);
  assert.equal(c.state.doc.path, 'next');
  assert.ok(calls.some(([cmd, a]) => cmd === 'vw_close' && a.handle === 'h1'));
});

test('racing opens keep the latest document and release the obsolete result', async () => {
  const old = deferred();
  const { controller: c, calls } = create({}, { vw_open: ({ path }) => path === 'slow' ? old.promise : { handle: 'fast', path, viewerId: 'text', editable: true } });
  const slow = c.open('slow'); await Promise.resolve();
  await c.open('fast'); old.resolve({ handle: 'slow', path: 'slow', editable: true, viewerId: 'text' }); await slow;
  assert.equal(c.state.doc.handle, 'fast');
  assert.ok(calls.some(([cmd, a]) => cmd === 'vw_close' && a.handle === 'slow'));
});

test('cancellation releases a late open and preserves the visible document', async () => {
  const pending = deferred();
  const { controller: c, calls } = create({}, { vw_open: ({ path }) => path === 'wait' ? pending.promise : { handle: 'visible', path, viewerId: 'text', editable: true } });
  await c.open('good'); const opening = c.open('wait'); await Promise.resolve(); c.cancelOpen();
  pending.resolve({ handle: 'late', path: 'wait', viewerId: 'text', editable: true }); await opening;
  assert.equal(c.state.doc.handle, 'visible');
  assert.ok(calls.some(([cmd, a]) => cmd === 'vw_close' && a.handle === 'late'));
});

test('simultaneous loadMore requests serialize a page without duplicates', async () => {
  const pending = deferred();
  const { controller: c, calls } = create({}, { vw_page: ({ start }) => start === 0 ? { blocks: [para('zero')], total: 3 } : pending.promise });
  await c.open('good'); const a = c.loadMore(); const b = c.loadMore();
  pending.resolve({ blocks: [para('one')], total: 3 }); await Promise.all([a, b]);
  assert.equal(c.state.loaded, 2); assert.equal(c.state.blocks.length, 2);
  assert.equal(calls.filter(([cmd, a]) => cmd === 'vw_page' && a.start === 1).length, 1);
});

test('a page finishing after a different document opens cannot enter the new document', async () => {
  const pending = deferred();
  const { controller: c } = create({}, { vw_page: ({ handle, start }) => handle === 'h1' && start === 1 ? pending.promise : { blocks: [para(handle)], total: 3 } });
  await c.open('first'); const loading = c.loadMore(); await c.open('second');
  pending.resolve({ blocks: [para('stale')], total: 3 }); await loading;
  assert.equal(c.state.loaded, 1); assert.equal(c.state.blocks[0].text[0].text, 'h2');
});

test('save refreshes page, outline and canonical snapshot and preview remains visible', async () => {
  let saved = false;
  const { controller: c } = create({}, {
    vw_page: () => ({ blocks: [para(saved ? 'new' : 'old')], total: 1 }),
    vw_outline: () => [{ text: saved ? 'New outline' : 'Old outline', level: 1, block: 0 }],
    vw_snapshot: () => saved ? 'canonical new' : 'old',
    vw_save: () => { saved = true; return { bytes: 3 }; },
  });
  await c.open('good'); await c.edit(); c.setDraft('new'); assert.equal(await c.save(), true);
  assert.equal(c.state.blocks[0].text[0].text, 'new'); assert.equal(c.state.outline[0].text, 'New outline');
  assert.equal(c.state.snapshot, 'canonical new'); assert.equal(c.state.dirty, false); assert.equal(c.state.editing, false);
});

test('failed save preserves dirty draft and blocks save-and-open', async () => {
  const { controller: c } = create({ confirmDirty: async () => 'save' }, { vw_save: () => { throw new Error('conflict'); } });
  await c.open('good'); await c.edit(); c.setDraft('draft'); assert.equal(await c.open('other'), false);
  assert.equal(c.state.doc.path, 'good'); assert.equal(c.state.draft, 'draft'); assert.equal(c.state.dirty, true);
});

test('distant navigation loads the actual target and search results are capped', async () => {
  const { controller: c } = create({}, { vw_search: () => Array.from({ length: 700 }, (_, i) => ({ block: i, start: 0, excerpt: `${i}` })) });
  await c.open('good'); assert.equal(await c.ensureBlock(2), true); assert.equal(c.state.loaded, 3);
  await c.search('needle'); assert.equal(c.state.hits.length, 500); assert.equal(c.state.searchLimited, true);
});

test('archive member opens use committed source path and force read-only even if bridge says editable', async () => {
  const { controller: c, calls } = create({}, { vw_open: ({ path }) => ({ handle: path, path, viewerId: 'archive', editable: true }) });
  await c.open('archive.zip'); await c.openMember('member.txt');
  const args = calls.filter(([cmd]) => cmd === 'vw_open').at(-1)[1];
  assert.equal(args.path, 'archive.zip'); assert.equal(args.inner, 'member.txt'); assert.equal(c.state.doc.editable, false);
});

test('search response from a replaced document is discarded', async () => {
  const pending = deferred(); const { controller: c } = create({}, { vw_search: () => pending.promise });
  await c.open('first'); const searching = c.search('x'); await c.open('second');
  pending.resolve([{ block: 0, excerpt: 'stale', start: 0 }]); await searching;
  assert.deepEqual(c.state.hits, []);
});

test('unknown page totals continue until an empty page instead of stopping after the first page', async () => {
  const { controller: c } = create({}, { vw_page: ({ start }) => ({ blocks: start < 2 ? [para(`${start}`)] : [], total: null }) });
  await c.open('stream'); assert.equal(c.state.hasMore, true); await c.loadMore(); assert.equal(c.state.hasMore, true);
  await c.loadMore(); assert.equal(c.state.hasMore, false); assert.equal(c.state.loaded, 2);
});

test('native textarea LF normalization does not dirty or rewrite an unchanged CRLF document', async () => {
  const { controller: c, calls } = create({}, { vw_snapshot: () => 'a\r\nb\r\n' });
  await c.open('windows.txt'); await c.edit(); c.setDraft('a\nb\n');
  assert.equal(c.state.dirty, false); await c.save();
  assert.equal(calls.some(([cmd]) => cmd === 'vw_save'), false);
});
test('editing a CRLF document restores CRLF before sending text to the core', async () => {
  const { controller: c, calls } = create({}, { vw_snapshot: () => 'a\r\nb\r\n' });
  await c.open('windows.txt'); await c.edit(); c.setDraft('changed\nb\n'); await c.save();
  assert.equal(calls.find(([cmd]) => cmd === 'vw_save')[1].text, 'changed\r\nb\r\n');
});
test('mixed line endings cannot be silently rewritten through the native textarea', async () => {
  const { controller: c, calls } = create({}, { vw_snapshot: () => 'a\r\nb\nc' });
  await c.open('mixed.txt'); await c.edit(); c.setDraft('a\nb\nc'); assert.equal(c.state.dirty, false);
  c.setDraft('changed\nb\nc'); assert.equal(await c.save(), false);
  assert.equal(c.state.dirty, true); assert.equal(calls.some(([cmd]) => cmd === 'vw_save'), false);
});

test('distant navigation requests the target page directly and continues from that absolute offset', async () => {
  const { controller: c, calls } = create({}, { vw_page: ({ start }) => ({ blocks: [para(`${start}`)], total: 1000002 }) });
  await c.open('large'); assert.equal(await c.ensureBlock(1000000), true);
  assert.equal(c.state.viewStart, 1000000); assert.equal(c.state.blocks.length, 1); assert.equal(c.state.loaded, 1000001);
  await c.loadMore(); assert.equal(c.state.loaded, 1000002);
  assert.deepEqual(calls.filter(([cmd]) => cmd === 'vw_page').map(([, args]) => args.start), [0, 1000000, 1000001]);
});
test('export is bound to the committed handle and native cancellation is quiet', async () => {
  const { controller: c, calls } = create({}, { vw_export: () => null });
  await c.open('archive.zip'); assert.equal(await c.exportCopy(), false);
  assert.deepEqual(calls.find(([cmd]) => cmd === 'vw_export')[1], { handle: 'h1' }); assert.equal(c.state.error, '');
});
test('failed export leaves the current document usable and reports the native error', async () => {
  const { controller: c } = create({}, { vw_export: () => { throw new Error('exists'); } });
  await c.open('archive.zip'); assert.equal(await c.exportCopy(), false);
  assert.equal(c.state.doc.path, 'archive.zip'); assert.match(c.state.error, /exists/u);
});
