import test from 'node:test';
import assert from 'node:assert/strict';
const security = await import('../ui/security.mjs').catch(() => ({}));
test('external URL policy allows only absolute http, https and mailto', () => {
  assert.equal(typeof security.externalUrl, 'function', 'External link policy must exist');
  for (const url of ['javascript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', '//evil.test', '/path', 'https:\\evil.test', 'java\nscript:alert(1)']) assert.equal(security.externalUrl(url), null, url);
  for (const url of ['http://example.com/', 'https://example.com/a', 'mailto:person@example.com']) assert.equal(security.externalUrl(url), url);
});
test('blob URL is bound to the current handle and encodes opaque blob ids', () => {
  assert.equal(typeof security.blobUrl, 'function', 'Controlled blob URL builder must exist');
  assert.equal(security.blobUrl('h1', '../pic?x', 'Win32'), 'http://vw-blob.localhost/h1/..%2Fpic%3Fx');
  assert.equal(security.blobUrl('h1', 'b1', 'Linux'), 'vw-blob://localhost/h1/b1');
  assert.equal(security.blobUrl('', 'b1', 'Win32'), null);
});
