import { expect, test } from '@playwright/test';

const API_BASE_URL = 'http://localhost:8080/api/v1';

/**
 * Day 19: profile settings and "log out everywhere" through the real UI and API. Registration
 * goes straight to the API with a unique spoofed X-Forwarded-For so it doesn't eat into the
 * browser's shared signup rate-limit bucket (see signup-to-login.spec.ts).
 */
test('rename yourself, then log out everywhere', async ({ page, request }) => {
  const email = `day19-e2e-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;
  const password = 'correct-horse-battery-staple-42';

  const registered = await request.post(`${API_BASE_URL}/auth/register`, {
    data: { email, password, display_name: 'Before Rename' },
    headers: { 'x-forwarded-for': uniqueTestIp() },
  });
  expect(registered.status()).toBe(201);

  await page.goto('/login');
  await page.getByTestId('login-email').fill(email);
  await page.getByTestId('login-password').fill(password);
  await page.getByTestId('login-submit').click();
  await expect(page).toHaveURL(/\/home$/);

  await page.getByTestId('home-profile-link').click();
  await expect(page).toHaveURL(/\/settings\/profile$/);
  await expect(page.getByTestId('profile-display-name')).toHaveValue('Before Rename');

  await page.getByTestId('profile-display-name').fill('After Rename');
  await page.getByTestId('profile-save').click();
  await expect(page.getByTestId('profile-saved')).toBeVisible();

  // Survives a reload: the name came back from the server, not local state.
  await page.reload();
  await expect(page.getByTestId('profile-display-name')).toHaveValue('After Rename');

  await page.getByTestId('profile-logout-all').click();
  await expect(page).toHaveURL(/\/login$/);

  await page.goto('/home');
  await expect(page).toHaveURL(/\/login/);
});

function uniqueTestIp(): string {
  const n = () => Math.floor(Math.random() * 254) + 1;
  return `198.51.${n()}.${n()}`;
}
