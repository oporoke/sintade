import { signUpAndLogIn } from './support/auth';
import { anonymousPage, apiAs, openWhenProcessed, recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 54 Check: "first frame ≤ 1.5 s". A 5 s recording is made through the real `/record` page,
 * shared with a link, processed by the worker, and opened by an anonymous viewer. The viewer
 * sees the poster and player; the speed menu, fullscreen button and ← → keys work; and the page
 * reports how long the first frame took (staging has no VPS yet, so this is local storage).
 */
test('an anonymous viewer opens a share link: first frame fast, speed, seek, fullscreen', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  const shared = await recordAndShare(page, 5);

  const viewer = await anonymousPage(browser);
  await openWhenProcessed(viewer, shared.slug);
  await expect(viewer.getByTestId('watch-title')).toHaveText('Untitled recording');
  const player = viewer.getByTestId('watch-player');
  await expect(player).toHaveAttribute('data-first-frame-ms', /^\d+$/, { timeout: 10_000 });
  const firstFrameMs = Number(await player.getAttribute('data-first-frame-ms'));
  test.info().annotations.push({ type: 'first frame', description: `${firstFrameMs} ms` });
  expect(firstFrameMs).toBeLessThanOrEqual(1_500);

  const video = viewer.getByTestId('watch-video');
  const duration = await video.evaluate((el: HTMLVideoElement) => el.duration);
  expect(duration).toBeGreaterThan(3);

  // Speed.
  await viewer.getByTestId('watch-speed').selectOption('1.5');
  expect(await video.evaluate((el: HTMLVideoElement) => el.playbackRate)).toBe(1.5);

  // Keyboard seek: → moves 5 s forward (capped at the end), ← back again.
  await player.focus();
  await video.evaluate((el: HTMLVideoElement) => {
    el.pause();
    el.currentTime = 0;
  });
  await viewer.keyboard.press('ArrowRight');
  const afterRight = await video.evaluate((el: HTMLVideoElement) => el.currentTime);
  expect(afterRight).toBeCloseTo(Math.min(5, duration), 0);
  await viewer.keyboard.press('ArrowLeft');
  expect(await video.evaluate((el: HTMLVideoElement) => el.currentTime)).toBeCloseTo(0, 0);

  // Space plays and pauses.
  await viewer.keyboard.press(' ');
  await expect.poll(() => video.evaluate((el: HTMLVideoElement) => !el.paused)).toBe(true);
  await viewer.keyboard.press(' ');
  await expect.poll(() => video.evaluate((el: HTMLVideoElement) => el.paused)).toBe(true);

  // Fullscreen.
  await viewer.getByTestId('watch-fullscreen').click();
  await expect
    .poll(() => viewer.evaluate(() => document.fullscreenElement !== null), { timeout: 5_000 })
    .toBe(true);

  // Revoking kills the link at once.
  await apiAs(page, 'DELETE', `/recordings/${shared.recordingId}/links/${shared.linkId}`);
  await viewer.reload();
  await expect(viewer.getByTestId('watch-not-found')).toBeVisible();
  await viewer.context().close();
});
