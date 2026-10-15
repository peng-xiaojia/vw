// Build with TAURI_CONFIG={"identifier":"dev.vw.test.editor"} before running.
const { chromium } = require('C:/Users/jjiap/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const { spawn } = require('node:child_process');
const fs = require('node:fs'); const path = require('node:path'); const assert = require('node:assert/strict');
const root = path.resolve('target/native-editor'); const output = path.resolve('output/playwright/editor');
fs.mkdirSync(root, { recursive: true }); fs.mkdirSync(output, { recursive: true });
const exe = path.join(root, 'vw-app.exe'); fs.copyFileSync(path.resolve('target/debug/vw-app.exe'), exe);
const js = path.join(root, 'example.js'); fs.writeFileSync(js, 'const total=1;function sum(){return total;}\r\n');
const log = path.join(root, 'legacy.log'); fs.writeFileSync(log, Buffer.from([0xd6, 0xd0, 0xce, 0xc4, 0x0d, 0x0a]));
const plain = path.join(root, 'plain.txt'); fs.writeFileSync(plain, 'first\nsecond\n');
const port = 9465; let app, browser;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
(async () => {
  app = spawn(exe, [], { windowsHide: true, stdio: 'ignore', env: { ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    VW_WEBVIEW_DATA: path.join(root, 'webview-data'), VW_TMP_DIR: path.join(root, 'sessions'), VW_PLUGIN_DIR: path.join(root, 'plugins'),
  } });
  let ready = false;
  for (let i = 0; i < 150; i++) {
    if (app.exitCode !== null) throw Error(`App exited ${app.exitCode}`);
    try { if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) { ready = true; break; } } catch {}
    await pause(200);
  }
  assert.equal(ready, true); browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = browser.contexts()[0].pages()[0]; await page.locator('#document-tabs').waitFor({ state: 'attached' });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  assert.equal(await page.evaluate(() => window.__TAURI__.app.getIdentifier()), 'dev.vw.test.editor');
  await page.locator('#btn-open-in-doc').click();
  assert.equal(await page.locator('#document-external .menu-popup button[data-action="reveal"]').isDisabled(), true);
  assert.equal(await page.locator('#document-external .menu-popup button[data-action="terminal"]').isDisabled(), true);
  await page.locator('#document-info').click();
  assert.match(await page.evaluate(async () => {
    try { await window.__TAURI__.core.invoke('vw_open_elsewhere', { handle: 'missing', action: 'terminal' }); return ''; }
    catch (error) { return String(error); }
  }), /未知 handle/);
  const open = filename => page.evaluate(async filename => (await import('./main.js')).openDocument(filename), filename);
  const draft = () => page.evaluate(async () => (await import('./main.js')).controller.state.draft);
  const cm = fn => page.locator('.CodeMirror').evaluate((el, source) => { const f = new Function('cm', source); return f(el.CodeMirror); }, fn);
  const menu = async (name, action) => {
    await page.locator('#top-menu summary').filter({ hasText: name }).click(); await page.locator(`#top-menu [data-action="${action}"]`).click();
  };
  assert.equal(await open(js), true);
  await page.locator('#btn-open-in-doc').click();
  assert.equal(await page.locator('#document-external .menu-popup button[data-action="reveal"]').isEnabled(), true);
  assert.equal(await page.locator('#document-external .menu-popup button[data-action="terminal"]').isEnabled(), true);
  await page.locator('#document-info').click();
  await page.locator('.CodeMirror').waitFor();
  assert.ok(await page.locator('.cm-keyword').count() > 0);
  await menu('编辑', 'edit'); await menu('编辑', 'format');
  await page.waitForFunction(async () => (await import('./main.js')).controller.state.draft.includes('const total = 1;'));
  assert.equal(fs.readFileSync(js, 'utf8'), 'const total=1;function sum(){return total;}\r\n');
  await page.keyboard.press('Control+f'); await page.locator('#edit-query').fill('total');
  await page.locator('#edit-find-next').click();
  const firstMatch = await cm('return cm.indexFromPos(cm.getCursor("from"))');
  await page.locator('#edit-find-next').click();
  assert.notEqual(await cm('return cm.indexFromPos(cm.getCursor("from"))'), firstMatch, 'find next advances');
  await page.locator('#edit-find-close').click();
  await page.keyboard.press('Control+h');
  await page.locator('#edit-query').fill('total'); await page.locator('#edit-replacement').fill('value');
  await page.locator('#edit-replace-all').click();
  assert.equal((await draft()).match(/value/g).length, 2);
  assert.match(await page.locator('#edit-find-result').textContent(), /2/);
  await page.locator('#edit-find-close').click(); await page.keyboard.press('Control+z');
  assert.equal((await draft()).match(/total/g).length, 2, 'replace all is one undo step');
  await page.keyboard.press('Control+Shift+z'); assert.equal((await draft()).match(/value/g).length, 2);
  await menu('视图', 'lineNumbers'); assert.equal(await page.locator('.CodeMirror-linenumbers').count(), 0);
  await menu('视图', 'lineNumbers'); assert.ok(await page.locator('.CodeMirror-linenumbers').count() > 0);
  const wrappingBefore = await cm('return cm.getOption("lineWrapping")');
  await page.keyboard.press('Alt+z'); assert.equal(await cm('return cm.getOption("lineWrapping")'), !wrappingBefore);
  await page.locator('#mode-binary').click();
  assert.equal(await page.getByRole('tab').count(), 2);
  await page.waitForFunction(() => document.querySelector('#badge').textContent.startsWith('hex'));
  assert.ok(await page.locator('#content pre').count() > 0);
  assert.ok(await page.getByRole('tab', { selected: true }).textContent().then(s => s.includes('二进制')));
  await page.locator('#mode-binary').click(); assert.match(await draft(), /value/);
  await page.locator('#save-encoding').selectOption('utf-16le'); await page.locator('#line-ending').selectOption('crlf');
  await page.keyboard.press('Control+s');
  await page.waitForFunction(async () => !(await import('./main.js')).controller.state.dirty);
  const saved = fs.readFileSync(js); assert.ok(saved.subarray(0, 2).equals(Buffer.from([255, 254])));
  assert.match(saved.subarray(2).toString('utf16le'), /const value = 1;\r\n/);
  assert.equal(fs.readFileSync(`${js}.bak`, 'utf8'), 'const total=1;function sum(){return total;}\r\n');
  assert.equal(await open(log), true); await page.locator('#reopen-encoding').selectOption('gbk');
  await page.waitForFunction(async () => (await import('./main.js')).controller.state.draft === '中文\n');
  await page.locator('#save-encoding').selectOption('utf-8'); await page.locator('#line-ending').selectOption('lf');
  await page.waitForFunction(async () => { const s = (await import('./main.js')).controller.state; return s.saveEncoding === 'utf-8' && s.lineEnding === 'lf' && s.dirty; });
  await page.keyboard.press('Control+s'); await page.waitForFunction(async () => { const s = (await import('./main.js')).controller.state; return !s.dirty && !s.saving && s.status.startsWith('已保存'); });
  assert.equal(fs.readFileSync(log, 'utf8'), '中文\n');
  assert.equal(await open(plain), true);
  await page.evaluate(async () => (await import('./main.js')).navigateBlock(0, 6, 'second'));
  assert.equal(await cm('return cm.getSelection()'), 'second', 'search/outline navigation reaches the visible text viewer');
  await menu('编辑', 'edit'); await page.locator('.CodeMirror').click(); await page.keyboard.press('Control+a'); await page.keyboard.insertText('first EDITED\nsecond\n');
  assert.equal(await open(js), true); assert.equal(await open(plain), true); assert.match(await draft(), /EDITED/);
  await page.keyboard.press('Control+z'); assert.equal(await draft(), 'first\nsecond\n', 'tab keeps its undo history');
  for (const filename of ['plugins/office/tests/fixtures/simple-table.doc', 'plugins/office/tests/fixtures/any_sheets.xls', 'target/tmp/office/text.docx', 'target/tmp/office/cells.xlsx']) {
    assert.equal(await open(path.resolve(filename)), true, filename);
    assert.match(await page.locator('#badge').textContent(), /office/);
    assert.ok(await page.locator('#content table, #content p, #content h1, #content h2').count() > 0);
    await page.locator('#mode-binary').click();
    await page.waitForFunction(() => document.querySelector('#badge').textContent.startsWith('hex'));
  }
  await page.screenshot({ path: path.join(output, 'office-binary-tabs.png') });
  assert.equal(await open(js), true); await menu('编辑', 'edit');
  await page.screenshot({ path: path.join(output, 'code-editor.png') });
  const viewport = page.viewportSize() || await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
  assert.ok(await page.locator('#status-bar').evaluate((el, height) => el.getBoundingClientRect().bottom <= height, viewport.height));
  assert.deepEqual(errors, []);
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify({ passed: true, checks: [
    'real WebView2 / IPC', 'external tool buttons and invalid-handle IPC rejection', 'bundled Prettier worker', 'code highlighting', 'replace all and undo/redo',
    'line numbers and wrapping', 'binary view preserves draft', 'UTF-16 CRLF bytes and backup',
    'GBK reopen and UTF-8 LF save', 'per-tab undo history', 'find next and visible text navigation', 'DOC/XLS/DOCX/XLSX preview and binary', 'footer bounds',
  ] }, null, 2));
  console.log('Native editor passed. Artifacts:', output);
  await page.evaluate(async () => { const { controller } = await import('./main.js');
    for (const tab of controller.tabs) tab.controller.state.dirty = false;
    await controller.closeAll(); await window.__TAURI__.window.getCurrentWindow().destroy();
  }).catch(() => {});
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  await browser?.close(); if (app && app.exitCode === null) app.kill();
});
