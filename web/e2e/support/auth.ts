import { APIRequestContext, Page, expect } from '@playwright/test';

const API_BASE_URL = 'http://localhost:8080/api/v1';

/**
 * Registers a fresh user straight through the API (with a unique test IP so it doesn't eat the
 * browser's shared signup rate-limit bucket), then logs in through the real UI.
 */
export async function signUpAndLogIn(page: Page, request: APIRequestContext): Promise<string> {
  const email = `e2e-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;
  const password = 'correct-horse-battery-staple-42';
  const registered = await request.post(`${API_BASE_URL}/auth/register`, {
    data: { email, password, display_name: 'E2E Creator' },
    headers: { 'x-forwarded-for': uniqueTestIp() },
  });
  expect(registered.status()).toBe(201);

  await page.goto('/login');
  await page.getByTestId('login-email').fill(email);
  await page.getByTestId('login-password').fill(password);
  await page.getByTestId('login-submit').click();
  await expect(page).toHaveURL(/\/home$/);
  return email;
}

function uniqueTestIp(): string {
  const n = () => Math.floor(Math.random() * 254) + 1;
  return `198.18.${n()}.${n()}`;
}
