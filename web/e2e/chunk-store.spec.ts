import { expect, test } from '@playwright/test';

/**
 * Day 25 Check: "Chunks survive a page reload". For each backend, write a 5-chunk test take,
 * reload the page, and verify every chunk's bytes from the fresh page.
 */
for (const backend of ['opfs', 'indexeddb'] as const) {
  test(`${backend} chunks survive a page reload`, async ({ page }) => {
    await page.goto('/debug');
    const status = page.getByTestId(`store-status-${backend}`);

    await page.getByTestId(`store-clear-${backend}`).click();
    await expect(status).toHaveText(/cleared|unavailable in this browser/);
    if ((await status.textContent())?.includes('unavailable')) {
      test.info().annotations.push({ type: 'backend', description: `${backend} unavailable` });
      // OPFS may be absent or read-only on the main thread; IndexedDB never may.
      expect(backend).toBe('opfs');
      return;
    }

    await page.getByTestId(`store-write-${backend}`).click();
    await expect(status).toHaveText(/^wrote selftest-\d+$/);
    const takeId = ((await status.textContent()) ?? '').replace('wrote ', '');

    await page.reload();

    await page.getByTestId(`store-inspect-${backend}`).click();
    await expect(status).toHaveText('1 test take(s) stored');
    await expect(page.getByTestId(`store-results-${backend}`)).toHaveText(
      `${takeId}: 5 chunks (0,1,2,3,4), intact`,
    );

    await page.getByTestId(`store-clear-${backend}`).click();
    await expect(status).toHaveText('cleared');
  });
}

test('openChunkStore picks a backend', async ({ page }) => {
  await page.goto('/debug');
  await expect(page.getByTestId('store-auto')).toHaveText(
    /openChunkStore\(\) picks: (opfs|indexeddb)$/,
  );
});
