import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { anonymousPage } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 59 Check: "stop-to-playable ≤ 5 s". A 6 s recording is made on `/record`. The moment its
 * link appears, a stranger opens it. The recording is still processing, so the page says so,
 * plays the original (WebM) as a preview, and, when the MP4 is ready, switches to it by itself.
 * Stop → first frame on the stranger's screen is measured from the click on Stop.
 */
test('a stranger can watch within 5 s of Stop, then the page upgrades to the MP4', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  await startRecording(page, { mic: false });
  await page.waitForTimeout(6_000);

  const stoppedAt = Date.now();
  await page.getByTestId('recorder-stop').click();
  await finishedTake(page);
  const line = page.getByTestId('recorder-share-link');
  await expect(line).toBeVisible({ timeout: 10_000 });
  const url = (await line.locator('a').getAttribute('href')) ?? '';

  const viewer = await anonymousPage(browser);
  await viewer.goto(url);
  // Processing is still running (or just finishing): either the notice or already the player.
  const video = viewer.getByTestId('watch-video');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(viewer.getByTestId('watch-player')).toHaveAttribute('data-first-frame-ms', /^\d+$/, {
    timeout: 30_000,
  });
  const stopToPlayableMs = Date.now() - stoppedAt;
  test.info().annotations.push({ type: 'stop → playable', description: `${stopToPlayableMs} ms` });
  expect(stopToPlayableMs).toBeLessThanOrEqual(5_000);
  const previewing = (await viewer.getByTestId('watch-preview').count()) > 0;
  test.info().annotations.push({ type: 'preview first', description: String(previewing) });

  // Then the full MP4 takes over without a reload.
  await expect(viewer.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });
  await expect(video).toHaveAttribute('src', /mp4\/default\.mp4/);
  await expect(viewer.getByTestId('watch-processing')).toHaveCount(0);
  expect(await video.evaluate((el: HTMLVideoElement) => el.readyState)).toBeGreaterThanOrEqual(1);
  await viewer.context().close();
});
