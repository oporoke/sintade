import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { anonymousPage, apiAs, openWhenProcessed } from './support/share';
import { expect, test } from './support/test';

/**
 * M8 launch rehearsal (Day 70): the MVP exit criteria (docs/design.md §12) that a browser test can
 * show, run against the API at its *production* rate limits (start the stack with
 * RATE_LIMIT_SCALE=1, which `just rehearse-launch` does). Long, so only with LAUNCH=1.
 *
 *  - kill the tab late in a recording, reopen, and get the whole recording back with no gaps;
 *  - stop → link ≤ 5 s at the 95th percentile, over many recordings.
 */
test.skip(!process.env['LAUNCH'], 'Launch rehearsal: run with LAUNCH=1 (just rehearse-launch)');
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);

const OUT = join(__dirname, '..', 'demo-output', 'm8');

/** Nearest-rank percentile of a list of numbers. */
function percentile(values: number[], p: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.ceil((p / 100) * sorted.length);
  return sorted[Math.max(0, rank - 1)] ?? 0;
}

test('kill the tab at minute 9, reopen: the recording comes back whole, with no gaps', async ({
  context,
  page,
  request,
  browserName,
}) => {
  test.setTimeout(30 * 60_000);
  const dir = join(OUT, browserName);
  mkdirSync(dir, { recursive: true });
  await signUpAndLogIn(page, request);

  const recording = await context.newPage();
  await startRecording(recording, { mic: true });
  const startedAt = Date.now();
  await recording.waitForTimeout(9 * 60_000);
  // The crash: nothing runs on the way out. Only what was already persisted survives.
  await recording.close({ runBeforeUnload: false });
  const recordedMs = Date.now() - startedAt;

  await page.goto('/record');
  const dialog = page.getByTestId('recovery-dialog');
  await expect(dialog).toBeVisible({ timeout: 15_000 });
  await expect(dialog.getByTestId('recovery-take')).toHaveCount(1);
  await dialog.getByTestId('recovery-upload').click();
  const uploaded = dialog.getByTestId('recovery-uploaded');
  await expect(uploaded.or(dialog.getByTestId('recovery-error'))).toBeVisible({ timeout: 60_000 });
  await expect(dialog.getByTestId('recovery-error')).toHaveCount(0);
  const chunkCount = Number(await uploaded.getAttribute('data-chunk-count'));

  // The recovered recording is the newest in the library; share it and wait for the MP4.
  const list = (await apiAs(page, 'GET', '/recordings')).body as { items: { id: string }[] };
  const recordingId = list.items[0]?.id ?? '';
  expect(recordingId).not.toBe('');
  const link = (await apiAs(page, 'POST', `/recordings/${recordingId}/links`, {
    visibility: 'link',
  })) as { body: { slug: string } };
  const watch = await context.newPage();
  const processingStarted = Date.now();
  await openWhenProcessed(watch, link.body.slug);
  const processingMs = Date.now() - processingStarted;
  await watch.getByTestId('watch-video').evaluate(async (el: HTMLVideoElement) => {
    el.pause();
    el.currentTime = el.duration / 2;
    await new Promise((resolve) => el.addEventListener('seeked', resolve, { once: true }));
  });

  // The file itself: download it and look at every frame.
  const grant = (await apiAs(page, 'GET', `/recordings/${recordingId}/download`)).body as {
    url: string;
  };
  const bytes = await (await request.get(grant.url, { ignoreHTTPSErrors: true })).body();
  const file = join(dir, 'recovered.mp4');
  writeFileSync(file, bytes);
  execFileSync('ffmpeg', ['-v', 'error', '-xerror', '-i', file, '-f', 'null', '-'], {
    stdio: 'pipe',
  });
  const probe = JSON.parse(
    execFileSync('ffprobe', ['-v', 'error', '-print_format', 'json', '-show_format', file], {
      encoding: 'utf8',
    }),
  ) as { format: { duration: string } };
  const durationMs = Number(probe.format.duration) * 1000;
  const pts = execFileSync(
    'ffprobe',
    [
      '-v',
      'error',
      '-select_streams',
      'v:0',
      '-show_entries',
      'packet=pts_time',
      '-of',
      'csv=p=0',
      file,
    ],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
  )
    .split('\n')
    .map(Number)
    .filter((n) => Number.isFinite(n))
    .sort((a, b) => a - b);
  const longestGapS = pts.slice(1).reduce((max, t, i) => Math.max(max, t - (pts[i] ?? t)), 0);

  const result = {
    browser: browserName,
    recordedBeforeKillMs: Math.round(recordedMs),
    chunks: chunkCount,
    recoveredDurationMs: Math.round(durationMs),
    lostAtTheEndMs: Math.round(recordedMs - durationMs),
    longestFrameGapS: Number(longestGapS.toFixed(2)),
    processingAfterRecoveryMs: processingMs,
  };
  test.info().annotations.push({ type: 'recovery', description: JSON.stringify(result) });
  writeFileSync(join(dir, 'recovery.json'), `${JSON.stringify(result, null, 2)}\n`);

  // At most the last unflushed timeslice (2 s) plus the time the recorder took to start is lost.
  expect(durationMs).toBeLessThanOrEqual(recordedMs + 1_500);
  expect(recordedMs - durationMs).toBeLessThan(5_000);
  // "No gap longer than the last unflushed timeslice" (US-12): no stretch of the video is missing.
  expect(longestGapS).toBeLessThanOrEqual(2);
});

test('stop → link ≤ 5 s at the 95th percentile, over 20 recordings', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.setTimeout(20 * 60_000);
  const dir = join(OUT, browserName);
  mkdirSync(dir, { recursive: true });
  await signUpAndLogIn(page, request);

  const stopToLink: number[] = [];
  const stopToPlayable: number[] = [];
  const firstFrame: number[] = [];
  const viewer = await anonymousPage(browser);
  for (let i = 0; i < 20; i += 1) {
    await startRecording(page, { mic: false });
    await page.waitForTimeout(4_000);
    const stoppedAt = Date.now();
    await page.getByTestId('recorder-stop').click();
    await finishedTake(page);
    const link = page.getByTestId('recorder-share-link');
    await expect(link).toBeVisible({ timeout: 15_000 });
    stopToLink.push(Date.now() - stoppedAt);
    const url = (await link.locator('a').getAttribute('href')) ?? '';

    await viewer.goto(url);
    const player = viewer.getByTestId('watch-player');
    await expect(player).toHaveAttribute('data-first-frame-ms', /^\d+$/, { timeout: 30_000 });
    stopToPlayable.push(Date.now() - stoppedAt);
    firstFrame.push(Number(await player.getAttribute('data-first-frame-ms')));
    await page.getByTestId('recorder-new').click();
  }

  const summary = (values: number[]) => ({
    p50: percentile(values, 50),
    p75: percentile(values, 75),
    p95: percentile(values, 95),
    max: Math.max(...values),
  });
  const result = {
    browser: browserName,
    recordings: stopToLink.length,
    stopToLinkMs: summary(stopToLink),
    stopToPlayableMs: summary(stopToPlayable),
    firstFrameMs: summary(firstFrame),
  };
  test.info().annotations.push({ type: 'latency', description: JSON.stringify(result) });
  writeFileSync(join(dir, 'latency.json'), `${JSON.stringify(result, null, 2)}\n`);

  expect(result.stopToLinkMs.p95).toBeLessThanOrEqual(5_000);
  expect(result.stopToPlayableMs.p95).toBeLessThanOrEqual(5_000);
  expect(result.firstFrameMs.p75).toBeLessThanOrEqual(1_500);
  await viewer.context().close();
});
