import test from 'node:test';
import assert from 'node:assert/strict';
const { DocumentTabs } = await import('../ui/tabs.mjs').catch(() => ({}));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { resolve, promise }; };
function setup(overrides = {}, choice = async () => 'cancel') {
  const calls = [];
  let serial = 0;
  const invoke = async (command, args) => {
    calls.push([command, args]);
    if (overrides[command]) return overrides[command](args);
    if (command === 'vw_open') return { handle: `h${++serial}`, path: args.path, viewerId: 'text', editable: true };
    if (command === 'vw_page') return { blocks: [{ kind: 'preformatted', text: 'original' }], total: 1 };
    if (command === 'vw_outline') return [];
    if (command === 'vw_snapshot') return 'original';
    if (command === 'vw_save') return { bytes: args.text.length };
    if (command === 'vw_close') return;
    if (command === 'vw_search') return [];
    throw new Error(command);
  };
  return { tabs: new DocumentTabs({ invoke, confirmDirty: choice }), calls };
}

test('tabs preserve drafts, edit modes and independent state while opening and switching', async () => {
  const { tabs, calls } = setup();
  await tabs.open('a.txt'); const first = tabs.active;
  tabs.setDraft('unsaved a'); first.view = { scrollTop: 123 };
  await tabs.open('b.txt'); const second = tabs.active;
  tabs.setDraft('unsaved b');
  assert.equal(tabs.tabs.length, 2);
  tabs.activate(first.id);
  assert.equal(tabs.state.draft, 'unsaved a'); assert.equal(tabs.state.editing, true);
  assert.equal(tabs.active.view.scrollTop, 123);
  tabs.activate(second.id); assert.equal(tabs.state.draft, 'unsaved b');
  assert.equal(calls.some(([name]) => name === 'vw_close'), false);
});

test('opening an existing Windows path activates its tab without dropping a dirty draft', async () => {
  const { tabs, calls } = setup();
  await tabs.open('C:\\Notes\\a.txt'); const first = tabs.active;
  tabs.setDraft('changed'); await tabs.open('b.txt');
  await tabs.open('c:/notes/a.txt');
  assert.equal(tabs.active, first); assert.equal(tabs.state.draft, 'changed');
  assert.equal(tabs.tabs.length, 2);
  assert.equal(calls.filter(([name]) => name === 'vw_open').length, 2);
});

test('closing an inactive dirty tab can be cancelled without changing the active document', async () => {
  const { tabs, calls } = setup();
  await tabs.open('a.txt'); tabs.setDraft('draft'); const first = tabs.active;
  await tabs.open('b.txt'); const second = tabs.active;
  assert.equal(await tabs.close(first.id), false);
  assert.equal(tabs.active, second); assert.equal(tabs.tabs.length, 2);
  assert.equal(calls.some(([name]) => name === 'vw_close'), false);
});

test('closeAll checks inactive dirty tabs and only releases handles after every guard succeeds', async () => {
  let choice = 'cancel';
  const { tabs, calls } = setup({}, async () => choice);
  await tabs.open('a.txt'); tabs.setDraft('a draft'); await tabs.open('b.txt');
  assert.equal(await tabs.closeAll(), false); assert.equal(tabs.tabs.length, 2);
  assert.equal(calls.some(([name]) => name === 'vw_close'), false);
  choice = 'save'; assert.equal(await tabs.closeAll(), true); assert.equal(tabs.tabs.length, 0);
  assert.equal(calls.filter(([name]) => name === 'vw_save').length, 1);
  assert.equal(calls.filter(([name]) => name === 'vw_close').length, 2);
});

test('failed and simultaneous opens retain successful tabs and deduplicate pending paths', async () => {
  const pending = deferred();
  const { tabs, calls } = setup({ vw_open: ({ path }) => path === 'slow' ? pending.promise : path === 'bad' ? Promise.reject(new Error('bad')) : { handle: path, path, viewerId: 'text', editable: true } });
  const slow = tabs.open('slow'); const duplicate = tabs.open('slow');
  await tabs.open('fast'); const fast = tabs.active;
  pending.resolve({ handle: 'slow', path: 'slow', viewerId: 'text', editable: true });
  await Promise.all([slow, duplicate]);
  assert.equal(tabs.tabs.length, 2); assert.equal(tabs.active, fast);
  assert.equal(await tabs.open('bad'), false); assert.equal(tabs.active, fast);
  assert.equal(tabs.tabs.length, 2);
  assert.equal(calls.filter(([name, args]) => name === 'vw_open' && args.path === 'slow').length, 1);
});

test('closing a loading tab releases its late handle and cannot resurrect it', async () => {
  const pending = deferred(); const { tabs, calls } = setup({ vw_open: () => pending.promise });
  const opening = tabs.open('slow');
  await Promise.resolve(); const id = tabs.tabs[0].id;
  await tabs.close(id);
  pending.resolve({ handle: 'late', path: 'slow', viewerId: 'text', editable: true }); await opening;
  assert.equal(tabs.tabs.length, 0);
  assert.ok(calls.some(([name, args]) => name === 'vw_close' && args.handle === 'late'));
});

test('exit freezes editing and switching while an inactive tab is being saved', async () => {
  const save = deferred();
  const { tabs, calls } = setup({ vw_save: () => save.promise }, async () => 'save');
  await tabs.open('a.txt'); const first = tabs.active;
  await tabs.open('b.txt'); const second = tabs.active; tabs.setDraft('save b');
  tabs.activate(first.id);
  const exiting = tabs.closeAll();
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  tabs.setDraft('must not be accepted during exit');
  tabs.activate(second.id);
  assert.equal(first.controller.state.dirty, false);
  assert.equal(tabs.active, first);
  save.resolve({ bytes: 6 });
  assert.equal(await exiting, true);
  assert.equal(calls.filter(([name]) => name === 'vw_save').length, 1);
});
