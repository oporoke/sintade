import { signUpAndLogIn } from './support/auth';
import { expect, test } from './support/test';

/**
 * Day 37 Check: "Uploaded chunk hashes match server records". `/debug`'s upload self-test runs
 * the real `Uploader` (SubtleCrypto SHA-256, presign, PUT straight to MinIO, ack) against the
 * real API; the test then reads the server's records itself and compares.
 */
test('uploaded chunk hashes match server records', async ({ page, request }) => {
  await signUpAndLogIn(page, request);
  await page.goto('/debug');
  await page.getByTestId('upload-selftest-run').click();

  const summary = page.getByTestId('upload-selftest-summary');
  const error = page.getByTestId('upload-selftest-error');
  await expect(summary.or(error)).toBeVisible({ timeout: 30_000 });
  await expect(error).toHaveCount(0);
  await expect(summary).toContainText('3 chunks uploaded');
  await expect(summary).toContainText('every hash matches the server record');

  // Independently of the page: ask the API what it recorded, with the page's session.
  const takeId = await summary.getAttribute('data-take-id');
  const status = await page.request.get(`/api/v1/takes/${takeId}/status`);
  expect(status.ok()).toBe(true);
  const body = (await status.json()) as {
    received: number[];
    finalized: boolean;
    chunks: { idx: number; size_bytes: number; sha256: string }[];
  };
  expect(body.received).toEqual([0, 1, 2]);
  expect(body.finalized).toBe(false);

  const rows = page.getByTestId('upload-selftest-chunk');
  await expect(rows).toHaveCount(3);
  for (const chunk of body.chunks) {
    const row = rows.nth(chunk.idx);
    expect(await row.getAttribute('data-local-sha256')).toBe(chunk.sha256);
    expect(chunk.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(chunk.size_bytes).toBe(32_768 + chunk.idx * 777);
  }
});
