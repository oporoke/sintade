import { Page } from '@playwright/test';

import { expect, test } from './fixtures';
import { signUpAndLogIn } from './support';

const TARGET = 'https://popup-tab.example/';
const HTML = `<!doctype html><title>Quarterly numbers</title>
<body style="margin:0;background:#00aaff"><p id="n">0</p>
<script>let n=0;(function t(){document.getElementById('n').textContent=String(n++);requestAnimationFrame(t)})();</script>`;

async function tabIdOf(
  context: import('@playwright/test').BrowserContext,
  extensionId: string,
  page: Page,
) {
  const probe = await context.newPage();
  await probe.goto(`chrome-extension://${extensionId}/popup.html`);
  await page.bringToFront();
  const id = await probe.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab?.id ?? null;
  });
  await probe.close();
  expect(id).not.toBeNull();
  return id as number;
}

/**
 * Day 65 Check: "recording from any tab in ≤ 3 clicks". Click 1 is the toolbar button (opening the
 * popup); this test then needs Record (2) and Stop (3): nothing else stands between a signed-in
 * user and a link, no countdown and no dialog. The link is copied on its own.
 */
test('record any tab in three clicks: icon, Record, Stop; the link is copied', async ({
  context,
  extensionId,
}) => {
  test.setTimeout(120_000);
  // The clipboard is read back from a page of the app (extension pages can't be granted).
  await context.grantPermissions(['clipboard-read'], { origin: 'https://localhost:4200' });
  await context.route(`${TARGET}**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: HTML }),
  );
  const app = await context.newPage();
  await signUpAndLogIn(context, app);
  const target = await context.newPage();
  await target.goto(TARGET);
  const tabId = await tabIdOf(context, extensionId, target);

  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`); // click 1
  await expect(popup.getByTestId('popup-status')).toHaveAttribute('data-state', 'signed-in');
  await expect(popup.getByTestId('popup-tab')).toHaveText('Tab: Quarterly numbers');
  // The sources, with their defaults.
  await expect(popup.getByTestId('popup-opt-tabAudio')).toBeChecked();
  await expect(popup.getByTestId('popup-opt-highlights')).toBeChecked();
  await expect(popup.getByTestId('popup-opt-microphone')).not.toBeChecked();
  await expect(popup.getByTestId('popup-opt-keystrokes')).not.toBeChecked();

  // In the browser the popup hangs off the toolbar and the tab being recorded is in front; here
  // the popup is a tab, so put the recorded tab in front for the click and back afterwards.
  await target.bringToFront();
  await popup.getByTestId('popup-record').dispatchEvent('click'); // click 2
  await expect(popup.getByTestId('popup-recording')).toContainText(/Recording 0:0\d/, {
    timeout: 20_000,
  });
  await popup.bringToFront();
  await expect(popup.getByTestId('popup-options')).toBeHidden();
  await target.waitForTimeout(6_000);
  await expect(popup.getByTestId('popup-recording')).toContainText(/Recording 0:0[5-9]/);

  await popup.getByTestId('popup-stop').click(); // click 3
  await expect(popup.getByTestId('popup-result')).toBeVisible({ timeout: 60_000 });
  const url = (await popup.getByTestId('popup-link').getAttribute('href')) ?? '';
  expect(url).toMatch(/\/s\/[0-9A-Za-z]{12}$/);
  await expect(popup.getByTestId('popup-copied')).toBeVisible();
  await app.bringToFront();
  expect(await app.evaluate(() => navigator.clipboard.readText())).toBe(url);
  await popup.bringToFront();

  // Record another goes back to the start.
  await popup.getByTestId('popup-another').click();
  await expect(popup.getByTestId('popup-record')).toBeEnabled();
  await expect(popup.getByTestId('popup-result')).toBeHidden();
});

test('pages the browser does not let an extension record are explained, not attempted', async ({
  context,
  extensionId,
}) => {
  const app = await context.newPage();
  await signUpAndLogIn(context, app);
  const blank = await context.newPage();
  await blank.goto('about:blank');
  const tabId = await tabIdOf(context, extensionId, blank);
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
  // A browser page is not shown to the extension at all: it says so and offers no Record.
  await expect(popup.getByTestId('popup-blocked')).toContainText("can't see this tab");
  await expect(popup.getByTestId('popup-record')).toBeDisabled();
});

test('a signed-out recording attempt says why and offers to sign in', async ({
  context,
  extensionId,
}) => {
  test.setTimeout(60_000);
  await context.route(`${TARGET}**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: HTML }),
  );
  const target = await context.newPage();
  await target.goto(TARGET);
  const tabId = await tabIdOf(context, extensionId, target);
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
  await expect(popup.getByTestId('popup-record')).toBeDisabled(); // not signed in

  // Forced past the button (a stale popup, another window): the server refuses, and it says so.
  await target.bringToFront();
  await popup.evaluate(
    (id) => chrome.runtime.sendMessage({ type: 'start-recording', tabId: id }),
    tabId,
  );
  await expect(popup.getByTestId('popup-recording')).toContainText('Sign in to Sintade to record', {
    timeout: 20_000,
  });
  await expect(popup.getByTestId('popup-error-sign-in')).toBeVisible();
  await expect(popup.getByTestId('popup-error-retry')).toBeHidden();
});

test.describe('the microphone', () => {
  test('a blocked microphone is explained and leaves the setting off', async ({
    context,
    extensionId,
  }) => {
    const app = await context.newPage();
    await signUpAndLogIn(context, app);
    const target = await context.newPage();
    await context.route(`${TARGET}**`, (route) =>
      route.fulfill({ status: 200, contentType: 'text/html', body: HTML }),
    );
    await target.goto(TARGET);
    const tabId = await tabIdOf(context, extensionId, target);
    const popup = await context.newPage();
    await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);

    // Nothing answers the browser's prompt here, so it is refused.
    const [permissionPage] = await Promise.all([
      context.waitForEvent('page'),
      popup.getByTestId('popup-opt-microphone').click(),
    ]);
    await expect(permissionPage.getByTestId('mic-status')).toHaveAttribute('data-state', 'denied', {
      timeout: 15_000,
    });
    await expect(permissionPage.getByTestId('mic-retry')).toBeVisible();
    await expect(popup.getByTestId('popup-opt-microphone')).not.toBeChecked();
    await expect(popup.getByTestId('popup-mic-hint')).toBeVisible();
    await popup.reload();
    await expect(popup.getByTestId('popup-opt-microphone')).not.toBeChecked();
  });

  test('recording with the microphone on but not allowed says so and offers to try again', async ({
    context,
    extensionId,
  }) => {
    test.setTimeout(90_000);
    await context.route(`${TARGET}**`, (route) =>
      route.fulfill({ status: 200, contentType: 'text/html', body: HTML }),
    );
    const app = await context.newPage();
    await signUpAndLogIn(context, app);
    const target = await context.newPage();
    await target.goto(TARGET);
    const tabId = await tabIdOf(context, extensionId, target);
    const popup = await context.newPage();
    await popup.goto(`chrome-extension://${extensionId}/popup.html?tabId=${tabId}`);
    await popup.evaluate(() =>
      chrome.storage.local.set({ settings: { microphone: true, tabAudio: true } }),
    );
    await popup.reload();
    await expect(popup.getByTestId('popup-opt-microphone')).toBeChecked();

    await target.bringToFront();
    await popup.getByTestId('popup-record').dispatchEvent('click');
    await popup.bringToFront();
    await expect(popup.getByTestId('popup-recording')).toContainText("microphone isn't available", {
      timeout: 20_000,
    });
    await expect(popup.getByTestId('popup-error-retry')).toBeVisible();
    await popup.getByTestId('popup-error-retry').click();
    await expect(popup.getByTestId('popup-record')).toBeEnabled();
  });

  test.describe('when the browser allows it', () => {
    test.use({ fakeUi: true });

    test('the permission page records the choice, and the popup keeps it', async ({
      context,
      extensionId,
    }) => {
      const permissionPage = await context.newPage();
      await permissionPage.goto(`chrome-extension://${extensionId}/mic.html`);
      await expect(permissionPage.getByTestId('mic-status')).toHaveAttribute(
        'data-state',
        'granted',
        { timeout: 15_000 },
      );
      const popup = await context.newPage();
      await popup.goto(`chrome-extension://${extensionId}/popup.html`);
      await expect(popup.getByTestId('popup-opt-microphone')).toBeChecked();
      // Turning it off is just a setting. (Turning it on again goes via the permission page
      // unless the browser remembers the grant, which differs between browsers and runners.)
      await popup.getByTestId('popup-opt-microphone').uncheck();
      await popup.reload();
      await expect(popup.getByTestId('popup-opt-microphone')).not.toBeChecked();
    });
  });
});
