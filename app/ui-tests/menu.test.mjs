import test from 'node:test';
import assert from 'node:assert/strict';
import { menuStates } from '../ui/menu.mjs';
import { fileIcon } from '../ui/icons.mjs';

test('menu prevents saves and edits for read-only and busy documents', () => {
  const flags = state => Object.fromEntries(menuStates(state));
  assert.equal(flags({}).save, false);
  assert.equal(flags({ doc: { editable: false }, dirty: true }).save, false);
  assert.equal(flags({ doc: { editable: true }, dirty: true, saving: true }).save, false);
  assert.equal(flags({ doc: { editable: true }, dirty: true }).save, true);
  assert.equal(flags({ doc: { editable: true }, opening: true }).edit, false);
  assert.equal(flags({ doc: { viewerId: 'dir' } }).export, false);
  assert.equal(flags({ doc: { viewerId: 'archive' } }).export, true);
});

test('tree selects existing icon assets for folders and file types', () => {
  assert.equal(fileIcon({ isDir: true }), 'folder');
  assert.equal(fileIcon({ isDir: true }, true), 'folder-open');
  assert.equal(fileIcon({ name: 'IMAGE.PNG' }), 'image');
  assert.equal(fileIcon({ name: 'app.rs' }), 'file-code');
  assert.equal(fileIcon({ name: 'report.docx' }), 'file-text');
  assert.equal(fileIcon({ name: 'unknown.bin' }), 'file');
});
