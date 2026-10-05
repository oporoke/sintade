import { BrowserContext, Page, expect } from '@playwright/test';

export const APP = 'https://localhost:4200';

/** Registers a fresh user through the API, then signs in through the real login page. */
export async function signUpAndLogIn(context: BrowserContext, page: Page): Promise<string> {
  const email = `ext-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;
  const password = 'correct-horse-battery-staple-42';
  const n = () => Math.floor(Math.random() * 254) + 1;
  const registered = await context.request.post(`${APP}/api/v1/auth/register`, {
    data: { email, password, display_name: 'Extension User' },
    headers: { 'x-forwarded-for': `198.18.${n()}.${n()}` },
    ignoreHTTPSErrors: true,
  });
  expect(registered.status()).toBe(201);
  await page.goto(`${APP}/login`);
  await page.getByTestId('login-email').fill(email);
  await page.getByTestId('login-password').fill(password);
  await page.getByTestId('login-submit').click();
  await expect(page).toHaveURL(/\/home$/);
  return email;
}
