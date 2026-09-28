import { signUpAndLogIn } from './support/auth';
import { expect, test } from './support/test';

/**
 * Day 38 Check: "Throttled/offline network test recovers". `/debug`'s streamed upload writes a
 * chunk every 0.7 s (like a recorder) and uploads it with the real `Uploader` while the test
 * slows storage down, makes it fail, and takes the browser offline. Every chunk must still
 * reach the server with the right hash.
 */
test('uploads recover from a throttled, failing and offline network', async ({
  page,
  context,
  request,
  browserName,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);
  await page.goto('/debug');

  // Throttle every storage PUT by 600 ms, and fail the first two with 503.
  let failuresLeft = 2;
  let puts = 0;
  let cut = false;
  await page.route('**/sintade-dev/**', async (route) => {
    if (cut) {
      return route.abort('internetdisconnected');
    }
    if (route.request().method() !== 'PUT') {
      return route.continue();
    }
    puts += 1;
    await new Promise((resolve) => setTimeout(resolve, 600));
    if (failuresLeft > 0) {
      failuresLeft -= 1;
      return route.fulfill({ status: 503, body: 'Slow Down' });
    }
    return route.continue();
  });

  await page.getByTestId('upload-selftest-stream').click();
  const progress = page.getByTestId('upload-selftest-progress');
  await expect(progress).toHaveAttribute('data-uploaded', /^[1-9]/, { timeout: 30_000 });

  // Drop the network while chunks keep being written locally. Playwright's WebKit offline
  // emulation also breaks reading in-memory Blobs ("The I/O read operation failed"), which real
  // Safari doesn't do, so there the cut is every API and storage request failing instead.
  const emulateOffline = browserName !== 'webkit';
  if (emulateOffline) {
    await context.setOffline(true);
    await expect(progress).toHaveAttribute('data-state', 'offline', { timeout: 10_000 });
  } else {
    cut = true;
    await page.route('**/api/v1/**', (route) =>
      cut ? route.abort('internetdisconnected') : route.continue(),
    );
    await expect(progress).toHaveAttribute('data-state', /retrying|uploading/, { timeout: 10_000 });
  }
  await page.waitForTimeout(1_000);
  const uploadedWhileOffline = Number(await progress.getAttribute('data-uploaded'));
  await page.waitForTimeout(3_000);
  expect(Number(await progress.getAttribute('data-uploaded'))).toBe(uploadedWhileOffline);
  if (emulateOffline) {
    await context.setOffline(false);
  } else {
    cut = false;
  }

  const summary = page.getByTestId('upload-selftest-summary');
  const error = page.getByTestId('upload-selftest-error');
  await expect(summary.or(error)).toBeVisible({ timeout: 45_000 });
  await expect(error).toHaveCount(0);
  await expect(summary).toContainText('8 chunks uploaded');
  await expect(summary).toContainText('every hash matches the server record');
  expect(failuresLeft).toBe(0);
  expect(puts).toBeGreaterThanOrEqual(8 + 2);

  const takeId = await summary.getAttribute('data-take-id');
  const status = await page.request.get(`/api/v1/takes/${takeId}/status`);
  const body = (await status.json()) as { received: number[] };
  expect(body.received).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
  test.info().annotations.push({
    type: 'recovery',
    description: `offline after ${uploadedWhileOffline} chunk(s); ${puts} PUTs for 8 chunks`,
  });
});
