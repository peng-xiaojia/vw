// Native WebView2 + real IPC integration. Build with the isolated identifier first:
// $env:TAURI_CONFIG='{"identifier":"dev.vw.test.htmltabs"}'; cargo build -p vw-app --offline
const { chromium } = require('C:/Users/jjiap/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const assert = require('node:assert/strict');
const output = path.resolve('output/playwright/html-tabs');
const root = path.resolve('target/native-html-tabs');
fs.mkdirSync(root, { recursive: true }); fs.mkdirSync(output, { recursive: true });
const exe = path.join(root, 'vw-app.exe');
fs.copyFileSync(path.resolve(process.env.VW_TEST_EXE || 'target/debug/vw-app.exe'), exe);
const text = path.join(root, '草稿.txt'); fs.writeFileSync(text, 'Original\n');
const html = path.join(root, 'preview.html');
fs.writeFileSync(html, '<!doctype html><link rel="stylesheet" href="assets/style.css"><h1>Styled</h1><img src="assets/logo.svg"><button onclick="this.textContent=\'Clicked\'">Click</button><script>window.previewScript=true</script>');
fs.mkdirSync(path.join(root, 'assets'), { recursive: true });
fs.writeFileSync(path.join(root, 'assets/style.css'), 'h1{color:rgb(12,34,56)} body{min-height:1800px}');
fs.writeFileSync(path.join(root, 'assets/logo.svg'), '<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect width="80" height="40" fill="blue"/></svg>');
const port = 9463;
let app, browser, referenceBrowser;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

(async () => {
  app = spawn(exe, [], { windowsHide: true, stdio: 'ignore', env: {
    ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    VW_WEBVIEW_DATA: path.join(root, 'webview-data'), VW_TMP_DIR: path.join(root, 'sessions'), VW_PLUGIN_DIR: path.join(root, 'plugins'),
  } });
  let ready = false;
  for (let i = 0; i < 150; ++i) {
    if (app.exitCode !== null) throw new Error(`App exited: ${app.exitCode}`);
    try { if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) { ready = true; break; } } catch {}
    await pause(200);
  }
  assert.equal(ready, true, 'WebView2 debugging endpoint');
  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = browser.contexts()[0].pages()[0];
  await page.locator('#document-tabs').waitFor({ state: 'attached' });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  const identifier = await page.evaluate(() => window.__TAURI__.app.getIdentifier());
  assert.equal(identifier, 'dev.vw.test.htmltabs', 'only run the isolated test build');
  const open = filename => page.evaluate(async filename => (await import('./main.js')).openDocument(filename), filename);
  const activeFrame = () => page.frames().find(frame => frame.url().includes('vw-preview.localhost'));
  assert.equal(await open(text), true); assert.equal(await open(html), true);
  await page.getByRole('tab', { name: 'preview.html' }).waitFor();
  const frame = page.frameLocator('#html-preview-host iframe');
  await frame.locator('h1').waitFor();
  assert.equal(await frame.locator('h1').evaluate(el => getComputedStyle(el).color), 'rgb(12, 34, 56)');
  await frame.locator('img').evaluate(img => img.decode());
  assert.equal(await frame.locator('img').evaluate(img => img.naturalWidth), 80);
  assert.equal(await activeFrame().evaluate(() => window.previewScript), true);
  assert.equal(await page.evaluate(() => window.previewScript), undefined);
  assert.equal(await activeFrame().evaluate(() => { try { return Boolean(parent.document); } catch { return false; } }), false);
  const ipc = await activeFrame().evaluate(async () => {
    if (!window.__TAURI__?.core?.invoke) return { allowed: false, reason: 'no bridge' };
    return Promise.race([
      window.__TAURI__.core.invoke('vw_startup_paths').then(() => ({ allowed: true }), error => ({ allowed: false, reason: String(error) })),
      new Promise(resolve => setTimeout(() => resolve({ allowed: false, reason: 'blocked without response' }), 1500)),
    ]);
  });
  console.log('Sandbox IPC:', JSON.stringify(ipc));
  assert.equal(ipc.allowed, false);
  // Even raw requests from the sandbox cannot access the application's privileged origin.
  assert.equal(await activeFrame().evaluate(async () => {
    try { await fetch('http://ipc.localhost/vw_pick_files'); return true; } catch { return false; }
  }), false);
  await frame.getByRole('button', { name: 'Click', exact: true }).click();
  await activeFrame().evaluate(() => scrollTo(0, 600));
  await page.getByRole('tab', { name: '草稿.txt' }).click();
  await page.locator('.CodeMirror').click(); await page.keyboard.press('Control+a'); await page.keyboard.insertText('Unsaved draft\n');
  await page.getByRole('tab', { name: 'preview.html' }).click();
  assert.equal(await frame.getByRole('button').textContent(), 'Clicked');
  assert.equal(await activeFrame().evaluate(() => scrollY), 600);
  await page.keyboard.press('Control+Shift+Tab');
  assert.equal(await page.locator('#editor').inputValue(), 'Unsaved draft\n');
  await page.keyboard.press('Control+w');
  await page.locator('#dirty-dialog [data-choice="cancel"]').click();
  assert.equal(await page.getByRole('tab').count(), 2);
  // Window exit must also protect an inactive draft.
  await page.getByRole('tab', { name: 'preview.html' }).click();
  await page.evaluate(() => window.__TAURI__.window.getCurrentWindow().close());
  await page.locator('#dirty-dialog [data-choice="cancel"]').click();
  assert.equal(await page.getByRole('tab').count(), 2);
  assert.equal(fs.readFileSync(text, 'utf8'), 'Original\n');
  const sample = process.env.VW_TEST_HTML || 'D:/desktop/临时文件/设计/design/index.html';
  const comparisons = {};
  if (fs.existsSync(sample)) {
    assert.equal(await open(sample), true);
    const sampleFrame = page.frameLocator('#html-preview-host iframe:not(.html-preview-inactive)');
    await sampleFrame.locator('.entries').waitFor();
    await sampleFrame.locator('.logo img').evaluate(img => img.decode());
    const box = await page.locator('#html-preview-host iframe:not(.html-preview-inactive)').boundingBox();
    // WebView2 uses a classic scrollbar; headless Chromium uses an overlay scrollbar.
    // Match the usable layout width rather than including WebView2's scrollbar gutter.
    const layoutWidth = await sampleFrame.locator('html').evaluate(el => el.clientWidth);
    referenceBrowser = await chromium.launch({ headless: true, executablePath: 'C:/Users/jjiap/AppData/Local/ms-playwright/chromium-1208/chrome-win64/chrome.exe' });
    const reference = await referenceBrowser.newPage({ viewport: { width: layoutWidth, height: Math.round(box.height) } });
    await reference.goto(pathToFileURL(sample).href);
    const metrics = () => {
      const h1 = document.querySelector('h1'), entries = document.querySelector('.entries');
      const style = getComputedStyle(h1), grid = getComputedStyle(entries);
      return { background: getComputedStyle(document.body).backgroundColor, fontSize: style.fontSize, color: style.color,
        fontFamily: style.fontFamily, gridColumns: grid.gridTemplateColumns.split(' ').length,
        entryWidth: document.querySelector('.entry').getBoundingClientRect().width,
        logoWidth: document.querySelector('.logo img').naturalWidth };
    };
    const native = await sampleFrame.locator('body').evaluate(metrics);
    const expected = await reference.evaluate(metrics);
    console.log('HTML layout comparison:', JSON.stringify({ native, expected }));
    assert.deepEqual({ ...native, entryWidth: 0 }, { ...expected, entryWidth: 0 });
    assert.ok(Math.abs(native.entryWidth - expected.entryWidth) < 2);
    comparisons.native = native; comparisons.browser = expected;
    await page.screenshot({ path: path.join(output, 'native-index.png') });
    await reference.screenshot({ path: path.join(output, 'browser-index.png') });
    const navigation = page.waitForEvent('framenavigated', { predicate: f => f.url().includes('/prototype/pages/index.html'), timeout: 10000 });
    await sampleFrame.locator('a[href="prototype/pages/index.html"]').click();
    await navigation.catch(async error => { console.log('Navigation state:', page.frames().map(f => f.url()), await page.locator('.html-link-prompt').allTextContents()); throw error; });
    const appFrame = page.frames().find(f => f.url().includes('/prototype/pages/index.html'));
    assert.ok(appFrame, 'local page navigation stays inside preview');
    await appFrame.locator('.list a').first().waitFor({ state: 'attached' });
    assert.ok(await appFrame.locator('.list a').count() > 10);
    await page.screenshot({ path: path.join(output, 'native-prototype.png') });
    // Changing tabs must not reload a navigated HTML page.
    await page.getByRole('tab', { name: 'preview.html' }).click();
    await page.getByRole('tab', { name: 'index.html' }).click();
    assert.ok(page.frames().some(f => f.url().includes('/prototype/pages/index.html')));
  }
  assert.deepEqual(errors, []);
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify({ passed: true, comparisons,
    checks: ['real IPC opens two tabs', 'CSS/SVG/JS via custom protocol', 'no parent DOM or usable IPC', 'IPC CSP blocked', 'scroll and script state retained', 'dirty close cancelled', 'inactive dirty exit cancelled', 'original HTML layout and local navigation'],
  }, null, 2));
  console.log('Native HTML/tabs passed:', JSON.stringify(comparisons));
  await page.evaluate(async () => {
    const { controller } = await import('./main.js');
    for (const tab of controller.tabs) tab.controller.state.dirty = false;
    await controller.closeAll(); await window.__TAURI__.window.getCurrentWindow().destroy();
  }).catch(error => { if (!/closed|destroyed/i.test(error.message)) throw error; });
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  await referenceBrowser?.close(); await browser?.close();
  if (app && app.exitCode === null) app.kill();
});
