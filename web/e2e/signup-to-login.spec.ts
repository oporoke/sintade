import { expect, test } from '@playwright/test';

const API_BASE_URL = 'http://localhost:8080/api/v1';
const MAILPIT_API = 'http://localhost:8025/api/v1';

/**
 * The literal Day 17 Check: "Full signup-to-login through the UI". Exercises the real backend
 * (API + a running `worker` that actually delivers the verification email via Mailpit) end to
 * end through the browser -- no mocking of the API.
 */
test('sign up, verify email, then log in through the UI', async ({ page, request }) => {
  const email = `day17-e2e-${Date.now()}@example.com`;
  const password = 'correct-horse-battery-staple-42';
  const displayName = 'Day Seventeen E2E';

  // No X-Forwarded-For spoofing here: this signup genuinely goes through the browser, and the
  // backend's CORS policy doesn't allow-list that header (a real browser client could never
  // set it either -- only a reverse proxy adds it, invisible to CORS). One real signup per
  // browser project (chromium/firefox/webkit) stays within Day 16's 3/hour/IP limit.
  await page.goto('/signup');
  await page.getByTestId('signup-email').fill(email);
  await page.getByTestId('signup-display-name').fill(displayName);
  await page.getByTestId('signup-password').fill(password);
  await page.getByTestId('signup-submit').click();
  await expect(page.getByTestId('signup-success')).toBeVisible();

  const token = await waitForVerificationToken(request, email);

  await page.goto(`/verify-email?token=${token}`);
  await expect(page.getByTestId('verify-email-success')).toBeVisible();

  await page.goto('/login');
  await page.getByTestId('login-email').fill(email);
  await page.getByTestId('login-password').fill(password);
  await page.getByTestId('login-submit').click();

  await expect(page).toHaveURL(/\/home$/);
  await expect(page.getByTestId('home-user-email')).toHaveText(email);
});

test('a wrong password shows an inline error and does not navigate', async ({ page, request }) => {
  const email = `day17-e2e-badpw-${Date.now()}@example.com`;
  const password = 'correct-horse-battery-staple-42';

  await request.post(`${API_BASE_URL}/auth/register`, {
    data: { email, password, display_name: 'Bad Password Tester' },
    headers: { 'x-forwarded-for': uniqueTestIp() },
  });

  await page.goto('/login');
  await page.getByTestId('login-email').fill(email);
  await page.getByTestId('login-password').fill('totally-the-wrong-password');
  await page.getByTestId('login-submit').click();

  await expect(page.getByTestId('login-error')).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
});

function uniqueTestIp(): string {
  const n = () => Math.floor(Math.random() * 254) + 1;
  return `203.0.${n()}.${n()}`;
}

async function waitForVerificationToken(
  request: import('@playwright/test').APIRequestContext,
  toEmail: string,
): Promise<string> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    const response = await request.get(
      `${MAILPIT_API}/search?query=${encodeURIComponent(`to:${toEmail}`)}`,
    );
    const body = (await response.json()) as { messages: { ID: string }[] };
    if (body.messages.length > 0) {
      const messageResponse = await request.get(
        `${MAILPIT_API}/message/${body.messages[0].ID}`,
      );
      const message = (await messageResponse.json()) as { Text: string };
      const match = /token=([0-9a-f]+)/.exec(message.Text);
      if (match) {
        return match[1];
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`no verification email arrived for ${toEmail} within the timeout`);
}
