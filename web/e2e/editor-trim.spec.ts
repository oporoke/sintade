import { Locator, Page } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';
import { recordAndShare } from './support/share';
import { expect, test } from './support/test';

/** Drags a trim handle to `fraction` of the timeline's width. */
async function drag(page: Page, handle: Locator, timeline: Locator, fraction: number) {
  const from = await handle.boundingBox();
  const box = await timeline.boundingBox();
  if (!from || !box) {
    throw new Error('the timeline is not laid out');
  }
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * fraction, from.y + from.height / 2, { steps: 8 });
  await page.mouse.up();
}

/**
 * Day 94 Check: "preview starts/ends at handles". The owner drags the start handle to about 3 s
 * and the end handle to about 9 s of a 12 s recording; the frame under each handle shows as it
 * moves. Preview then plays from the start handle and stops at the end handle, and the native
 * play button starts over from the start handle.
 */
test('the trim preview plays from the start handle and stops at the end handle', async ({
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
  const video = page.getByTestId('editor-video');
  const timeline = page.getByTestId('editor-timeline');
  await expect(timeline).toBeVisible({ timeout: 30_000 });
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.readyState), { timeout: 30_000 })
    .toBeGreaterThanOrEqual(1);

  const start = page.getByTestId('editor-trim-start');
  const end = page.getByTestId('editor-trim-end');
  await drag(page, start, timeline, 0.25);
  const startMs = Number(await start.getAttribute('aria-valuenow'));
  // The frame under the handle is what the video shows.
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000))
    .toBeCloseTo(startMs, -2);
  await drag(page, end, timeline, 0.75);
  const endMs = Number(await end.getAttribute('aria-valuenow'));
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000))
    .toBeCloseTo(endMs, -2);
  const duration = Number(await timeline.getAttribute('data-duration-ms'));
  expect(startMs).toBeGreaterThan(duration * 0.2);
  expect(startMs).toBeLessThan(duration * 0.3);
  expect(endMs).toBeGreaterThan(duration * 0.7);
  expect(endMs).toBeLessThan(duration * 0.8);
  await expect(page.getByTestId('editor-kept')).toHaveAttribute(
    'data-kept-ms',
    String(endMs - startMs),
  );

  // Every time the video starts playing, and where it is when it pauses.
  await video.evaluate((el: HTMLVideoElement) => {
    const log: [string, number][] = [];
    (window as unknown as { previewLog: [string, number][] }).previewLog = log;
    el.addEventListener('playing', () => log.push(['playing', el.currentTime * 1000]));
    el.addEventListener('pause', () => log.push(['pause', el.currentTime * 1000]));
  });
  const log = () =>
    page.evaluate(() => (window as unknown as { previewLog: [string, number][] }).previewLog);
  const afterStop = async () => {
    await expect
      .poll(async () => (await log()).some(([event]) => event === 'pause'), { timeout: 20_000 })
      .toBe(true);
    await expect
      .poll(() => video.evaluate((el: HTMLVideoElement) => el.paused && !el.seeking))
      .toBe(true);
  };

  await page.getByTestId('editor-preview').click();
  await afterStop();
  let events = await log();
  const playing = events.find(([event]) => event === 'playing');
  const paused = events.find(([event]) => event === 'pause');
  expect(playing, JSON.stringify(events)).toBeDefined();
  expect(Math.abs((playing?.[1] ?? -1) - startMs), JSON.stringify(events)).toBeLessThanOrEqual(100);
  // It played the kept stretch: it paused at the end handle, not earlier, and only just past it.
  expect(paused?.[1] ?? 0).toBeGreaterThanOrEqual(endMs - 50);
  expect(paused?.[1] ?? 0).toBeLessThanOrEqual(endMs + 150);
  const stoppedAt = await video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000);
  expect(Math.abs(stoppedAt - endMs)).toBeLessThanOrEqual(50);

  // The native play button, pressed at the end handle, starts over at the start handle.
  await page.evaluate(() => {
    (window as unknown as { previewLog: [string, number][] }).previewLog.length = 0;
  });
  await video.evaluate((el: HTMLVideoElement) => el.play());
  await afterStop();
  events = await log();
  const replay = events.find(([event]) => event === 'playing');
  expect(Math.abs((replay?.[1] ?? -1) - startMs), JSON.stringify(events)).toBeLessThanOrEqual(100);
  const restoppedAt = await video.evaluate((el: HTMLVideoElement) => el.currentTime * 1000);
  expect(Math.abs(restoppedAt - endMs)).toBeLessThanOrEqual(50);
});
