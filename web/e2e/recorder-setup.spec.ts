import { expect, test } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';

/** Day 27 Check: "Toggle disabled with reason on Safari/Firefox" (US-10). */
test('system-audio toggle reflects what this browser can record', async ({
  page,
  request,
  browserName,
}) => {
  await signUpAndLogIn(page, request);
  await page.getByTestId('home-record-link').click();
  await expect(page).toHaveURL(/\/record$/);

  const toggle = page.getByTestId('recorder-system-audio');
  const hint = page.getByTestId('recorder-system-audio-hint');
  if (browserName === 'chromium') {
    await expect(toggle).toBeEnabled();
    // Full system audio on Windows, tab audio only elsewhere. (Playwright's "Desktop Chrome"
    // profile reports a Windows UA, so go by what the page sees.)
    const ua = await page.evaluate(() => navigator.userAgent);
    await expect(hint).toHaveText(
      /Windows/.test(ua)
        ? 'Records the audio of the screen or tab you share.'
        : /only a browser tab's audio can be captured/,
    );
  } else {
    await expect(toggle).toBeDisabled();
    await expect(hint).toHaveText(
      browserName === 'firefox'
        ? "Firefox can't record system or tab audio."
        : "Safari can't record system or tab audio.",
    );
  }
  // The reason is wired to the control for assistive tech, not just shown nearby.
  await expect(toggle).toHaveAttribute('aria-describedby', 'recorder-system-audio-hint');
  await page.screenshot({ path: test.info().outputPath('recorder-setup.png'), fullPage: true });
});

test('choose a screen and a microphone', async ({ page, request, browserName }) => {
  test.skip(
    browserName !== 'chromium',
    'Fake display/mic capture is wired for Chromium only; Firefox/WebKit arrive with the Day 31 harness',
  );
  await signUpAndLogIn(page, request);
  await page.goto('/record');

  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await expect(page.getByTestId('recorder-screen-info')).toContainText('no system audio');

  const select = page.getByTestId('recorder-mic-select');
  const firstMic = await select.locator('option').nth(1).getAttribute('value');
  await select.selectOption(firstMic ?? '');
  await expect(page.getByTestId('recorder-mic-info')).toContainText('working');

  // The choice is remembered on the next visit.
  await page.reload();
  await expect(page.getByTestId('recorder-mic-select')).toHaveValue(firstMic ?? '');
});
