import { signUpAndLogIn } from './support/auth';
import { apiAs, recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 57: the library. A new account sees the empty state; after one real recording the card
 * gets a thumbnail once processing finishes; with 30 recordings the first page holds 24 and
 * "Load more" brings the other 6, newest first.
 */
test('the library: empty state, a real recording with a thumbnail, and pagination', async ({
  page,
  request,
  browserName,
}) => {
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  await page.goto('/library');
  await expect(page.getByTestId('library-empty')).toBeVisible();
  await expect(page.getByTestId('library-item')).toHaveCount(0);

  if (browserName !== 'webkit') {
    // A real recording: it lists as uploaded/processing, then shows its poster.
    const shared = await recordAndShare(page, 5);
    await page.goto('/library');
    const card = page.locator(`[data-recording-id="${shared.recordingId}"]`);
    await expect(card).toBeVisible();
    await expect(async () => {
      await page.reload();
      await expect(card.locator('img')).toBeVisible({ timeout: 2_000 });
    }).toPass({ timeout: 60_000, intervals: [1_000] });
    await expect(card.getByTestId('library-title')).toHaveText('Untitled recording');
    await expect(card.getByTestId('library-duration')).toContainText('s');
    await expect(card.getByTestId('library-download')).toBeVisible();
    // The thumbnail really loads (a decoded image, not a broken link).
    const loaded = await card
      .locator('img')
      .evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0);
    expect(loaded).toBe(true);
    // Share opens the dialog for this recording, with its link.
    await card.getByTestId('library-share').click();
    await expect(page.getByTestId('share-url')).toHaveValue(new RegExp(`/s/${shared.slug}$`));
    await page.getByTestId('share-close').click();
  }

  // Pagination: top up to 30 recordings.
  const have = await page.getByTestId('library-item').count();
  for (let i = have; i < 30; i += 1) {
    const created = await apiAs(page, 'POST', '/recordings', {
      title: `Seeded ${i}`,
      mime_type: 'video/webm;codecs=vp9',
      has_system_audio: false,
      has_mic: false,
      has_camera: false,
    });
    expect(created.status).toBe(201);
  }
  await page.goto('/library');
  await expect(page.getByTestId('library-item')).toHaveCount(24);
  await page.getByTestId('library-more').click();
  await expect(page.getByTestId('library-item')).toHaveCount(30);
  await expect(page.getByTestId('library-more')).toHaveCount(0);
  const ids = await page
    .getByTestId('library-item')
    .evaluateAll((cards) => cards.map((c) => c.getAttribute('data-recording-id')));
  expect(new Set(ids).size).toBe(30);
});
