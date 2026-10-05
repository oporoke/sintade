import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { BrowserContext, test as base, chromium } from '@playwright/test';

import { DEV_EXTENSION_ID } from './ids';

const loadedIds = new WeakMap<BrowserContext, string>();

export const DIST = join(dirname(fileURLToPath(import.meta.url)), '..', 'dist');

interface Fixtures {
  /** A browser with the built extension loaded unpacked. */
  context: BrowserContext;
  /** The loaded extension's id, read from its service worker's URL. */
  extensionId: string;
}

export const test = base.extend<Fixtures>({
  // eslint-disable-next-line no-empty-pattern
  context: async ({}, use, testInfo) => {
    const profile = await mkdtemp(join(tmpdir(), 'sintade-ext-'));
    const channel = testInfo.project.use.channel;
    // Edge honours --load-extension. Branded Chrome 137+ ignores it, so Chrome gets the
    // extension over the DevTools protocol (`Extensions.loadUnpacked`, which needs the
    // --enable-unsafe-extension-debugging switch).
    const viaFlag = channel !== 'chrome';
    const context = await chromium.launchPersistentContext(profile, {
      channel,
      headless: true,
      ignoreHTTPSErrors: true,
      // Playwright passes --disable-extensions by default.
      ignoreDefaultArgs: ['--disable-extensions'],
      args: [
        '--enable-unsafe-extension-debugging',
        // The dev server's certificate is self-signed.
        '--ignore-certificate-errors',
        // Tab capture normally needs a click on the toolbar button; these let a test start it.
        `--allowlisted-extension-id=${DEV_EXTENSION_ID}`,
        '--auto-accept-this-tab-capture',
        '--autoplay-policy=no-user-gesture-required',
        ...(viaFlag ? [`--disable-extensions-except=${DIST}`, `--load-extension=${DIST}`] : []),
      ],
    });
    if (!viaFlag) {
      const browser = context.browser();
      if (!browser) {
        throw new Error('no browser handle to load the extension with');
      }
      const session = await browser.newBrowserCDPSession();
      const loaded = (await session.send(
        'Extensions.loadUnpacked' as never,
        { path: DIST } as never,
      )) as unknown as { id: string };
      loadedIds.set(context, loaded.id);
    }
    try {
      await use(context);
    } finally {
      await context.close();
      await rm(profile, { recursive: true, force: true });
    }
  },
  extensionId: async ({ context }, use) => {
    const known = loadedIds.get(context);
    if (known) {
      await use(known);
      return;
    }
    let [worker] = context.serviceWorkers();
    worker ??= await context.waitForEvent('serviceworker', { timeout: 15_000 });
    await use(new URL(worker.url()).host);
  },
});

export { expect } from '@playwright/test';
