import { Browser, Page } from '@playwright/test';

import { finishedTake, startRecording } from './loss';

export interface SharedRecording {
  recordingId: string;
  slug: string;
  linkId: string;
}

/** The signed-in page's `fetch` with the CSRF header the API wants on state changes. */
export async function apiAs(
  page: Page,
  method: string,
  path: string,
  body?: unknown,
): Promise<{ status: number; body: unknown }> {
  return page.evaluate(
    async ({ method, path, body }) => {
      const csrf = document.cookie.match(/(?:^|; )sintade_csrf=([^;]*)/)?.[1] ?? '';
      const response = await fetch(`/api/v1${path}`, {
        method,
        credentials: 'include',
        headers: { 'content-type': 'application/json', 'x-csrf-token': decodeURIComponent(csrf) },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await response.text();
      return { status: response.status, body: text ? JSON.parse(text) : null };
    },
    { method, path, body },
  );
}

/** Records `seconds` on `/record`, stops, and creates a share link with `visibility`. */
export async function recordAndShare(
  page: Page,
  seconds: number,
  visibility: 'private' | 'workspace' | 'link' | 'public' = 'link',
): Promise<SharedRecording> {
  await startRecording(page, { mic: false });
  await page.waitForTimeout(seconds * 1000);
  await page.getByTestId('recorder-stop').click();
  await finishedTake(page);
  const recordingId = await page
    .getByTestId('recorder-done-summary')
    .getAttribute('data-recording-id');
  if (!recordingId) {
    throw new Error('the recorder reported no recording id');
  }
  const created = await apiAs(page, 'POST', `/recordings/${recordingId}/links`, { visibility });
  const link = created.body as { slug: string; id: string };
  return { recordingId, slug: link.slug, linkId: link.id };
}

/** A fresh anonymous browser context (no cookies) for the viewer's side. */
export async function anonymousPage(browser: Browser): Promise<Page> {
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  return context.newPage();
}

/**
 * Opens a share link and waits until the full MP4 is what is on screen: the page first plays
 * the original as a preview, then switches itself over once processing has finished.
 */
export async function openWhenProcessed(page: Page, slug: string): Promise<void> {
  await page.goto(`/s/${slug}`);
  await page.getByTestId('watch-video').waitFor({ state: 'visible', timeout: 60_000 });
  await page.getByTestId('watch-preview').waitFor({ state: 'detached', timeout: 60_000 });
}
