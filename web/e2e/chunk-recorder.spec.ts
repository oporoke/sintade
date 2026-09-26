import { expect, test } from '@playwright/test';

/**
 * Day 23 Check: "Concatenated chunks play as a valid file". Device-free: the debug page records
 * 6 s of an animated canvas plus a tone with the real ChunkRecorder in 2 s slices, concatenates
 * the chunks in index order, and plays the result to the end.
 */
test('concatenated chunks play as a valid file', async ({ page, browserName }, testInfo) => {
  await page.goto('/debug');
  await page.getByTestId('rec-selftest-run').click();

  const error = page.getByTestId('rec-selftest-error');
  const playback = page.getByTestId('rec-selftest-playback');
  await expect(playback.or(error)).toBeVisible({ timeout: 45_000 });

  if (await error.isVisible()) {
    // Playwright's Linux WebKit build ships without MediaRecorder (Safari has it; see Day 22).
    await expect(error).toHaveText('MediaRecorder is not available in this browser');
    expect(browserName, 'only the Linux WebKit build may lack MediaRecorder').toBe('webkit');
    return;
  }

  // 6 s in 2 s slices: at least three chunks, all non-empty.
  const chunks = (await page.getByTestId('rec-selftest-chunks').textContent()) ?? '';
  const count = Number(/: (\d+) chunks/.exec(chunks)?.[1]);
  expect(count).toBeGreaterThanOrEqual(3);
  const sizes = /\(([\d, ]+) bytes\)/
    .exec(chunks)?.[1]
    .split(',')
    .map((size) => Number(size.trim()));
  expect(sizes?.every((size) => size > 0)).toBe(true);

  // The concatenation decodes (real frame size) and plays through to the end of the recording.
  await expect(playback).toContainText('played to the end');
  await expect(playback).toContainText('640×360');
  const played = Number(/([\d.]+) s$/.exec(((await playback.textContent()) ?? '').trim())?.[1]);
  expect(played).toBeGreaterThan(4.5);
  expect(played).toBeLessThan(7.5);

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByTestId('rec-selftest-download').click(),
  ]);
  const file = testInfo.outputPath('recorder-self-test.webm');
  await download.saveAs(file);
  testInfo.annotations.push({ type: 'chunks', description: chunks.trim() });
  await page.screenshot({ path: testInfo.outputPath('recorder-self-test.png'), fullPage: true });
});
