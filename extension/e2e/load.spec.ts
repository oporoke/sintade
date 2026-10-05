import { expect, test } from './fixtures';

/**
 * Day 61 Check: "loads unpacked in Chrome and Edge". The built extension is loaded into each
 * browser; its service worker starts, the manifest is the one we built, and the popup opens,
 * talks to the service worker and shows the version.
 */
test('loads unpacked, starts its service worker and opens the popup', async ({
  context,
  extensionId,
}) => {
  expect(extensionId).toMatch(/^[a-p]{32}$/);

  // Opening a page of the extension also wakes its service worker (Chrome starts it lazily).
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  const worker =
    context.serviceWorkers().find((w) => w.url().includes(extensionId)) ??
    (await context.waitForEvent('serviceworker', { timeout: 15_000 }));
  const manifest = await worker.evaluate(() => chrome.runtime.getManifest());
  expect(manifest.manifest_version).toBe(3);
  expect(manifest.name).toBe('Sintade');
  expect([...(manifest.permissions ?? [])].sort()).toEqual([
    'activeTab',
    'offscreen',
    'scripting',
    'storage',
    'tabCapture',
  ]);

  await expect(popup.getByTestId('popup-title')).toHaveText('Sintade');
  // It reached the service worker and the server answered (nobody is signed in here).
  await expect(popup.getByTestId('popup-status')).toHaveAttribute(
    'data-state',
    /^(signed-out|unreachable)$/,
  );
  await expect(popup.getByTestId('popup-version')).toHaveText(`Version ${manifest.version}`);
});
