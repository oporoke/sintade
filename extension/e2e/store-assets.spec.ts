import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { BrowserContext, Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { signUpAndLogIn } from './support';

/**
 * Regenerates the store listing images in docs/store/assets/ from the real extension and app:
 * `STORE_ASSETS=1 npx playwright test e2e/store-assets.spec.ts --project=chrome`. Not part of the
 * normal run.
 */
test.skip(!process.env['STORE_ASSETS'], 'Run with STORE_ASSETS=1 to regenerate the store images');

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'docs', 'store', 'assets');
const TARGET = 'https://demo-dashboard.example/';

const DEMO_PAGE = `<!doctype html><title>Release dashboard</title>
<style>
  body { margin:0; font: 20px system-ui, sans-serif; background:#f5f7fb; color:#14171f }
  header { background:#fff; padding:18px 32px; border-bottom:1px solid #dfe3ec; font-weight:700 }
  main { padding:32px; display:grid; grid-template-columns: repeat(3, 1fr); gap:24px }
  .card { background:#fff; border-radius:14px; padding:24px; box-shadow:0 1px 3px #0002 }
  .big { font-size:56px; font-weight:800; color:#3b63e6 }
  .bars { display:flex; align-items:flex-end; gap:10px; height:180px; grid-column: span 3 }
  .bars i { flex:1; min-width:10px; background:#3b63e6; border-radius:6px 6px 0 0; animation: grow 2.4s ease-in-out infinite alternate }
  @keyframes grow { from { transform: scaleY(.55); transform-origin: bottom } to { transform: scaleY(1); transform-origin: bottom } }
</style>
<header>Release dashboard</header>
<main>
  <div class="card"><div class="big" id="n">0</div>deploys this week</div>
  <div class="card"><div class="big">99.9%</div>uptime</div>
  <div class="card"><div class="big">12</div>open reviews</div>
  <div class="card bars">${Array.from({ length: 14 }, (_, i) => `<i style="height:${40 + ((i * 37) % 60)}%;animation-delay:${i * 0.12}s"></i>`).join('')}</div>
</main>
<script>let n=0;setInterval(()=>{document.getElementById('n').textContent=String(40+(n++%7))},400)</script>`;

interface State {
  phase: string;
  url?: string;
}

async function composite(
  context: BrowserContext,
  size: { width: number; height: number },
  file: string,
  html: string,
): Promise<void> {
  const page = await context.newPage();
  await page.setViewportSize(size);
  await page.setContent(html, { waitUntil: 'load' });
  await page.screenshot({ path: join(OUT, file), type: 'png' });
  await page.close();
}

const FRAME = (
  headline: string,
  sub: string,
  image: string,
  side: 'popup' | 'wide',
) => `<!doctype html>
<body style="margin:0;font-family:system-ui,sans-serif;background:linear-gradient(135deg,#0b0d12,#1a2140);color:#e6e8eb;height:100vh;display:flex;align-items:center;gap:56px;padding:0 80px;box-sizing:border-box">
  <div style="flex:1"><h1 style="font-size:54px;margin:0 0 16px;line-height:1.1">${headline}</h1><p style="font-size:24px;color:#9aa1ad;margin:0">${sub}</p></div>
  <img src="${image}" style="${side === 'popup' ? 'height:640px;border-radius:14px;box-shadow:0 20px 60px #0008' : 'width:720px;border-radius:14px;box-shadow:0 20px 60px #0008'}">
</body>`;

async function dataUrl(page: Page, selector?: string): Promise<string> {
  const buffer = selector
    ? await page.locator(selector).screenshot({ type: 'png' })
    : await page.screenshot({ type: 'png' });
  return `data:image/png;base64,${buffer.toString('base64')}`;
}

test('store images from the real extension', async ({ context, extensionId }, testInfo) => {
  test.skip(testInfo.project.name !== 'chrome', 'one browser is enough for the images');
  test.setTimeout(240_000);
  mkdirSync(OUT, { recursive: true });
  await context.route(`${TARGET}**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: DEMO_PAGE }),
  );
  const app = await context.newPage();
  await app.setViewportSize({ width: 1280, height: 800 });
  await signUpAndLogIn(context, app);
  const target = await context.newPage();
  await target.setViewportSize({ width: 1280, height: 800 });
  await target.goto(TARGET);

  // The popup is 320 px wide; photograph it at 2x and place it in a branded frame.
  const popup = await context.newPage();
  await popup.setViewportSize({ width: 320, height: 520 });
  const probe = await context.newPage();
  await probe.goto(`chrome-extension://${extensionId}/popup.html`);
  await target.bringToFront();
  const tabId = await probe.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab?.id ?? null;
  });
  await probe.close();
  await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
  await expect(popup.getByTestId('popup-record')).toBeEnabled();
  const idle = await dataUrl(popup, 'main');

  await composite(
    context,
    { width: 1280, height: 800 },
    'screenshot-1-record.png',
    FRAME(
      'Record any tab in three clicks',
      'Click the button, press Record, press Stop.',
      idle,
      'popup',
    ),
  );

  await target.bringToFront();
  await popup.getByTestId('popup-record').dispatchEvent('click');
  await expect(popup.getByTestId('popup-recording')).toContainText(/Recording 0:0\d/, {
    timeout: 20_000,
  });
  await popup.bringToFront();
  await target.waitForTimeout(5_000);
  const recording = await dataUrl(popup, 'main');
  await composite(
    context,
    { width: 1280, height: 800 },
    'screenshot-2-recording.png',
    FRAME(
      'It uploads while you record',
      'Your link is ready seconds after you stop.',
      recording,
      'popup',
    ),
  );

  await popup.getByTestId('popup-stop').click();
  await expect(popup.getByTestId('popup-result')).toBeVisible({ timeout: 60_000 });
  const url = (await popup.getByTestId('popup-link').getAttribute('href')) ?? '';
  const done = await dataUrl(popup, 'main');
  await composite(
    context,
    { width: 1280, height: 800 },
    'screenshot-3-link.png',
    FRAME('The link is copied for you', 'Paste it in chat, email or a ticket.', done, 'popup'),
  );

  // The finished video on its watch page, and the library.
  const watch = await context.newPage();
  await watch.setViewportSize({ width: 1280, height: 800 });
  await watch.goto(url);
  await expect(watch.getByTestId('watch-video')).toBeVisible({ timeout: 30_000 });
  await expect(watch.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });
  await watch.getByTestId('watch-video').evaluate(async (el: HTMLVideoElement) => {
    el.currentTime = 2;
    await new Promise((resolve) => el.addEventListener('seeked', resolve, { once: true }));
  });
  await watch.waitForTimeout(500);
  await watch.screenshot({ path: join(OUT, 'screenshot-4-watch.png'), type: 'png' });

  await app.goto('https://localhost:4200/library');
  await expect(app.getByTestId('library-item').first()).toBeVisible();
  await expect(app.locator('[data-testid="library-item"] img').first()).toBeVisible();
  await app.screenshot({ path: join(OUT, 'screenshot-5-library.png'), type: 'png' });

  // Promotional tiles.
  const iconUrl = `chrome-extension://${extensionId}/icons/icon-128.png`;
  const tile = (w: number, h: number, big: number, small: number) => `<!doctype html>
<body style="margin:0;width:${w}px;height:${h}px;font-family:system-ui,sans-serif;background:linear-gradient(135deg,#0b0d12,#24305e);color:#fff;display:flex;align-items:center;justify-content:center;gap:${Math.round(w / 28)}px">
  <img src="${iconUrl}" style="width:${Math.round(h * 0.34)}px;height:${Math.round(h * 0.34)}px">
  <div><div style="font-size:${big}px;font-weight:800;line-height:1">Sintade</div>
  <div style="font-size:${small}px;color:#b9c3e6;margin-top:${Math.round(small / 2)}px">Record any tab.<br>Share a link in seconds.</div></div>
</body>`;
  const tilePage = await context.newPage();
  await tilePage.goto(`chrome-extension://${extensionId}/popup.html`); // same origin as the icon
  for (const [file, w, h, big, small] of [
    ['promo-small-440x280.png', 440, 280, 56, 20],
    ['promo-marquee-1400x560.png', 1400, 560, 120, 40],
  ] as const) {
    await tilePage.setViewportSize({ width: w, height: h });
    await tilePage.setContent(tile(w, h, big, small), { waitUntil: 'load' });
    await tilePage.screenshot({ path: join(OUT, file), type: 'png' });
  }
  const state = (await popup.evaluate(() =>
    chrome.runtime.sendMessage({ type: 'recording-status' }),
  )) as {
    state: State;
  };
  expect(state.state.phase).toBe('done');
});
