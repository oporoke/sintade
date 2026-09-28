import { signUpAndLogIn } from './support/auth';
import { expect, test } from './support/test';

interface StoredTake {
  chunks: { name: string; size: number }[];
  journal: { chunkCount: number; durationMs: number; mimeType: string } | null;
}

/**
 * Day 31 Check: "E2E: record 5 s, chunks present in OPFS", on the fake-media harness. The take
 * is recorded through the real recorder page, then OPFS is read directly from the page.
 */
test('record 5 s: chunks and journal are in OPFS', async ({ page, request, browserName }) => {
  await signUpAndLogIn(page, request);
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape'); // skip the countdown

  if (browserName === 'webkit') {
    // Playwright's Linux WebKit ships without MediaRecorder (Safari has it; Day 22), so the
    // harness takes it as far as recording can go there: a clear problem, never a blank page.
    await expect(page.getByTestId('recorder-problem-title')).toHaveText(
      "This browser can't record",
    );
    return;
  }

  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(5000);
  await page.getByTestId('recorder-stop').click();
  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 10_000 });
  const takeId = await summary.getAttribute('data-take-id');

  const stored: StoredTake = await page.evaluate(async (id) => {
    const root = await navigator.storage.getDirectory();
    const take = await (
      await root.getDirectoryHandle('sintade-chunks')
    ).getDirectoryHandle(id ?? '');
    const chunks: { name: string; size: number }[] = [];
    let journal = null;
    for await (const [name, handle] of take.entries()) {
      if (handle.kind !== 'file') continue;
      const file = await (handle as FileSystemFileHandle).getFile();
      if (name === 'take.json') journal = JSON.parse(await file.text());
      else chunks.push({ name, size: file.size });
    }
    return { chunks: chunks.sort((a, b) => a.name.localeCompare(b.name)), journal };
  }, takeId);

  test.info().annotations.push({ type: 'opfs', description: JSON.stringify(stored) });
  // 5 s in 2 s slices: at least 3 non-empty chunks, numbered from 0 without gaps.
  expect(stored.chunks.length).toBeGreaterThanOrEqual(3);
  expect(stored.chunks.every((chunk) => chunk.size > 0)).toBe(true);
  expect(stored.chunks.map((chunk) => chunk.name)).toEqual(
    stored.chunks.map((_, index) => `${String(index).padStart(6, '0')}.chunk`),
  );
  expect(stored.journal?.chunkCount).toBe(stored.chunks.length);
  expect(stored.journal?.durationMs).toBeGreaterThan(4500);
  expect(stored.journal?.durationMs).toBeLessThan(7000);
});
