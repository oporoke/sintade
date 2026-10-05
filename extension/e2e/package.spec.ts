import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test as base } from './fixtures';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** The zip a store would receive, unpacked: built with `npm run package`, not the e2e build. */
function packagedExtension(): string {
  execFileSync('npm', ['run', 'package'], {
    cwd: root,
    stdio: 'pipe',
    env: {
      ...process.env,
      SINTADE_ORIGIN: 'https://app.sintade.test',
      SINTADE_ALLOW_TEST_ORIGIN: '1',
    },
  });
  const out = mkdtempSync(join(tmpdir(), 'sintade-pkg-'));
  const version = JSON.parse(
    execFileSync('node', ['-p', "JSON.stringify(require('./package.json'))"], {
      cwd: root,
    }).toString(),
  ) as { version: string };
  execFileSync('unzip', [
    '-q',
    join(root, 'artifacts', `sintade-extension-${version.version}.zip`),
    '-d',
    out,
  ]);
  return out;
}

const dir = packagedExtension();
const test = base.extend({});
test.use({ extensionDir: dir });

/**
 * Day 66: the exact archive that is uploaded to the stores loads in Chrome and Edge, starts its
 * service worker, shows its popup and asks for nothing the privacy policy doesn't explain. (The
 * dev key and the test-page host permission of the e2e build are not in it.)
 */
test('the packaged extension loads unpacked, with the store manifest', async ({
  context,
  extensionId,
}) => {
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  await expect(popup.getByTestId('popup-title')).toHaveText('Sintade');
  // The production address isn't reachable here: the popup says so instead of failing.
  await expect(popup.getByTestId('popup-status')).toHaveAttribute('data-state', 'unreachable');

  const worker =
    context.serviceWorkers().find((w) => w.url().includes(extensionId)) ??
    (await context.waitForEvent('serviceworker', { timeout: 15_000 }));
  const manifest = await worker.evaluate(() => chrome.runtime.getManifest());
  expect([...(manifest.permissions ?? [])].sort()).toEqual([
    'activeTab',
    'offscreen',
    'scripting',
    'storage',
    'tabCapture',
  ]);
  expect(manifest.host_permissions).toEqual(['https://app.sintade.test/*']);
  expect('key' in manifest).toBe(false);
});
