import { expect, test } from './fixtures';
import { APP, signUpAndLogIn } from './support';

/**
 * Day 62 Check: "extension calls /me as the signed-in user". The browser's session is the
 * extension's session (host permission for the app origin; ADR-0018): sign in on the web app and
 * the popup says who you are; sign out and it asks you to sign in; its Sign in button opens the
 * app's login page.
 */
test('the popup is signed in as whoever is signed in to the web app', async ({
  context,
  extensionId,
}) => {
  const popupUrl = `chrome-extension://${extensionId}/popup.html`;

  // Nobody is signed in: it says so, and offers to sign in.
  const signedOut = await context.newPage();
  await signedOut.goto(popupUrl);
  await expect(signedOut.getByTestId('popup-status')).toHaveText('Sign in to Sintade to record');
  await expect(signedOut.getByTestId('popup-record')).toBeDisabled();
  const [login] = await Promise.all([
    context.waitForEvent('page'),
    signedOut.getByTestId('popup-sign-in').click(),
  ]);
  await login.waitForURL(`${APP}/login`);

  // Sign in on the web app: the popup now knows the user, with no extra step.
  const email = await signUpAndLogIn(context, login);
  const popup = await context.newPage();
  await popup.goto(popupUrl);
  await expect(popup.getByTestId('popup-status')).toHaveText(`Signed in as ${email}`);
  await expect(popup.getByTestId('popup-sign-in')).toBeHidden();

  // Sign out of the web app: the popup follows.
  await login.getByTestId('home-logout').click();
  await login.waitForURL(`${APP}/login`);
  await popup.reload();
  await expect(popup.getByTestId('popup-status')).toHaveText('Sign in to Sintade to record');
});

test('the extension’s requests are accepted without a CSRF cookie only with its header', async ({
  context,
  extensionId,
}) => {
  const page = await context.newPage();
  await signUpAndLogIn(context, page);
  const popup = await context.newPage();
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  // From an extension page with host permission: the session cookie is sent; a state-changing
  // call (PATCH /me) passes the CSRF check on the extension header alone.
  const withHeader = await popup.evaluate(async (app) => {
    const response = await fetch(`${app}/api/v1/me`, {
      method: 'PATCH',
      credentials: 'include',
      headers: { 'content-type': 'application/json', 'X-Sintade-Client': 'extension/0.1.0' },
      body: JSON.stringify({ display_name: 'Via extension' }),
    });
    return response.status;
  }, APP);
  expect(withHeader).toBe(200);
  const without = await popup.evaluate(async (app) => {
    const response = await fetch(`${app}/api/v1/me`, {
      method: 'PATCH',
      credentials: 'include',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ display_name: 'Nope' }),
    });
    return response.status;
  }, APP);
  expect(without).toBe(403);
});
