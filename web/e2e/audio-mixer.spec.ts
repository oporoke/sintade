import { expect, test } from '@playwright/test';

/**
 * Day 22 Check: "Test clip contains both audio sources". The self-test needs no devices or
 * permissions, so it runs on every engine: a 440 Hz "mic" tone and a 1000 Hz "display" tone go
 * through the real AudioMixer into a MediaRecorder clip, which is decoded and analysed; the
 * mixer's per-source and mix meters are checked against theory mid-clip.
 */
test('mixed test clip contains both audio sources', async ({ page }, testInfo) => {
  await page.goto('/debug');
  await page.getByTestId('selftest-run').click();

  await expect(page.getByTestId('selftest-results')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId('selftest-error')).toHaveCount(0);
  await expect(page.getByTestId('selftest-tone-440')).toHaveText('present');
  await expect(page.getByTestId('selftest-tone-1000')).toHaveText('present');
  await expect(page.getByTestId('selftest-tone-2500')).toHaveText('absent');

  // Meters read mid-clip: each 0.25-amplitude sine is RMS ≈ 0.177; the two summed ≈ 0.25.
  // (Deterministic, unlike metering Chromium's fake mic: its 2 Hz beep phase-locks with the
  // 100 ms level sampling, so a run either always or never catches it.)
  const levels = (await page.getByTestId('selftest-levels').textContent()) ?? '';
  const [mic, display, mix] = [...levels.matchAll(/(\d+\.\d+)/g)].map((m) => Number(m[1]));
  expect(mic).toBeCloseTo(0.177, 1);
  expect(display).toBeCloseTo(0.177, 1);
  expect(mix).toBeCloseTo(0.25, 1);

  const clip = (await page.getByTestId('selftest-clip').textContent())?.trim();
  testInfo.annotations.push({ type: 'clip', description: clip });
  await page.screenshot({ path: testInfo.outputPath('mix-self-test.png'), fullPage: true });
});
