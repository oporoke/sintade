import { Page, expect, test } from '@playwright/test';

/**
 * Days 23–24: the debug page's device-free recorder self-test records an animated canvas plus a
 * tone with the real ChunkRecorder in 2 s slices, concatenates the chunks in index order and
 * plays the result to the end.
 */

interface ScenarioResult {
  summary: string;
  chunkCount: number;
  chunkSizes: number[];
  recordedSeconds: number;
  playedSeconds: number;
  playback: string;
}

/** Runs one scenario; returns null when this engine can't record (asserted to be WebKit only). */
async function runScenario(
  page: Page,
  button: string,
  browserName: string,
): Promise<ScenarioResult | null> {
  await page.goto('/debug');
  await page.getByTestId(button).click();

  const error = page.getByTestId('rec-selftest-error');
  const playback = page.getByTestId('rec-selftest-playback');
  await expect(playback.or(error)).toBeVisible({ timeout: 45_000 });
  if (await error.isVisible()) {
    // Playwright's Linux WebKit build ships without MediaRecorder (Safari has it; see Day 22).
    await expect(error).toHaveText('MediaRecorder is not available in this browser');
    expect(browserName, 'only the Linux WebKit build may lack MediaRecorder').toBe('webkit');
    return null;
  }

  const chunks = (await page.getByTestId('rec-selftest-chunks').textContent()) ?? '';
  const playbackText = ((await playback.textContent()) ?? '').trim();
  return {
    summary: ((await page.getByTestId('rec-selftest-summary').textContent()) ?? '').trim(),
    chunkCount: Number(/: (\d+) chunks/.exec(chunks)?.[1]),
    chunkSizes: (/\(([\d, ]+) bytes\)/.exec(chunks)?.[1] ?? '')
      .split(',')
      .map((size) => Number(size.trim())),
    recordedSeconds: Number(/over ([\d.]+) s/.exec(chunks)?.[1]),
    playedSeconds: Number(/([\d.]+) s$/.exec(playbackText)?.[1]),
    playback: playbackText,
  };
}

test('concatenated chunks play as a valid file', async ({ page, browserName }, testInfo) => {
  const result = await runScenario(page, 'rec-selftest-run', browserName);
  if (!result) {
    return;
  }
  // 6 s in 2 s slices: at least three chunks, all non-empty.
  expect(result.chunkCount).toBeGreaterThanOrEqual(3);
  expect(result.chunkSizes.every((size) => size > 0)).toBe(true);
  // The concatenation decodes (real frame size) and plays through to the end of the recording.
  expect(result.playback).toContain('played to the end');
  expect(result.playback).toContain('640×360');
  expect(result.playedSeconds).toBeGreaterThan(4.5);
  expect(result.playedSeconds).toBeLessThan(7.5);

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByTestId('rec-selftest-download').click(),
  ]);
  await download.saveAs(testInfo.outputPath('recorder-self-test.webm'));
  await page.screenshot({ path: testInfo.outputPath('recorder-self-test.png'), fullPage: true });
});

/** Day 24 Check: "Clip with a pause plays correctly". 2 s + (2 s paused) + 2 s. */
test('a clip with a pause plays correctly', async ({ page, browserName }, testInfo) => {
  const result = await runScenario(page, 'rec-selftest-run-pause', browserName);
  if (!result) {
    return;
  }
  expect(result.summary).toBe('pause: stopped by stop()');
  // The timer excluded the 2 s pause...
  expect(result.recordedSeconds).toBeGreaterThan(3.8);
  expect(result.recordedSeconds).toBeLessThan(4.5);
  // ...and so does the file: it plays to the end, ~4 s of media rather than 6.
  expect(result.playback).toContain('played to the end');
  expect(result.playback).toContain('640×360');
  expect(result.playedSeconds).toBeGreaterThan(3.3);
  expect(result.playedSeconds).toBeLessThan(5);
  await page.screenshot({ path: testInfo.outputPath('recorder-pause.png'), fullPage: true });
});

test('the recorder stops by itself when the source track ends', async ({ page, browserName }) => {
  const result = await runScenario(page, 'rec-selftest-run-track-ended', browserName);
  if (!result) {
    return;
  }
  expect(result.summary).toBe('track-ended: stopped by track ended');
  expect(result.recordedSeconds).toBeGreaterThan(2.8);
  expect(result.recordedSeconds).toBeLessThan(3.8);
  expect(result.playback).toContain('played to the end');
  expect(result.playedSeconds).toBeGreaterThan(2.3);
  expect(result.playedSeconds).toBeLessThan(4.2);
});
