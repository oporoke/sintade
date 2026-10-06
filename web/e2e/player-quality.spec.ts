import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { anonymousPage } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 83 Check: "quality switch mid-play without stall". A 6 s recording is made, its MP4 is
 * ready, and the first view asks for the HLS ladder. Once the ladder exists a reload plays it
 * through hls.js (or natively where the browser can); the viewer picks 360p while the video
 * plays and the picture keeps moving: no `waiting` event, time keeps advancing, and the rung
 * on screen changes.
 */
test('the viewer can switch quality mid-play without a stall', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(180_000);
  await signUpAndLogIn(page, request);
  await startRecording(page, { mic: false });
  await page.waitForTimeout(8_000);
  await page.getByTestId('recorder-stop').click();
  await finishedTake(page);
  const line = page.getByTestId('recorder-share-link');
  await expect(line).toBeVisible({ timeout: 10_000 });
  const url = (await line.locator('a').getAttribute('href')) ?? '';

  const viewer = await anonymousPage(browser);
  await viewer.goto(url);
  await expect(viewer.getByTestId('watch-video')).toBeVisible({ timeout: 30_000 });
  await expect(viewer.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });

  // The MP4's first view has queued the ladder: reload until the page offers rungs.
  const quality = viewer.getByTestId('watch-quality');
  await expect(async () => {
    await viewer.reload();
    await expect(viewer.getByTestId('watch-video')).toBeVisible();
    await expect(quality).toBeVisible({ timeout: 3_000 });
  }).toPass({ timeout: 90_000, intervals: [2_000] });

  const options = await quality.locator('option').allTextContents();
  expect(options.map((text) => text.replace(/\s+/g, ' ').trim())).toEqual(
    expect.arrayContaining(['360p']),
  );
  expect(options.length).toBeGreaterThanOrEqual(3);

  const video = viewer.getByTestId('watch-video');
  await video.evaluate((el: HTMLVideoElement) => {
    el.muted = true;
    (window as unknown as { stalls: number }).stalls = 0;
    el.addEventListener('waiting', () => {
      (window as unknown as { stalls: number }).stalls += 1;
    });
    return el.play();
  });
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime), { timeout: 15_000 })
    .toBeGreaterThan(0.5);

  const before = await video.evaluate((el: HTMLVideoElement) => el.currentTime);
  const stallsBefore = await viewer.evaluate(
    () => (window as unknown as { stalls: number }).stalls,
  );
  await quality.selectOption({ label: '360p' });
  await expect(quality).toHaveAttribute('data-current-height', '360', { timeout: 15_000 });
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime), { timeout: 15_000 })
    .toBeGreaterThan(before + 1);
  const stalls = await viewer.evaluate(() => (window as unknown as { stalls: number }).stalls);
  expect(stalls - stallsBefore, 'waiting events during the switch').toBe(0);
  expect(await video.evaluate((el: HTMLVideoElement) => el.paused)).toBe(false);
  await viewer.context().close();
});
