const { chromium } = require('C:/Users/jjiap/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve('app/ui');
const server = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://127.0.0.1');
  if (url.pathname === '/preview') {
    response.setHeader('Content-Type', 'text/html; charset=utf-8');
    response.setHeader('Content-Security-Policy', `default-src 'none'; script-src 'unsafe-inline' http://127.0.0.1:${server.address().port}; style-src 'unsafe-inline' http://127.0.0.1:${server.address().port}; sandbox allow-scripts`);
    response.end(url.searchParams.get('text') + '<script src="/html-frame-bridge.js"></script>'); return;
  }
  if (url.pathname === '/fixture.css') {
    response.setHeader('Content-Type', 'text/css'); response.end('h1 { color: rgb(12, 34, 56); }'); return;
  }
  const file = path.join(root, decodeURIComponent(request.url === '/' ? 'index.html' : request.url.slice(1)));
  if (!file.startsWith(root) || !fs.existsSync(file)) { response.writeHead(404); response.end(); return; }
  response.setHeader('Content-Type', file.endsWith('.html') ? 'text/html' : file.endsWith('.css') ? 'text/css' : file.endsWith('.svg') ? 'image/svg+xml' : 'text/javascript');
  response.end(fs.readFileSync(file));
});

(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ headless: true, executablePath: 'C:/Users/jjiap/AppData/Local/ms-playwright/chromium-1208/chrome-win64/chrome.exe' });
  try {
    const page = await browser.newPage({ viewport: { width: 1100, height: 760 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => {
      window.__TAURI__ = {
        core: { invoke: async (command, args = {}) => {
          if (command === 'vw_startup_paths') return [];
          if (command === 'vw_pick_path') return window.testPath || 'C:\\notes\\draft.txt';
          if (command === 'vw_pick_files') return window.testPaths || [window.testPath || 'C:\\notes\\draft.txt'];
          if (command === 'vw_html_preview') return `${location.origin}/preview?text=${encodeURIComponent(args.text)}`;
          if (command === 'vw_open_external') { (window.externalOpens ??= []).push(args.url); return; }
          if (command === 'vw_list_dir') return { path: 'C:\\notes', parent: 'C:\\', entries: [] };
          if (command === 'vw_open') return { handle: args.path, path: args.path, sourceDisplay: args.path, title: args.path.split('\\').at(-1), viewerId: args.path.endsWith('.json') ? 'json' : args.path.endsWith('.md') ? 'markdown' : args.path.endsWith('.html') ? 'html' : 'text', editable: /\.(txt|md|html)$/i.test(args.path) };
          if (command === 'vw_page') {
            const blocks = args.handle.endsWith('.json') ? [{ kind: 'code', lang: 'json', text: '{"nested":{"value":42},"items":[1,2]}' }]
              : args.handle.endsWith('.md') ? [{ kind: 'heading', level: 1, text: [{ kind: 'text', text: 'Chapter' }] }, { kind: 'para', text: [{ kind: 'text', text: 'Body paragraph' }] }, { kind: 'heading', level: 2, text: [{ kind: 'text', text: 'Section' }] }, { kind: 'heading', level: 1, text: [{ kind: 'text', text: 'Next' }] }]
              : args.handle.endsWith('.html') ? [{ kind: 'preformatted', text: '<h1>Hello</h1>' }]
              : [{ kind: 'preformatted', text: 'First line' }];
            return { blocks, total: blocks.length };
          }
          if (command === 'vw_outline') return args.handle.endsWith('.md') ? [{ level: 1, text: 'Chapter', block: 0 }, { level: 2, text: 'Section', block: 2 }, { level: 1, text: 'Next', block: 3 }] : [{ level: 1, text: 'First line', block: 0 }];
          if (command === 'vw_snapshot') return args.handle.endsWith('.html') ? '<!doctype html><link rel="stylesheet" href="/fixture.css"><style>body { margin: 28px; font: 17px sans-serif; } h1 { font-size: 32px; }</style><h1>Hello</h1><script>window.injected = true</script><button onclick="this.textContent=\'Clicked\'">Click me</button>' : args.handle.endsWith('.md') ? '# Chapter\n\nBody paragraph\n\n## Section\n\n# Next' : 'First line';
          if (command === 'vw_preview_markdown') return args.text.startsWith('# Fresh') ? { blocks: [{ kind: 'heading', level: 1, text: [{ kind: 'text', text: 'Fresh' }] }], outline: [{ level: 1, text: 'Fresh', block: 0 }] } : { blocks: [{ kind: 'heading', level: 1, text: [{ kind: 'text', text: 'Chapter' }] }, { kind: 'para', text: [{ kind: 'text', text: 'Body paragraph' }] }, { kind: 'heading', level: 2, text: [{ kind: 'text', text: 'Section' }] }, { kind: 'heading', level: 1, text: [{ kind: 'text', text: 'Next' }] }], outline: [{ level: 1, text: 'Chapter', block: 0 }, { level: 2, text: 'Section', block: 2 }, { level: 1, text: 'Next', block: 3 }] };
          if (command === 'vw_close') return;
          if (command === 'vw_plugin_market') return [];
          if (command === 'vw_plugin_prepare') return { token: 't1', manifest: { id: 'demo.plugin', name: 'Demo', version: '1.0.0', permissions: [], contributes: { viewers: [{ extensions: ['demo'] }] } } };
          if (command === 'vw_plugins') return { entries: [], directory: 'C:\\plugins', platform: 'win32-x64' };
          throw new Error(command);
        } },
        window: { getCurrentWindow: () => ({ onCloseRequested: async () => {}, minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {} }) },
        webview: { getCurrentWebview: () => ({ onDragDropEvent: async () => {} }) },
        event: { listen: async () => {} },
      };
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.locator('#top-menu summary').first().click();
    assert.equal(await page.locator('#btn-save').isVisible(), true);
    assert.equal(await page.locator('#btn-save').isDisabled(), true);
    assert.equal(await page.locator('#btn-export').isDisabled(), true);
    assert.equal(await page.locator('#btn-cancel').isDisabled(), true);
    const save = await page.locator('#btn-save').boundingBox();
    assert.equal(await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.id, [save.x + 15, save.y + save.height / 2]), 'btn-save');
    await page.locator('#btn-file').click();
    await page.locator('#top-menu summary').filter({ hasText: '视图' }).click();
    await page.locator('[data-action="appearance"]').click();
    await page.locator('[name="theme-mode"][value="dark"]').check();
    assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    await page.locator('#document-font-family').selectOption('serif');
    await page.locator('#document-font-size').fill('20');
    assert.equal(await page.locator('#content').evaluate(element => getComputedStyle(element).fontSize), '20px');
    assert.equal(await page.locator('html').getAttribute('data-document-font'), 'serif');
    await page.screenshot({ path: 'target/ui-dark-mode.png' });
    await page.locator('#appearance-dialog form button').click();
    await page.locator('#btn-sidebar').click();
    const left = await page.locator('#sidebar').boundingBox();
    const splitter = await page.locator('#sidebar-resizer').boundingBox();
    await page.mouse.move(splitter.x + 2, splitter.y + 100);
    await page.mouse.down(); await page.mouse.move(splitter.x + 102, splitter.y + 100); await page.mouse.up();
    assert.ok((await page.locator('#sidebar').boundingBox()).width > left.width + 80);
    await page.locator('#btn-sidebar').click();
    if (!(await page.locator('#outline-panel').isVisible())) await page.locator('#btn-outline').click();
    const right = await page.locator('#outline-panel').boundingBox();
    const rightSplitter = await page.locator('#outline-resizer').boundingBox();
    await page.mouse.move(rightSplitter.x + 2, rightSplitter.y + 100);
    await page.mouse.down(); await page.mouse.move(rightSplitter.x - 78, rightSplitter.y + 100); await page.mouse.up();
    assert.ok((await page.locator('#outline-panel').boundingBox()).width > right.width + 60);
    await page.locator('#btn-plugins').click();
    await page.locator('#market-repository').fill('example/vw-plugin');
    await page.locator('#market-add-form button').click();
    assert.equal(await page.locator('#market-list .plugin-card').count(), 1);
    await page.locator('#market-list .plugin-card button').first().click();
    assert.equal(await page.locator('#plugin-dialog').evaluate(element => element.open), true);
    await page.locator('#btn-plugin-cancel').click();
    await page.locator('#btn-close-plugins').click();
    await page.evaluate(() => { window.testPath = 'C:\\notes\\sample.json'; });
    await page.keyboard.press('Control+o');
    await page.locator('.json-tree details').first().waitFor();
    assert.equal(await page.locator('.json-tree details').count(), 3);
    await page.locator('.json-tree details').nth(1).locator('summary').click();
    await page.locator('.json-tree .json-number').first().waitFor();
    assert.equal(await page.locator('.json-tree .json-number').count(), 1);
    await page.screenshot({ path: 'target/ui-json-tree.png' });
    await page.evaluate(() => { window.testPath = 'C:\\notes\\sample.md'; });
    await page.keyboard.press('Control+o');
    await page.locator('#content .heading-fold').first().click();
    assert.equal(await page.locator('#content [data-block="1"]').evaluate(element => element.hidden), true);
    assert.equal(await page.locator('#content [data-block="3"]').evaluate(element => element.hidden), false);
    if (!(await page.locator('#outline-panel').isVisible())) await page.locator('#btn-outline').click();
    assert.equal(await page.locator('#outline > li').count(), 2);
    await page.locator('#outline-collapse-all').click();
    assert.equal(await page.locator('#outline li li:visible').count(), 0);
    await page.locator('#outline-expand-all').click();
    assert.equal(await page.locator('#outline li li:visible').count(), 1);
    await page.locator('#outline .outline-toggle').first().click();
    assert.equal(await page.locator('#outline li li:visible').count(), 0);
    await page.locator('#outline .outline-toggle').first().click();
    const beforeZoom = await page.locator('#content').evaluate(element => parseInt(getComputedStyle(element).fontSize, 10));
    await page.locator('#content').evaluate(element => element.dispatchEvent(new WheelEvent('wheel', { deltaY: -100, ctrlKey: true, bubbles: true, cancelable: true })));
    assert.equal(await page.locator('#content').evaluate(element => parseInt(getComputedStyle(element).fontSize, 10)), beforeZoom + 1);
    if (!(await page.locator('#btn-edit').isVisible())) await page.locator('#top-menu summary').filter({ hasText: '编辑' }).click();
    await page.locator('#btn-edit').click();
    assert.equal(await page.locator('.CodeMirror').isVisible(), true);
    await page.locator('.CodeMirror').click(); await page.keyboard.press('Control+a'); await page.keyboard.insertText('# Fresh');
    if (!(await page.locator('#btn-preview').isVisible())) await page.locator('#top-menu summary').filter({ hasText: '编辑' }).click();
    await page.locator('#btn-preview').click();
    assert.equal(await page.locator('#content').isVisible(), true);
    assert.equal(await page.locator('#content h1').first().textContent(), '▾Fresh');
    assert.equal(await page.locator('#outline .outline-link').first().textContent(), 'Fresh');
    await page.evaluate(() => { window.testPath = 'C:\\notes\\sample.html'; });
    await page.keyboard.press('Control+o');
    const htmlFrame = page.frameLocator('#html-preview-host iframe');
    await htmlFrame.locator('h1').filter({ hasText: 'Hello' }).waitFor();
    assert.equal(await page.locator('#dirty-dialog').evaluate(element => element.open), false);
    assert.equal(await page.getByRole('tab').count(), 4);
    assert.equal(await page.locator('#document-mode').isVisible(), true);
    assert.equal(await page.locator('#mode-preview').getAttribute('aria-pressed'), 'true');
    assert.equal(await page.evaluate(() => window.injected), undefined);
    assert.equal(await htmlFrame.locator('h1').evaluate(element => getComputedStyle(element).color), 'rgb(12, 34, 56)');
    assert.equal(await htmlFrame.locator('h1').evaluate(element => getComputedStyle(element).fontSize), '32px');
    assert.equal(await htmlFrame.locator('h1').evaluate(() => window.injected), true);
    assert.equal(await htmlFrame.locator('h1').evaluate(() => { try { return Boolean(parent.document); } catch { return false; } }), false);
    await htmlFrame.locator('h1').evaluate(() => parent.postMessage({ type: 'vw-html', action: 'external', value: 'https://example.com/forged' }, '*'));
    await page.locator('.html-link-prompt').waitFor();
    assert.deepEqual(await page.evaluate(() => window.externalOpens ?? []), []);
    await page.locator('.html-link-prompt button').first().dispatchEvent('click');
    assert.deepEqual(await page.evaluate(() => window.externalOpens ?? []), []);
    await page.getByRole('button', { name: '打开外部链接' }).click();
    assert.deepEqual(await page.evaluate(() => window.externalOpens), ['https://example.com/forged']);
    await htmlFrame.locator('h1').evaluate(() => parent.postMessage({ type: 'vw-html', action: 'key', value: { key: 'w' } }, '*'));
    assert.equal(await page.getByRole('tab').count(), 4);
    await htmlFrame.getByRole('button', { name: 'Click me' }).click();
    await page.getByRole('tab', { name: 'sample.md' }).click();
    assert.equal(await page.locator('#content h1').textContent(), '▾Fresh');
    await page.getByRole('tab', { name: 'sample.json' }).click();
    assert.equal(await page.locator('.json-tree details').nth(1).getAttribute('open'), '');
    await page.getByRole('tab', { name: 'sample.html' }).click();
    assert.equal(await htmlFrame.getByRole('button').textContent(), 'Clicked');
    if (!(await page.locator('#btn-edit').isVisible())) await page.locator('#top-menu summary').filter({ hasText: '编辑' }).click();
    await page.locator('#btn-edit').click();
    assert.match(await page.locator('#editor').inputValue(), /<h1>Hello<\/h1>/);
    assert.equal(await page.locator('#mode-source').getAttribute('aria-pressed'), 'true');
    if (!(await page.locator('#btn-preview').isVisible())) await page.locator('#top-menu summary').filter({ hasText: '编辑' }).click();
    await page.locator('#btn-preview').click();
    await htmlFrame.locator('h1').waitFor();
    await htmlFrame.locator('h1').evaluate(element => element.dispatchEvent(new WheelEvent('wheel', { deltaY: -100, ctrlKey: true, bubbles: true, cancelable: true })));
    await page.waitForFunction(() => document.querySelector('#html-preview-host iframe'));
    await htmlFrame.locator('html[style*="1.1"]').waitFor();
    await page.keyboard.press('Control+Shift+Tab');
    assert.equal(await page.getByRole('tab', { selected: true }).textContent(), '● sample.md');
    await page.keyboard.press('Control+w');
    await page.locator('#dirty-dialog [data-choice="cancel"]').click();
    assert.equal(await page.getByRole('tab').count(), 4);
    await page.keyboard.press('Control+w');
    await page.locator('#dirty-dialog [data-choice="discard"]').click();
    assert.equal(await page.getByRole('tab').count(), 3);
    await page.evaluate(() => { window.testPaths = ['C:\\notes\\one.log', 'C:\\notes\\two.log']; });
    await page.keyboard.press('Control+o');
    await page.getByRole('tab', { name: 'two.log' }).waitFor();
    assert.equal(await page.getByRole('tab').count(), 5);
    await page.screenshot({ path: 'target/ui-markdown-fold.png' });
    assert.deepEqual(errors, []);
    console.log('Feature smoke passed: menus, theme, resizing, plugins, JSON/Markdown folding, isolated HTML CSS/scripts, persistent tabs, dirty close, multi-select and shortcuts.');
    await page.close();
  } finally { await browser.close(); server.close(); }
})().catch(error => { console.error(error); server.close(); process.exitCode = 1; });
