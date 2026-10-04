import { Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { APP, signUpAndLogIn } from './support';

const TARGET = 'https://tab-under-test.example/';

/** A page that is visibly alive: a magenta background and a counter that changes every frame. */
const TARGET_HTML = `<!doctype html><title>Tab under test</title>
<body style="margin:0;background:#ff00ff;color:#fff;font:64px sans-serif">
<p id="n">0</p>
<script>let n=0;(function tick(){document.getElementById('n').textContent=String(n++);requestAnimationFrame(tick)})();</script>`;

interface RecState {
  phase: string;
  url?: string;
  recordingId?: string;
  message?: string;
}

async function send<T>(popup: Page, message: object): Promise<T> {
  return popup.evaluate((m) => chrome.runtime.sendMessage(m), message) as Promise<T>;
}

/**
 * Day 63 Check: "tab recording from the extension plays on Sintade". A signed-in user records a
 * tab through the extension (offscreen document, shared capture engine, same ingest as the web
 * recorder); stops; and the link it returns plays on the watch page, showing that tab.
 */
test('records a tab through the extension and the link plays on Sintade', async ({
  context,
  extensionId,
}) => {
  test.setTimeout(120_000);
  await context.route(`${TARGET}**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: TARGET_HTML }),
  );
  const app = await context.newPage();
  await signUpAndLogIn(context, app);

  const target = await context.newPage();
  await target.goto(TARGET);
  await target.bringToFront();

  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  await target.bringToFront();
  const tabId = await popup.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab?.id ?? null;
  });
  expect(tabId).not.toBeNull();

  const started = await send<{ state: RecState }>(popup, { type: 'start-recording', tabId });
  expect(['starting', 'recording']).toContain(started.state.phase);
  await expect
    .poll(
      async () =>
        (await send<{ state: RecState }>(popup, { type: 'recording-status' })).state.phase,
    )
    .toBe('recording');
  await target.waitForTimeout(8_000);

  await send(popup, { type: 'stop-recording' });
  let done: RecState = { phase: 'idle' };
  await expect
    .poll(
      async () => {
        done = (await send<{ state: RecState }>(popup, { type: 'recording-status' })).state;
        return done.phase;
      },
      { timeout: 60_000, intervals: [500] },
    )
    .toMatch(/^(done|error)$/);
  expect(done.phase, done.message).toBe('done');
  expect(done.url).toMatch(/\/s\/[0-9A-Za-z]{12}$/);

  // The link plays on Sintade, and what plays is the recorded tab.
  const watch = await context.newPage();
  await watch.goto(done.url ?? '');
  const video = watch.getByTestId('watch-video');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(watch.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });
  await expect(video).toHaveAttribute('src', /mp4\/default\.mp4/);
  const seconds = await video.evaluate((el: HTMLVideoElement) => el.duration);
  expect(seconds).toBeGreaterThan(6);
  expect(seconds).toBeLessThan(11);

  // A frame from the middle is the tab's magenta page, not a blank or a black picture.
  const colour = await video.evaluate(async (el: HTMLVideoElement) => {
    el.pause();
    el.currentTime = 4;
    await new Promise((resolve) => el.addEventListener('seeked', resolve, { once: true }));
    const canvas = document.createElement('canvas');
    canvas.width = 32;
    canvas.height = 32;
    const context2d = canvas.getContext('2d');
    context2d?.drawImage(el, el.videoWidth - 32, el.videoHeight - 32, 32, 32, 0, 0, 32, 32);
    const [r = 0, g = 0, b = 0] = context2d?.getImageData(8, 8, 1, 1).data ?? [];
    return { r, g, b };
  });
  expect(colour.r).toBeGreaterThan(200);
  expect(colour.g).toBeLessThan(80);
  expect(colour.b).toBeGreaterThan(200);
  expect(APP).toContain('localhost');
});
