import { Page, expect, test } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';

/**
 * Day 29 Check: "Recorder operable by keyboard only". After logging in, the whole take is
 * driven with Tab / Enter / Escape; no pointer. Chromium only: it's the engine with a fake
 * screen to share (Firefox/WebKit: Day 31 harness).
 */

/** Tabs forward until the element with `testId` has focus. */
async function tabTo(page: Page, testId: string): Promise<void> {
  for (let i = 0; i < 40; i += 1) {
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'));
    if (focused === testId) {
      return;
    }
    await page.keyboard.press('Tab');
  }
  throw new Error(`could not reach ${testId} with Tab`);
}

async function focusedTestId(page: Page): Promise<string | null | undefined> {
  return page.evaluate(() => document.activeElement?.getAttribute('data-testid'));
}

test('record, pause, resume and stop with the keyboard alone', async ({
  page,
  request,
  browserName,
}, testInfo) => {
  test.skip(browserName !== 'chromium', 'needs a fake screen to share (Chromium); see Day 31');
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  await page.locator('body').focus();

  await tabTo(page, 'recorder-choose-screen');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();

  await tabTo(page, 'recorder-start');
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('countdown-number')).toHaveText('3');
  await page.keyboard.press('Escape'); // skip the countdown

  // Recording: focus lands on Pause, which has a visible, spoken label.
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await expect.poll(() => focusedTestId(page)).toBe('recorder-pause');
  await expect(page.getByRole('button', { name: 'Pause' })).toBeFocused();
  await expect(page.getByRole('timer', { name: 'Recording time' })).toBeVisible();
  await page.waitForTimeout(2500);

  await page.keyboard.press('Enter'); // Pause
  await expect(page.getByTestId('recorder-status')).toHaveText('Paused');
  await expect(page.getByRole('button', { name: 'Resume' })).toBeFocused();
  await page.waitForTimeout(400); // one 250 ms timer tick after the pause
  const pausedAt = await page.getByTestId('recorder-timer').getAttribute('data-elapsed-ms');
  await page.waitForTimeout(1500);
  // The timer is frozen while paused.
  expect(await page.getByTestId('recorder-timer').getAttribute('data-elapsed-ms')).toBe(pausedAt);

  await page.keyboard.press('Enter'); // Resume
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(2000);

  await tabTo(page, 'recorder-stop');
  await page.keyboard.press('Enter');

  // Done: focus moves to the result, whose length excludes the 1.5 s pause.
  await expect(page.getByTestId('recorder-done')).toBeVisible({ timeout: 10_000 });
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.id))
    .toBe('recorder-done-heading');
  const durationMs = Number(
    await page.getByTestId('recorder-done-summary').getAttribute('data-duration-ms'),
  );
  expect(durationMs).toBeGreaterThan(3500);
  expect(durationMs).toBeLessThan(6000);

  await tabTo(page, 'recorder-download');
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.keyboard.press('Enter'),
  ]);
  const file = testInfo.outputPath('keyboard-take.webm');
  await download.saveAs(file);
  await page.screenshot({ path: testInfo.outputPath('recorder-done.png'), fullPage: true });
});
