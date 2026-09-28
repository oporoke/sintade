import { Page } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';
import { keepTakesOnDevice } from './support/offline';
import { expect, test } from './support/test';

/**
 * Day 40 Check: "Recovered take finalizes with no gaps". A tab dies mid-recording; reopening the
 * app offers the take, and Upload sends what the server lacks, finalizes, and clears it.
 */
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);

async function recordThenKill(page: Page, ms: number): Promise<void> {
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape'); // skip the countdown
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(ms);
  // The "crash": no unload handlers run; only what was persisted survives.
  await page.close({ runBeforeUnload: false });
}

async function uploadRecovered(page: Page) {
  await page.goto('/record');
  const dialog = page.getByTestId('recovery-dialog');
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  await expect(dialog.getByTestId('recovery-take')).toHaveCount(1);
  await dialog.getByTestId('recovery-upload').click();
  const uploaded = dialog.getByTestId('recovery-uploaded');
  const error = dialog.getByTestId('recovery-error');
  await expect(uploaded.or(error)).toBeVisible({ timeout: 30_000 });
  await expect(error).toHaveCount(0);
  const serverTakeId = await uploaded.getAttribute('data-server-take-id');
  const chunkCount = Number(await uploaded.getAttribute('data-chunk-count'));
  const status = await page.request.get(`/api/v1/takes/${serverTakeId}/status`);
  expect(status.ok()).toBe(true);
  const body = (await status.json()) as { finalized: boolean; received: number[] };
  return { serverTakeId, chunkCount, body };
}

test('a take whose upload was cut off is recovered with no gaps', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);

  // Storage accepts the first two chunks, then the connection to it dies.
  const recording = await context.newPage();
  let puts = 0;
  await recording.route('**/sintade-dev/**', (route) => {
    if (route.request().method() !== 'PUT') return route.continue();
    puts += 1;
    return puts <= 2 ? route.continue() : route.abort('internetdisconnected');
  });
  await recordThenKill(recording, 9_000);
  expect(puts).toBeGreaterThan(2);

  const { chunkCount, body } = await uploadRecovered(page);
  test.info().annotations.push({ type: 'recovered', description: `${chunkCount} chunks` });
  expect(chunkCount).toBeGreaterThanOrEqual(4);
  expect(body.finalized).toBe(true);
  expect(body.received).toEqual(Array.from({ length: chunkCount }, (_, idx) => idx));
});

test('a take recorded while the server was unreachable is recovered as a new recording', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);

  const recording = await context.newPage();
  await keepTakesOnDevice(recording);
  await recordThenKill(recording, 5_000);

  const { chunkCount, body } = await uploadRecovered(page);
  expect(chunkCount).toBeGreaterThanOrEqual(2);
  expect(body.finalized).toBe(true);
  expect(body.received).toEqual(Array.from({ length: chunkCount }, (_, idx) => idx));
});
