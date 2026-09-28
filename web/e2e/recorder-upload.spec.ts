import { signUpAndLogIn } from './support/auth';
import { expect, test } from './support/test';

/** 10 Mbps each way, in bytes per second, with 20 ms of latency (CDP network emulation). */
const TEN_MBPS = (10 * 1_000_000) / 8;

/**
 * Day 39 Check: "Stop → finalize in under 2 s on 10 Mbps". Records through the real `/record`
 * page, which uploads every chunk as it is stored; after Stop the page drains the queue,
 * finalizes and clears the device's copy. On Chromium the browser's network is throttled to
 * 10 Mbps (CDP); Firefox has no network throttling in Playwright and runs unthrottled.
 */
test('stop → finalize in under 2 s on 10 Mbps, then the device copy is gone', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(60_000);
  await signUpAndLogIn(page, request);
  if (browserName === 'chromium') {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Network.enable');
    await cdp.send('Network.emulateNetworkConditions', {
      offline: false,
      latency: 20,
      downloadThroughput: TEN_MBPS,
      uploadThroughput: TEN_MBPS,
    });
  }

  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-mic-select').selectOption({ index: 1 });
  await expect(page.getByTestId('recorder-mic-info')).toContainText('working');
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape'); // skip the countdown
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(10_000);
  await page.getByTestId('recorder-stop').click();

  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId('recorder-upload-notice')).toHaveCount(0);
  await expect(summary).toHaveAttribute('data-uploaded', 'true');
  await expect(summary).toContainText('uploaded. Processing has started.');
  const stopToFinalizeMs = Number(await summary.getAttribute('data-stop-to-finalize-ms'));
  test.info().annotations.push({ type: 'stop → finalize', description: `${stopToFinalizeMs} ms` });
  expect(stopToFinalizeMs).toBeLessThan(2_000);

  // The server has every chunk and the take is finalized.
  const takeId = await summary.getAttribute('data-take-id');
  const status = await page.request.get(`/api/v1/takes/${takeId}/status`);
  const body = (await status.json()) as { finalized: boolean; received: number[] };
  expect(body.finalized).toBe(true);
  expect(body.received.length).toBeGreaterThanOrEqual(5);
  expect(body.received).toEqual(body.received.map((_, index) => index));

  // And the device no longer holds it (OPFS cleared, §10 Record step 8).
  const left = await page.evaluate(async (id) => {
    const root = await navigator.storage.getDirectory();
    const chunks = await root.getDirectoryHandle('sintade-chunks', { create: true });
    try {
      await chunks.getDirectoryHandle(id ?? '');
      return true;
    } catch {
      return false;
    }
  }, takeId);
  expect(left).toBe(false);
});
