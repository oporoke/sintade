import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { anonymousPage } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 84 Check: "reopening resumes at last position". Also: the scrub bar shows the sprite's
 * thumbnail and time on hover, and the number keys jump through the video.
 */
test('a viewer who reopens the link is back where they stopped, and hovering previews', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(180_000);
  await signUpAndLogIn(page, request);
  await startRecording(page, { mic: false });
  await page.waitForTimeout(10_000);
  await page.getByTestId('recorder-stop').click();
  await finishedTake(page);
  const line = page.getByTestId('recorder-share-link');
  await expect(line).toBeVisible({ timeout: 10_000 });
  const url = (await line.locator('a').getAttribute('href')) ?? '';

  const viewer = await anonymousPage(browser);
  await viewer.goto(url);
  const video = viewer.getByTestId('watch-video');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(viewer.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });

  // The sprite is made after the MP4: reload until a hover shows a thumbnail.
  const scrub = viewer.getByTestId('watch-scrub');
  await expect(async () => {
    await viewer.reload();
    await expect(video).toBeVisible();
    await expect
      .poll(() => video.evaluate((el: HTMLVideoElement) => el.readyState), { timeout: 10_000 })
      .toBeGreaterThanOrEqual(1);
    const box = await scrub.boundingBox();
    if (!box) {
      throw new Error('no scrub bar');
    }
    await viewer.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await expect(viewer.getByTestId('watch-scrub-tile')).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 120_000, intervals: [2_000] });
  const half = await viewer.getByTestId('watch-scrub-time').textContent();
  expect(half).toMatch(/^(\d+ s|\d+:\d{2})$/);
  // The tile really loaded its sheet.
  const image = await viewer
    .getByTestId('watch-scrub-tile')
    .evaluate((el) => getComputedStyle(el).backgroundImage);
  expect(image).toContain('X-Amz-Signature');

  // Jump to 50 % with the key, let it play a moment, then leave and come back.
  await viewer.getByTestId('watch-player').focus();
  await viewer.keyboard.press('5');
  await video.evaluate((el: HTMLVideoElement) => {
    el.muted = true;
    return el.play();
  });
  await viewer.waitForTimeout(1_500);
  await video.evaluate((el: HTMLVideoElement) => el.pause());
  const left = await video.evaluate((el: HTMLVideoElement) => el.currentTime);
  expect(left).toBeGreaterThan(3);

  await viewer.reload();
  await expect(video).toBeVisible();
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime), { timeout: 10_000 })
    .toBeGreaterThan(left - 1);
  const back = await video.evaluate((el: HTMLVideoElement) => el.currentTime);
  expect(Math.abs(back - left)).toBeLessThan(1.5);
  await viewer.context().close();
});
