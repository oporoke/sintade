import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { signUpAndLogIn } from './support/auth';
import { NetworkProbe, expectNothingLost, finishedTake, startRecording } from './support/loss';
import { expect, test } from './support/test';

/**
 * M4 demo (Day 42): "10-minute recording, network cut for 60 s, nothing lost". Records through
 * the real `/record` page with a microphone until the free plan's 10-minute limit stops it
 * (Day 41), with the browser offline from 3:00 to 4:00. Then every chunk the recorder made must
 * be on the server, byte-for-byte what the browser sent. Long, so it only runs with `DEMO=1`
 * (`just demo-m4`); the film and a JSON result go to `web/demo-output/m4/<browser>/`.
 */
test.skip(!process.env['DEMO'], 'M4 demo: run with DEMO=1 (just demo-m4)');
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);
test.use({
  video: { mode: 'on', size: { width: 1280, height: 720 } },
  viewport: { width: 1280, height: 720 },
});

const PLAN_LIMIT_MS = 10 * 60_000;
const CUT_AT_MS = 3 * 60_000;
const CUT_FOR_MS = 60_000;
const OUTPUT = join(__dirname, '..', 'demo-output', 'm4');

test('10-minute recording, network cut for 60 s, nothing lost', async ({
  context,
  page,
  request,
  browserName,
}) => {
  test.setTimeout(PLAN_LIMIT_MS + 5 * 60_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  await startRecording(page, { mic: true, countdown: true });
  const startedAt = Date.now();

  await page.waitForTimeout(CUT_AT_MS);
  const beforeCut = probe.accepted;
  await probe.goOffline();
  await page.waitForTimeout(CUT_FOR_MS);
  const duringCut = probe.accepted - beforeCut;
  await probe.goOnline();
  const reconnectedAt = Date.now();
  // The minute's backlog (about 30 chunks) catches up while recording carries on.
  await expect
    .poll(() => probe.accepted, { timeout: 60_000, intervals: [1_000] })
    .toBeGreaterThanOrEqual(beforeCut + duringCut + 30);
  const caughtUpMs = Date.now() - reconnectedAt;

  // The recorder stops itself just inside the plan's limit.
  await expect(page.getByTestId('recorder-limit-notice')).toBeVisible({
    timeout: PLAN_LIMIT_MS + 60_000 - (Date.now() - startedAt),
  });
  const take = await finishedTake(page, 60_000);
  expect(take.durationMs).toBeGreaterThan(PLAN_LIMIT_MS - 5_000);
  expect(take.durationMs).toBeLessThanOrEqual(PLAN_LIMIT_MS);
  const status = await expectNothingLost(page, probe, take.takeId, take.chunkCount);
  const bytes = status.chunks.reduce((sum, chunk) => sum + chunk.size_bytes, 0);
  await page.waitForTimeout(3_000); // let the done screen show on film

  const result = {
    browser: browserName,
    takeId: take.takeId,
    durationMs: take.durationMs,
    chunks: take.chunkCount,
    serverChunks: status.received.length,
    bytes,
    hashesMatch: true,
    uploadedBeforeCut: beforeCut,
    acceptedDuringCut: duringCut,
    backlogCaughtUpMs: caughtUpMs,
    stopToFinalizeMs: take.stopToFinalizeMs,
    storagePuts: probe.accepted,
  };
  test.info().annotations.push({ type: 'M4 demo', description: JSON.stringify(result) });
  const dir = join(OUTPUT, browserName);
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, 'result.json'), `${JSON.stringify(result, null, 2)}\n`);
  const video = page.video();
  await page.close();
  // saveAs waits for Playwright to finish writing the film (a 10-minute one takes a while).
  await video?.saveAs(join(dir, 'recording.webm'));
});
