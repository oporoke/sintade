import { signUpAndLogIn } from './support/auth';
import { expect, test } from './support/test';

/**
 * Day 76 Check: "auto-stop at plan limit uploads cleanly". The create-recording response is
 * rewritten to a 9 s plan limit (the real one is 10 minutes), so the recorder's own limit logic
 * runs for real: a warning appears, the take stops by itself just inside the limit, uploads and
 * finalizes on the real server, which accepts it.
 */
test('auto-stop at the plan limit warns, stops and uploads cleanly', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(60_000);
  await signUpAndLogIn(page, request);
  await page.route('**/api/v1/recordings', async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const response = await route.fetch();
    const body = await response.json();
    await route.fulfill({ response, json: { ...body, max_duration_ms: 9_000 } });
  });
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');

  await expect(page.getByTestId('recorder-limit-warning')).toBeVisible({ timeout: 5_000 });
  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId('recorder-limit-notice')).toContainText(
    'so this one stopped there',
  );
  await expect(summary).toHaveAttribute('data-uploaded', 'true');
  const takeId = await summary.getAttribute('data-take-id');
  const status = await page.request.get(`/api/v1/takes/${takeId}/status`);
  const body = (await status.json()) as { finalized: boolean };
  expect(body.finalized).toBe(true);
});

/** Keyboard shortcuts and discard: a discarded take leaves nothing on the device or the server. */
test('shortcuts pause and discard; a discarded take is gone from the device and the library', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(2500);

  await page.keyboard.press('p');
  await expect(page.getByTestId('recorder-status')).toHaveText('Paused');
  await page.keyboard.press('p');
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');

  await page.keyboard.press('d');
  await expect(page.getByTestId('recorder-confirm')).toBeVisible();
  await page.getByTestId('recorder-confirm-yes').click();
  await expect(page.getByTestId('recorder-start')).toBeVisible();

  const left = await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const chunks = await root.getDirectoryHandle('sintade-chunks', { create: true });
    const names: string[] = [];
    for await (const [name] of chunks.entries()) names.push(name);
    return names;
  });
  expect(left).toEqual([]);
  const list = await page.request.get('/api/v1/recordings');
  const body = (await list.json()) as { items: unknown[] };
  expect(body.items).toEqual([]);
});

/**
 * Day 77 Check: "warning at 80% quota; laptop does not sleep". The browser's storage estimate is
 * replaced by one at 85 % used (the warning must show before and during the take) and the Wake
 * Lock API is wrapped to count what is held: one screen lock while recording, none after Stop.
 * Also: closing the tab mid-take asks for confirmation.
 */
test('low storage warns, the screen is kept awake while recording, and leaving asks first', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  await page.addInitScript(() => {
    const w = window as unknown as { __locks: { held: number; requests: number } };
    w.__locks = { held: 0, requests: 0 };
    const original = navigator.wakeLock;
    if (original) {
      const request = original.request.bind(original);
      Object.defineProperty(navigator, 'wakeLock', {
        value: {
          request: async (type: 'screen') => {
            w.__locks.requests += 1;
            const sentinel = await request(type);
            w.__locks.held += 1;
            sentinel.addEventListener('release', () => (w.__locks.held -= 1));
            return sentinel;
          },
        },
        configurable: true,
      });
    }
    navigator.storage.estimate = async () => ({ usage: 850_000_000, quota: 1_000_000_000 });
    window.addEventListener('beforeunload', (event) => {
      (window as unknown as { __unloadPrevented: boolean }).__unloadPrevented =
        event.defaultPrevented;
    });
  });
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  const hasWakeLock = await page.evaluate(() => 'wakeLock' in navigator);
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await expect(page.getByTestId('recorder-storage-warning')).toContainText('85% of the browser');

  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await expect(page.getByTestId('recorder-storage-warning')).toBeVisible();
  if (hasWakeLock) {
    await expect
      .poll(() => page.evaluate(() => (window as any).__locks.held)) // eslint-disable-line @typescript-eslint/no-explicit-any
      .toBe(1);
  }
  // Leaving mid-take is intercepted.
  const prevented = await page.evaluate(() => {
    const event = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(true);

  await page.waitForTimeout(2000);
  await page.getByTestId('recorder-stop').click();
  await expect(page.getByTestId('recorder-done-summary')).toBeVisible({ timeout: 20_000 });
  if (hasWakeLock) {
    await expect
      .poll(() => page.evaluate(() => (window as any).__locks.held)) // eslint-disable-line @typescript-eslint/no-explicit-any
      .toBe(0);
  }
});
