import { Page, expect, test } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';

/**
 * Day 30 Check: "Denying permission never shows a blank screen". Browsers report a denied (or
 * dismissed) permission as a NotAllowedError from getDisplayMedia/getUserMedia; this makes both
 * reject exactly that way on every engine, with one not-yet-permitted microphone listed (as
 * browsers do before permission), and checks the page explains what to do.
 */
async function denyCapture(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const deny = () => Promise.reject(new DOMException('Permission denied', 'NotAllowedError'));
    const devices = navigator.mediaDevices;
    Object.defineProperty(devices, 'getDisplayMedia', { value: deny, configurable: true });
    Object.defineProperty(devices, 'getUserMedia', { value: deny, configurable: true });
    Object.defineProperty(devices, 'enumerateDevices', {
      value: () =>
        Promise.resolve([
          { kind: 'audioinput', deviceId: '', groupId: '', label: '', toJSON: () => ({}) },
        ]),
      configurable: true,
    });
  });
}

async function expectHelpNotBlank(page: Page, title: string): Promise<void> {
  const problem = page.getByTestId('recorder-error');
  await expect(problem).toBeVisible();
  await expect(page.getByTestId('recorder-problem-title')).toHaveText(title);
  await expect(page.getByTestId('recorder-help-steps').locator('li').first()).toBeVisible();
  await expect(page.getByTestId('recorder-retry')).toBeVisible();
  // The rest of the recorder is still there too: nothing went blank.
  await expect(page.getByRole('heading', { name: 'New recording' })).toBeVisible();
  await expect(page.getByTestId('recorder-choose-screen')).toBeEnabled();
}

test('a denied screen share explains what to do', async ({ page, request }) => {
  await denyCapture(page);
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expectHelpNotBlank(page, 'Screen sharing was cancelled or blocked');
  await page.screenshot({ path: test.info().outputPath('screen-denied.png'), fullPage: true });
});

test('a denied microphone explains how to unblock it in this browser', async ({
  page,
  request,
  browserName,
}) => {
  await denyCapture(page);
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  await page.getByTestId('recorder-mic-select').selectOption({ index: 1 });
  await expectHelpNotBlank(page, 'Microphone access is blocked');

  const firstStep = page.getByTestId('recorder-help-steps').locator('li').first();
  const expected = {
    chromium: /site settings icon/,
    firefox: /crossed-out microphone icon/,
    webkit: /Settings for This Website/,
  }[browserName];
  await expect(firstStep).toHaveText(expected ?? /./);
  await page.screenshot({ path: test.info().outputPath('mic-denied.png'), fullPage: true });
});

test.describe('on a phone', () => {
  test.use({
    userAgent:
      'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  });

  test('the recorder is replaced by a view-only notice', async ({ page, request }) => {
    await signUpAndLogIn(page, request);
    await page.goto('/record');
    await expect(page.getByTestId('recorder-mobile-notice')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Recording needs a computer' })).toBeVisible();
    await expect(page.getByTestId('recorder-setup')).toHaveCount(0);
  });
});
