import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

import { checkManifest } from './manifest-check.mjs';
import { zip } from './zip.mjs';

const good = {
  manifest_version: 3,
  name: 'Sintade',
  description: 'Record any tab and get a shareable link in seconds.',
  version: '1.0.0',
  permissions: ['activeTab', 'tabCapture', 'offscreen', 'storage', 'scripting'],
  host_permissions: ['https://app.sintade.com/*'],
  icons: { '16': 'a.png', '32': 'b.png', '48': 'c.png', '128': 'd.png' },
  background: { service_worker: 'service-worker.js' },
  action: { default_popup: 'popup.html' },
};
const exists = () => true;

describe('checkManifest', () => {
  it('accepts a store manifest', () => {
    expect(checkManifest(good, exists)).toEqual([]);
  });

  it.each([
    [
      'an extra permission',
      { permissions: [...good.permissions, 'tabs'] },
      /permissions must be exactly/,
    ],
    ['a missing permission', { permissions: ['activeTab'] }, /permissions must be exactly/],
    ['a development key', { key: 'MIIB' }, /e2e build only/],
    ['a localhost host', { host_permissions: ['https://localhost:4200/*'] }, /development or test/],
    ['a wildcard host', { host_permissions: ['https://*/*'] }, /development or test/],
    ['all urls', { host_permissions: ['<all_urls>'] }, /https:\/\/<origin>/],
    ['a plain http host', { host_permissions: ['http://app.sintade.com/*'] }, /https:\/\//],
    [
      'two hosts',
      { host_permissions: ['https://a.com/*', 'https://b.com/*'] },
      /one Sintade origin/,
    ],
    [
      'a test-page host',
      { host_permissions: ['https://app.sintade.com/*', 'https://*.example/*'] },
      /one Sintade origin/,
    ],
    ['content scripts', { content_scripts: [] }, /content_scripts is not used/],
    ['a long description', { description: 'x'.repeat(133) }, /132/],
    ['a bad version', { version: 'v1' }, /version/],
    ['manifest v2', { manifest_version: 2 }, /must be 3/],
  ])('rejects %s', (_label, change, expected) => {
    expect(checkManifest({ ...good, ...change }, exists).join('\n')).toMatch(expected);
  });

  it('rejects missing files', () => {
    const problems = checkManifest(good, (path) => path !== 'd.png' && path !== 'popup.html');
    expect(problems.join('\n')).toMatch(/icon 128/);
    expect(problems.join('\n')).toMatch(/popup page/);
  });

  it('rejects the reserved test domains unless explicitly allowed', () => {
    const m = { ...good, host_permissions: ['https://app.sintade.test/*'] };
    delete process.env['SINTADE_ALLOW_TEST_ORIGIN'];
    expect(checkManifest(m, exists).join()).toMatch(/development or test/);
    process.env['SINTADE_ALLOW_TEST_ORIGIN'] = '1';
    expect(checkManifest(m, exists)).toEqual([]);
    delete process.env['SINTADE_ALLOW_TEST_ORIGIN'];
  });
});

describe('zip', () => {
  it('writes an archive that unzip reads back byte for byte, reproducibly', () => {
    const files = [
      { name: 'manifest.json', data: Buffer.from('{"a":1}') },
      {
        name: 'icons/icon-16.png',
        data: Buffer.from(Array.from({ length: 5000 }, (_, i) => i % 251)),
      },
      { name: 'popup.html', data: Buffer.from('<html>é</html>') },
    ];
    const first = zip(files);
    expect(zip(files).equals(first)).toBe(true);
    const dir = mkdtempSync(join(tmpdir(), 'zip-'));
    const path = join(dir, 'a.zip');
    writeFileSync(path, first);
    expect(execFileSync('unzip', ['-tq', path]).toString()).toContain('No errors');
    execFileSync('unzip', ['-q', path, '-d', join(dir, 'out')]);
    for (const { name, data } of files) {
      expect(readFileSync(join(dir, 'out', name)).equals(data)).toBe(true);
    }
  });
});

describe('npm run package', () => {
  it('refuses to run without a production origin', () => {
    const run = spawnSync(process.execPath, ['scripts/package.mjs'], {
      env: { ...process.env, SINTADE_ORIGIN: '' },
      encoding: 'utf8',
    });
    expect(run.status).toBe(2);
    expect(run.stderr).toContain('SINTADE_ORIGIN');
  });
});
