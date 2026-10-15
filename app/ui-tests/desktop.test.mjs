import test from 'node:test';
import assert from 'node:assert/strict';
import { DocumentController } from '../ui/controller.mjs';

const { bindNativeDrop } = await import('../ui/desktop.mjs').catch(() => ({}));

test('WebView file drops open a document through the normal dirty guard', async () => {
  assert.equal(typeof bindNativeDrop, 'function');
  let handler;
  const hovered = [];
  const opened = [];
  const c = new DocumentController({
    confirmDirty: async () => 'cancel',
    invoke: async (cmd, args) => {
      if (cmd === 'vw_open') { opened.push(args.path); return { handle: args.path, viewerId: 'text', editable: true }; }
      if (cmd === 'vw_page') return { blocks: [], total: 0 };
      if (cmd === 'vw_outline') return [];
      if (cmd === 'vw_snapshot') return 'original';
      if (cmd === 'vw_close') return;
      throw new Error(cmd);
    },
  });
  const api = {
    window: { getCurrentWindow() { throw new Error('Drops belong to the WebView'); } },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: async fn => { handler = fn; return () => {}; } }) },
  };
  await bindNativeDrop(api, { onPaths: paths => c.open(paths[0]), onHover: value => hovered.push(value) });
  await handler({ payload: { type: 'enter', paths: ['G:\\中文 文件.txt'] } });
  await handler({ payload: { type: 'over' } });
  await handler({ payload: { type: 'drop', paths: ['G:\\中文 文件.txt'] } });
  assert.equal(c.state.doc.sourcePath, 'G:\\中文 文件.txt');
  await c.edit(); c.setDraft('unsaved');
  await handler({ payload: { type: 'drop', paths: ['G:\\other.txt'] } });
  assert.deepEqual(opened, ['G:\\中文 文件.txt']);
  assert.equal(c.state.draft, 'unsaved');
  await handler({ payload: { type: 'leave' } });
  assert.deepEqual(hovered, [true, true, false, false, false]);
});

test('drop routing errors are reported and the overlay is cleared', async () => {
  assert.equal(typeof bindNativeDrop, 'function');
  let handler;
  const errors = []; const hover = [];
  await bindNativeDrop({ webview: { getCurrentWebview: () => ({ onDragDropEvent: async fn => { handler = fn; } }) } }, {
    onPaths: async () => { throw new Error('unavailable'); },
    onHover: value => hover.push(value), onError: error => errors.push(error.message),
  });
  await handler({ payload: { type: 'drop', paths: ['file.txt'] } });
  assert.deepEqual(hover, [false]); assert.deepEqual(errors, ['unavailable']);
});
