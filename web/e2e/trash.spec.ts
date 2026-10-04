import { signUpAndLogIn } from './support/auth';
import { anonymousPage, openWhenProcessed, recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 58 Check: "trashed link dies at once". A recording is made, shared and renamed in place in
 * the library; a stranger watches it by its link; the owner moves it to the trash and the
 * stranger's next request finds nothing, while the library no longer lists it.
 */
test('rename in place, then trash: the link dies at once', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  const shared = await recordAndShare(page, 5);

  await page.goto('/library');
  const card = page.locator(`[data-recording-id="${shared.recordingId}"]`);
  await card.getByTestId('library-rename').click();
  const input = card.getByTestId('library-title-input');
  await expect(input).toBeFocused();
  await input.fill('Weekly sync');
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    input.press('Enter'),
  ]);
  await expect(card.getByTestId('library-title')).toHaveText('Weekly sync');
  await page.reload();
  await expect(card.getByTestId('library-title')).toHaveText('Weekly sync');

  // A stranger watches it (once processing is done) under its new name.
  const stranger = await anonymousPage(browser);
  await openWhenProcessed(stranger, shared.slug);
  await expect(stranger.getByTestId('watch-title')).toHaveText('Weekly sync');

  // Trash it: ask first, then it is gone from the library…
  await card.getByTestId('library-trash').click();
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'DELETE' && r.status() === 204),
    card.getByTestId('library-trash-yes').click(),
  ]);
  await expect(card).toHaveCount(0);
  await expect(page.getByTestId('library-notice')).toContainText('30 days');

  // …and the stranger's very next request finds nothing.
  await stranger.reload();
  await expect(stranger.getByTestId('watch-not-found')).toBeVisible();
  await page.reload();
  await expect(page.getByTestId('library-item')).toHaveCount(0);
  await stranger.context().close();
});
