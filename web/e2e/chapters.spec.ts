import { signUpAndLogIn } from './support/auth';
import { anonymousPage, recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 85 Check: "clicking a chapter seeks correctly". The owner writes chapters in the library's
 * dialog; a stranger opens the link, sees them as a table of contents and marks on the scrub
 * bar, and clicking one puts the video at that chapter's time.
 */
test('chapters set in the library show on the watch page and seek when clicked', async ({
  browser,
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
  await expect(card.locator('img')).toBeVisible({ timeout: 60_000 });
  await card.getByTestId('library-chapters').click();
  await page.getByTestId('chapters-text').fill('0:00 Intro\n0:06 Middle\n0:09 Wrap-up');
  await page.getByTestId('chapters-save').click();
  await expect(page.getByTestId('chapters-saved')).toBeVisible();
  // A bad line is explained, not saved.
  await page.getByTestId('chapters-text').fill('0:00 Intro\nsoon Middle');
  await page.getByTestId('chapters-save').click();
  await expect(page.getByTestId('chapters-error')).toContainText('Line 2');
  await page.getByTestId('chapters-close').click();

  const viewer = await anonymousPage(browser);
  await viewer.goto(`/s/${shared.slug}`);
  const video = viewer.getByTestId('watch-video');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(viewer.getByTestId('watch-chapter')).toHaveText([/Intro/, /Middle/, /Wrap-up/]);
  await expect(viewer.getByTestId('watch-chapter-mark')).toHaveCount(2, { timeout: 30_000 });

  await viewer.getByTestId('watch-chapter').nth(1).click();
  await expect
    .poll(() => video.evaluate((el: HTMLVideoElement) => el.currentTime), { timeout: 10_000 })
    .toBeGreaterThan(5.5);
  const at = await video.evaluate((el: HTMLVideoElement) => el.currentTime);
  expect(at).toBeLessThan(7);
  await expect(viewer.getByTestId('watch-chapter').nth(1)).toHaveAttribute('aria-current', 'true');
  await viewer.context().close();
});
