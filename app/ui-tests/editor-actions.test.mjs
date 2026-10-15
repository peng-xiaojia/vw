import test from 'node:test';
import assert from 'node:assert/strict';
import { DocumentController } from '../ui/controller.mjs';
import { DocumentTabs } from '../ui/tabs.mjs';

const make = (overrides = {}) => {
  const calls = [];
  const c = new DocumentController({ invoke: async (cmd, args) => {
    calls.push([cmd, args]);
    if (overrides[cmd]) return overrides[cmd](args);
    if (cmd === 'vw_open') return { handle: 'a', path: 'code.js', viewerId: 'text', editable: true, format: { provider: 'rpc' }, editOptions: { encoding: 'utf-8', has_bom: false } };
    if (cmd === 'vw_page') return { blocks: [], total: 0 };
    if (cmd === 'vw_outline') return [];
    if (cmd === 'vw_snapshot') return 'a\r\nb\r\n';
    if (cmd === 'vw_save') return { bytes: 8 };
    if (cmd === 'vw_close') return;
  } });
  return { c, calls };
};

test('encoding and newline choices dirty unchanged text and are sent through the normal save pipeline', async () => {
  const { c, calls } = make(); await c.open('code.js'); await c.edit();
  c.setTextOptions({ encoding: 'utf-16le', lineEnding: 'cr' });
  assert.equal(c.state.dirty, true);
  await c.save();
  assert.deepEqual(calls.find(([name]) => name === 'vw_save')[1], { handle: 'a', text: 'a\rb\r', encoding: 'utf-16le' });
});

test('a stale formatter cannot overwrite text typed while formatting', async () => {
  const { c } = make(); await c.open('code.js'); await c.edit();
  let resolve; const pending = new Promise(r => { resolve = r; });
  const action = c.format(async () => pending);
  await new Promise(r => setTimeout(r, 0));
  c.setDraft('new input'); resolve('old formatted input');
  assert.equal(await action, false); assert.equal(c.state.draft, 'new input');
});

test('formatting only updates the draft and a failure preserves it', async () => {
  const { c, calls } = make(); await c.open('code.js'); await c.edit();
  assert.equal(await c.format(async () => 'formatted\n'), true);
  assert.equal(c.state.draft, 'formatted\n');
  assert.equal(c.state.dirty, true);
  assert.equal(calls.some(([cmd]) => cmd === 'vw_save'), false);
  assert.equal(await c.format(async () => { throw Error('parse error'); }), false);
  assert.equal(c.state.draft, 'formatted\n');
});

test('closeAll waits for pending formatting then guards the resulting draft', async () => {
  const { c } = make();
  const tabs = new DocumentTabs({ invoke: c.invoke, confirmDirty: async () => 'cancel' });
  await tabs.open('code.js');
  let resolve; const pending = new Promise(r => { resolve = r; });
  const format = tabs.format(async () => pending);
  await new Promise(r => setTimeout(r, 0));
  const close = tabs.closeAll(); resolve('formatted and unsaved');
  assert.equal(await format, true); assert.equal(await close, false);
  assert.equal(tabs.tabs.length, 1); assert.equal(tabs.state.draft, 'formatted and unsaved');
});

test('save cannot write a discarded draft while re-decoding is in flight', async () => {
  let resolve; const pending = new Promise(r => { resolve = r; });
  const { c, calls } = make({ vw_reopen_encoding: () => pending });
  c.confirmDirty = async () => 'discard';
  await c.open('code.js'); await c.edit(); c.setDraft('discarded old draft');
  const reopen = c.reopenEncoding('gbk');
  await new Promise(r => setTimeout(r, 0)); assert.equal(c.state.saving, true);
  assert.equal(await c.save(), false);
  assert.equal(calls.some(([cmd]) => cmd === 'vw_save'), false);
  resolve({ editable: true }); assert.equal(await reopen, true);
});

test('text options stay bound to their original tab across asynchronous snapshots', async () => {
  let snapshotReady; const pending = new Promise(r => { snapshotReady = r; });
  const { c } = make({ vw_snapshot: () => pending });
  const tabs = new DocumentTabs({ invoke: c.invoke });
  await tabs.open('code.js'); const original = tabs.active;
  const options = tabs.setTextOptions({ encoding: 'gbk' });
  await tabs.open('other.js'); const other = tabs.active;
  snapshotReady('original text'); assert.equal(await options, true);
  assert.equal(original.controller.state.saveEncoding, 'gbk');
  assert.equal(other.controller.state.saveEncoding, null);
});

test('pending text options cannot dirty a tab after closeAll has frozen input', async () => {
  let snapshotReady, closeReady;
  const snapshot = new Promise(r => { snapshotReady = r; });
  const release = new Promise(r => { closeReady = r; });
  const { c } = make({ vw_snapshot: () => snapshot, vw_close: () => release });
  const tabs = new DocumentTabs({ invoke: c.invoke });
  await tabs.open('code.js'); const original = tabs.active;
  const options = tabs.setTextOptions({ encoding: 'gbk' });
  const close = tabs.closeAll(); await new Promise(r => setTimeout(r, 0));
  assert.equal(tabs.closing, true); snapshotReady('original text');
  assert.equal(await options, false); assert.equal(original.controller.state.dirty, false);
  assert.equal(original.controller.state.saveEncoding, null);
  closeReady(); assert.equal(await close, true); assert.equal(tabs.tabs.length, 0);
});
