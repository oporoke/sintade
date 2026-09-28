import { expect, test } from './support/test';

/**
 * Day 21 Check: "Debug page previews chosen screen and mic". Runs the real `SourceManager`
 * on the fake-media harness (e2e/support/test.ts).
 */
test('debug page previews the chosen screen and mic', async ({ page }, testInfo) => {
  await page.goto('/debug');

  await page.getByTestId('source-pick-screen').click();
  const preview = page.getByTestId('source-screen-preview');
  await expect(preview).toBeVisible();
  // The <video> is actually playing frames from the captured display, not just mounted.
  await expect
    .poll(() => preview.evaluate((video: HTMLVideoElement) => video.videoWidth))
    .toBeGreaterThan(0);
  await expect(page.getByTestId('source-screen-info')).toContainText('live');

  await page.getByTestId('source-open-mic').click();
  await expect(page.getByTestId('source-mic-info')).toContainText('live');
  // After permission the mic list fills with real (fake-device) entries.
  await expect(page.getByTestId('source-mic-select').locator('option')).not.toHaveCount(1);

  await page.screenshot({ path: testInfo.outputPath('source-preview.png'), fullPage: true });

  await page.getByTestId('source-stop-all').click();
  await expect(preview).toHaveCount(0);
  await expect(page.getByTestId('source-mic-info')).toHaveCount(0);
  await expect(page.getByTestId('source-error')).toHaveCount(0);
});
