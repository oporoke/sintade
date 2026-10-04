import { signUpAndLogIn } from './support/auth';
import { anonymousPage } from './support/share';
import { finishedTake, startRecording } from './support/loss';
import { expect, test } from './support/test';

/**
 * Day 55 Check: "link copied and working after stop". Records 5 s on `/record`; when the upload
 * finishes the page creates a link and copies it. A stranger's browser opens that link (the
 * watch page, not "doesn't work"); the Share dialog then makes it private (the stranger gets
 * the 404 page) and finally turns it off.
 */
test('the link is copied after stop and works; the Share dialog changes who can watch', async ({
  browser,
  context,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(90_000);
  if (browserName === 'chromium') {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  }
  await signUpAndLogIn(page, request);
  await startRecording(page, { mic: false });
  await page.waitForTimeout(5_000);
  await page.getByTestId('recorder-stop').click();
  await finishedTake(page);

  const line = page.getByTestId('recorder-share-link');
  await expect(line).toBeVisible({ timeout: 10_000 });
  const url = (await line.locator('a').getAttribute('href')) ?? '';
  expect(url).toMatch(/\/s\/[0-9A-Za-z]{12}$/);
  if (browserName === 'chromium') {
    await expect(line).toHaveAttribute('data-copied', 'true');
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);
  }

  // A stranger opens it.
  const stranger = await anonymousPage(browser);
  await stranger.goto(url);
  await expect(stranger.getByTestId('watch-title')).toHaveText('Untitled recording');
  await expect(stranger.getByTestId('watch-not-found')).toHaveCount(0);

  // Share dialog: private → the stranger can't see it.
  await page.getByTestId('recorder-share').click();
  const dialog = page.getByTestId('share-dialog');
  await expect(dialog.getByTestId('share-url')).toHaveValue(url);
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    dialog.getByTestId('share-visibility').selectOption('private'),
  ]);
  await stranger.reload();
  await expect(stranger.getByTestId('watch-not-found')).toBeVisible();

  // Back to anyone-with-the-link, then off.
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    dialog.getByTestId('share-visibility').selectOption('link'),
  ]);
  await stranger.reload();
  await expect(stranger.getByTestId('watch-title')).toBeVisible();
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'DELETE' && r.ok()),
    dialog.getByTestId('share-revoke').click(),
  ]);
  await expect(dialog.getByTestId('share-no-link')).toBeVisible();
  await stranger.reload();
  await expect(stranger.getByTestId('watch-not-found')).toBeVisible();
  await stranger.context().close();
});
