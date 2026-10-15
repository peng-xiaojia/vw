import test from 'node:test';
import assert from 'node:assert/strict';
import { DocumentTabs } from '../ui/tabs.mjs';

function make({ choice = 'cancel', launch = async () => true, save = async () => ({ bytes: 5 }) } = {}) {
  const calls = [];
  const tabs = new DocumentTabs({ confirmDirty: async () => choice, invoke: async (cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === 'vw_open') return { handle: args.path, path: args.path, viewerId: 'text', editable: true };
    if (cmd === 'vw_page') return { blocks: [{ kind: 'preformatted', text: 'disk\n' }], total: 1 };
    if (cmd === 'vw_outline') return [];
    if (cmd === 'vw_snapshot') return 'disk\n';
    if (cmd === 'vw_save') return save();
    if (cmd === 'vw_open_elsewhere') return launch(args);
    if (cmd === 'vw_launch_external_app') return launch(args);
    if (cmd === 'vw_external_apps') return [{ id: 'code', name: 'VS Code', exe: 'C:\\tools\\Code.exe' }];
  } });
  return { tabs, calls };
}

test('cancelling an external editor request preserves the draft and does not launch', async () => {
  const { tabs, calls } = make(); await tabs.open('a.txt'); await tabs.edit(); tabs.setDraft('draft');
  assert.equal(await tabs.openElsewhere('other_app'), false);
  assert.equal(tabs.state.draft, 'draft'); assert.equal(tabs.state.dirty, true);
  assert.equal(calls.some(([cmd]) => cmd === 'vw_open_elsewhere'), false);
});

test('external open uses disk version without discarding the in-app draft', async () => {
  const { tabs, calls } = make({ choice: 'discard' }); await tabs.open('a.txt'); await tabs.edit(); tabs.setDraft('draft');
  assert.equal(await tabs.openElsewhere('other_app'), true);
  assert.deepEqual(calls.find(([cmd]) => cmd === 'vw_open_elsewhere')[1], { handle: 'a.txt', action: 'other_app' });
  assert.equal(tabs.state.draft, 'draft'); assert.equal(tabs.state.dirty, true);
  assert.match(tabs.state.status, /已保存版本/);
});

test('save failure blocks external launch', async () => {
  const { tabs, calls } = make({ choice: 'save', save: async () => { throw Error('conflict'); } });
  await tabs.open('a.txt'); await tabs.edit(); tabs.setDraft('draft');
  assert.equal(await tabs.openElsewhere('terminal'), false);
  assert.equal(calls.some(([cmd]) => cmd === 'vw_open_elsewhere'), false);
  assert.equal(tabs.state.dirty, true);
});

test('external request remains bound to original tab and native cancellation is quiet', async () => {
  let resolve; const pending = new Promise(r => { resolve = r; });
  const { tabs, calls } = make({ launch: async () => pending }); await tabs.open('a.txt');
  const original = tabs.active; const request = tabs.openElsewhere('other_app');
  await new Promise(r => setTimeout(r, 0)); await tabs.open('b.txt');
  assert.equal(await original.controller.openElsewhere('other_app'), false, 'duplicate launch is blocked');
  resolve(false); assert.equal(await request, false);
  assert.equal(calls.filter(([cmd]) => cmd === 'vw_open_elsewhere').length, 1);
  assert.equal(calls.find(([cmd]) => cmd === 'vw_open_elsewhere')[1].handle, 'a.txt');
  assert.equal(original.controller.state.error, ''); assert.equal(original.controller.state.externalOpening, false);
  assert.equal(tabs.state.doc.handle, 'b.txt');
});

test('closing waits for a native chooser before releasing its source handle', async () => {
  let resolve; const pending = new Promise(r => { resolve = r; });
  const { tabs, calls } = make({ launch: async () => pending }); await tabs.open('a.txt');
  const request = tabs.openElsewhere('other_app'); await new Promise(r => setTimeout(r, 0));
  const close = tabs.closeAll(); await new Promise(r => setTimeout(r, 0));
  assert.equal(calls.some(([cmd]) => cmd === 'vw_close'), false);
  resolve(false); await request; assert.equal(await close, true);
  assert.equal(tabs.tabs.length, 0);
});

test('archive members cannot be handed out as temporary editable sources', async () => {
  const { tabs, calls } = make(); await tabs.open('a.zip', { inner: 'a.txt' });
  assert.equal(await tabs.openElsewhere('other_app'), false);
  assert.equal(await tabs.openElsewhere('reveal'), false);
  assert.equal(await tabs.openInApp('code', 'VS Code'), false);
  assert.equal(calls.some(([cmd]) => cmd === 'vw_open_elsewhere' || cmd === 'vw_launch_external_app'), false);
  assert.match(tabs.state.error, /导出/);
});

test('reveal reports the explorer hand-off without touching the draft', async () => {
  const { tabs, calls } = make({ choice: 'discard' }); await tabs.open('a.txt'); await tabs.edit(); tabs.setDraft('draft');
  assert.equal(await tabs.openElsewhere('reveal'), true);
  assert.deepEqual(calls.find(([cmd]) => cmd === 'vw_open_elsewhere')[1], { handle: 'a.txt', action: 'reveal' });
  assert.equal(tabs.state.draft, 'draft'); assert.equal(tabs.state.dirty, true);
  assert.match(tabs.state.status, /文件资源管理器/);
});

test('known apps launch through the registry-resolved id and keep draft protection', async () => {
  const { tabs, calls } = make({ choice: 'discard' }); await tabs.open('a.txt'); await tabs.edit(); tabs.setDraft('draft');
  assert.equal(await tabs.openInApp('code', 'VS Code'), true);
  assert.deepEqual(calls.find(([cmd]) => cmd === 'vw_launch_external_app')[1], { handle: 'a.txt', appId: 'code' });
  assert.equal(tabs.state.draft, 'draft'); assert.equal(tabs.state.dirty, true);
  assert.match(tabs.state.status, /已用 VS Code 打开 · 使用的是已保存版本/);
});

test('app detection is fetched once and cached on the controller', async () => {
  const { tabs, calls } = make(); await tabs.open('a.txt');
  const count = () => calls.filter(([cmd]) => cmd === 'vw_external_apps').length;
  await tabs.ensureExternalApps();
  assert.equal(count(), 1);
  await tabs.ensureExternalApps();
  assert.equal(count(), 1);
  assert.deepEqual(tabs.state.externalApps, [{ id: 'code', name: 'VS Code', exe: 'C:\\tools\\Code.exe' }]);
});
