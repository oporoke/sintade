import { signUpAndLogIn } from './support/auth';
import { recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 93 Check: "timeline matches video length". The owner records 12 s and opens the editor
 * from the library: the timeline is as long as the video the browser plays (within a frame or
 * two), its thumbnails and waveform run edge to edge, and clicking its right edge puts the video
 * at the end, the middle at the middle.
 */
test('the editor timeline spans exactly the video, and clicking it seeks there', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(180_000);
  await signUpAndLogIn(page, request);
  const shared = await recordAndShare(page, 12);

  await page.goto('/library');
  const card = page.locator(`[data-recording-id="${shared.recordingId}"]`);
  await expect(card.getByTestId('library-edit')).toBeVisible({ timeout: 60_000 });
  await card.getByTestId('library-edit').click();
  await expect(page).toHaveURL(new RegExp(`/recordings/${shared.recordingId}/edit$`));

  // The sprite is made after the MP4: reopen until its thumbnails are there.
  const thumbs = page.getByTestId('editor-thumb');
  await expect
    .poll(
      async () => {
        await page.reload();
        await page.getByTestId('editor-timeline').waitFor({ timeout: 30_000 });
        return thumbs.count();
      },
      { timeout: 90_000, intervals: [2_000] },
    )
    .toBeGreaterThan(0);
  await expect(page.getByTestId('editor-waveform')).toHaveAttribute('data-state', 'ready', {
    timeout: 60_000,
  });

  const video = page.getByTestId('editor-video');
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.duration), { timeout: 30_000 })
    .toBeGreaterThan(0);
  const videoMs = await video.evaluate((el: HTMLVideoElement) => el.duration * 1000);
  const timeline = page.getByTestId('editor-timeline');
  const timelineMs = Number(await timeline.getAttribute('data-duration-ms'));
  expect(timelineMs).toBeGreaterThan(10_000);
  expect(Math.abs(timelineMs - videoMs)).toBeLessThanOrEqual(100);
  expect(await page.getByTestId('editor-duration').textContent()).toContain('0:1');

  // Thumbnails cover the timeline from its left edge to its right, and so does the waveform.
  const box = await timeline.boundingBox();
  const first = await thumbs.first().boundingBox();
  const last = await thumbs.last().boundingBox();
  const canvas = await page.getByTestId('editor-waveform').boundingBox();
  if (!box || !first || !last || !canvas) {
    throw new Error('the timeline is not laid out');
  }
  expect(Math.abs(first.x - box.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(last.x + last.width - (box.x + box.width))).toBeLessThanOrEqual(1);
  expect(Math.abs(canvas.width - box.width)).toBeLessThanOrEqual(1);
  expect(await thumbs.first().getAttribute('data-start-ms')).toBe('0');
  expect(Number(await thumbs.last().getAttribute('data-end-ms'))).toBe(timelineMs);

  // The right edge is the end; the middle is the middle.
  await timeline.click({ position: { x: box.width - 1, y: box.height / 2 } });
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000))
    .toBeGreaterThan(videoMs - 250);
  await timeline.click({ position: { x: box.width / 2, y: box.height / 2 } });
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000))
    .toBeLessThan(videoMs / 2 + 250);
  const middle = await video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000);
  expect(Math.abs(middle - videoMs / 2)).toBeLessThanOrEqual(250);
  const playhead = await page.getByTestId('editor-playhead').boundingBox();
  if (!playhead) {
    throw new Error('no playhead');
  }
  expect(Math.abs(playhead.x + playhead.width / 2 - (box.x + box.width / 2))).toBeLessThanOrEqual(
    3,
  );
});
