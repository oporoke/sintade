import { expect, test } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';

/** Day 28 Check: "Countdown and meter work in all engines". */

test('countdown shows 3, 2, 1 and then finishes', async ({ page }) => {
  await page.goto('/debug');
  await page.getByTestId('debug-countdown-run').click();
  const number = page.getByTestId('countdown-number');
  for (const n of ['3', '2', '1']) {
    await expect(number).toHaveText(n);
  }
  await expect(page.getByTestId('debug-countdown-outcome')).toHaveText('finished');
  await expect(number).toHaveCount(0);
});

test('Esc skips the countdown', async ({ page }) => {
  await page.goto('/debug');
  await page.getByTestId('debug-countdown-run').click();
  await expect(page.getByTestId('countdown-number')).toHaveText('3');
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('debug-countdown-outcome')).toHaveText('skipped');
  await expect(page.getByTestId('countdown-number')).toHaveCount(0);
});

test('the device-check meter moves with the microphone', async ({
  page,
  request,
  context,
  browserName,
}) => {
  if (browserName === 'webkit') {
    await context.grantPermissions(['microphone']);
  }
  await signUpAndLogIn(page, request);
  await page.goto('/record');

  const select = page.getByTestId('recorder-mic-select');
  await expect(select.locator('option')).not.toHaveCount(1); // at least one (fake) microphone
  await select.selectOption({ index: 1 });
  await expect(page.getByTestId('recorder-mic-info')).toBeVisible();
  await expect(page.getByTestId('recorder-error')).toHaveCount(0);

  // Watch every frame: the fake mics emit periodic tones, and the meter's peak-hold keeps each
  // one visible for a while, but polling coarsely could still land between them.
  await page.waitForFunction(
    () => Number(document.querySelector('[data-testid="recorder-mic-level"]')?.textContent) > 0.01,
    undefined,
    { polling: 'raf', timeout: 10_000 },
  );
  await page.screenshot({ path: test.info().outputPath('device-check.png'), fullPage: true });
});
