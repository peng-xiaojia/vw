import test from 'node:test';
import assert from 'node:assert/strict';
import { effectiveTheme } from '../ui/theme.mjs';
import { clampPaneWidth } from '../ui/resizer.mjs';

test('theme follows system and crosses midnight on schedule', () => {
  const settings = { mode: 'system', darkHour: '19:00', lightHour: '07:00' };
  assert.equal(effectiveTheme(settings, true), 'dark');
  assert.equal(effectiveTheme(settings, false), 'light');
  settings.mode = 'schedule';
  assert.equal(effectiveTheme(settings, false, new Date(2026, 0, 1, 23, 30)), 'dark');
  assert.equal(effectiveTheme(settings, false, new Date(2026, 0, 2, 6, 59)), 'dark');
  assert.equal(effectiveTheme(settings, true, new Date(2026, 0, 2, 7, 0)), 'light');
  settings.mode = 'light';
  assert.equal(effectiveTheme(settings, true), 'light');
});

test('pane width respects its bounds and leaves room for document', () => {
  assert.equal(clampPaneWidth(80, 1100, 170, 520), 170);
  assert.equal(clampPaneWidth(600, 1100, 170, 520), 520);
  assert.equal(clampPaneWidth(500, 700, 170, 520), 420);
});
