import { Page } from '@playwright/test';

/**
 * Makes `POST /api/v1/recordings` fail, so the recorder keeps takes on the device the way it
 * does when the server can't be reached at the start (Day 39). For tests about the on-device
 * copy (OPFS contents, "Save a copy"); uploaded takes are cleared from the device.
 */
export async function keepTakesOnDevice(page: Page): Promise<void> {
  await page.route('**/api/v1/recordings', (route) =>
    route.request().method() === 'POST' ? route.abort('internetdisconnected') : route.continue(),
  );
}
