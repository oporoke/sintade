import { Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { signUpAndLogIn } from './support';

const TARGET = 'https://overlay-tab.example/';
const CLICK = { x: 640, y: 420 };

/** A magenta page with a counter in the corner (so nothing else in the frame is amber or black). */
const TARGET_HTML = `<!doctype html><title>Overlay tab</title>
<body style="margin:0;height:100vh;background:#ff00ff;color:#fff;font:48px sans-serif">
<p id="n" style="margin:8px">0</p>
<input id="pw" type="password" style="position:fixed;left:40px;bottom:200px;width:300px;font-size:28px">
<script>let n=0;(function tick(){document.getElementById('n').textContent=String(n++);requestAnimationFrame(tick)})();</script>`;

interface RecState {
  phase: string;
  url?: string;
  message?: string;
}

async function send<T>(popup: Page, message: object): Promise<T> {
  return popup.evaluate((m) => chrome.runtime.sendMessage(m), message) as Promise<T>;
}

/**
 * Day 64 Check: "highlights visible in the output video". A tab is recorded through the
 * extension while the mouse is pressed in it and keys are typed; the finished MP4 on the watch
 * page has amber click rings at the click position and the dark keystroke label at the bottom,
 * and no ring before the first click. Text typed into a password field is not shown.
 */
test('click highlights and keystrokes appear in the recorded video', async ({
  context,
  extensionId,
}) => {
  test.setTimeout(150_000);
  await context.route(`${TARGET}**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: TARGET_HTML }),
  );
  const app = await context.newPage();
  await signUpAndLogIn(context, app);

  const target = await context.newPage();
  await target.goto(TARGET);
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  await popup.evaluate(() =>
    chrome.storage.local.set({ settings: { highlights: true, keystrokes: true } }),
  );
  await target.bringToFront();
  const tabId = await popup.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab?.id ?? null;
  });
  await send(popup, { type: 'start-recording', tabId });
  await expect
    .poll(
      async () =>
        (await send<{ state: RecState }>(popup, { type: 'recording-status' })).state.phase,
    )
    .toBe('recording');
  // The overlay script goes in once the stream is recording.
  await target.waitForTimeout(3_000);

  // Three clicks, a shortcut, and a password typed into the password field.
  for (let i = 0; i < 3; i += 1) {
    await target.mouse.click(CLICK.x, CLICK.y);
    await target.waitForTimeout(400);
  }
  await target.keyboard.press('Control+k');
  await target.waitForTimeout(1_200);
  await target.locator('#pw').click();
  await target.keyboard.type('hunter2');
  await target.waitForTimeout(2_000);

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

  const watch = await context.newPage();
  await watch.goto(done.url ?? '');
  const video = watch.getByTestId('watch-video');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(watch.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });
  await expect(video).toHaveAttribute('src', /mp4\/default\.mp4/);

  // Look at the video, frame by frame.
  const found = await video.evaluate(async (el: HTMLVideoElement, click) => {
    el.pause();
    const scale = el.videoWidth / 1280;
    const seek = (t: number) =>
      new Promise<void>((resolve) => {
        el.addEventListener('seeked', () => resolve(), { once: true });
        el.currentTime = t;
      });
    const canvas = document.createElement('canvas');
    const draw = (sx: number, sy: number, w: number, h: number) => {
      canvas.width = w;
      canvas.height = h;
      const c = canvas.getContext('2d', { willReadFrequently: true });
      c?.drawImage(el, sx, sy, w, h, 0, 0, w, h);
      return c?.getImageData(0, 0, w, h).data ?? new Uint8ClampedArray();
    };
    const rings: number[] = [];
    const labels: number[] = [];
    const passwordLabels: number[] = [];
    for (let t = 0.1; t < el.duration - 0.1; t += 0.1) {
      await seek(t);
      // Amber: red and green high, blue low. The magenta page and white text have none.
      const ring = draw(
        Math.round((click.x - 50) * scale),
        Math.round((click.y - 50) * scale),
        Math.round(100 * scale),
        Math.round(100 * scale),
      );
      let amber = 0;
      for (let i = 0; i < ring.length; i += 4) {
        if ((ring[i] ?? 0) > 200 && (ring[i + 1] ?? 0) > 140 && (ring[i + 2] ?? 255) < 110) {
          amber += 1;
        }
      }
      if (amber >= 8) rings.push(Math.round(t * 100) / 100);
      // Near-black: the keystroke label, bottom centre. The page has no dark pixels.
      const w = Math.round(400 * scale);
      const h = Math.round(90 * scale);
      const label = draw(
        Math.round((el.videoWidth - w) / 2),
        el.videoHeight - h - Math.round(20 * scale),
        w,
        h,
      );
      let dark = 0;
      for (let i = 0; i < label.length; i += 4) {
        if ((label[i] ?? 255) < 45 && (label[i + 1] ?? 255) < 45 && (label[i + 2] ?? 255) < 45) {
          dark += 1;
        }
      }
      if (dark >= 400) labels.push(Math.round(t * 100) / 100);
      passwordLabels.push(0);
    }
    return { rings, labels, duration: el.duration };
  }, CLICK);

  expect(found.rings.length, `ring frames: ${found.rings}`).toBeGreaterThanOrEqual(2);
  expect(found.labels.length, `label frames: ${found.labels}`).toBeGreaterThanOrEqual(2);
  // Nothing before the first click (the page had been recording for ~3 s by then).
  expect(Math.min(...found.rings)).toBeGreaterThan(1.5);
  expect(Math.min(...found.labels)).toBeGreaterThan(1.5);
  // The label is up only for the shortcut (~1.5 s). Seven characters typed into the password
  // field a moment later would keep it up for several more seconds if they were shown.
  const labelSpan = Math.max(...found.labels) - Math.min(...found.labels);
  expect(labelSpan, `label frames: ${found.labels}`).toBeLessThan(2.2);
});
