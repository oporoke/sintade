import { signUpAndLogIn } from './support/auth';
import { NetworkProbe, expectNothingLost, finishedTake, startRecording } from './support/loss';
import { expect, test } from './support/test';

/**
 * Day 42: loss scenarios through the real `/record` page (§11 "0 loss for any recording whose
 * chunks reached OPFS"; US-20 "the network drops for 60 s … upload on reconnect with no
 * loss"). Each test checks that the server ends up with every chunk the recorder made, each
 * byte-for-byte what the browser sent. The 10-minute, 60-second-cut version is the M4 demo
 * (`m4-demo.spec.ts`, `just demo-m4`); these are its CI-sized cousins.
 */
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);

test('offline mid-recording: chunks wait on the device and upload on reconnect', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  await startRecording(page);

  await expect.poll(() => probe.accepted, { timeout: 15_000 }).toBeGreaterThanOrEqual(2);
  await probe.goOffline();
  await page.waitForTimeout(1_000); // a PUT already on the wire may still land
  const beforeCut = probe.accepted;
  await page.waitForTimeout(11_000);
  // Recording carried on; nothing reached storage meanwhile.
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  expect(probe.accepted).toBe(beforeCut);
  await probe.goOnline();
  // The backlog drains while recording continues.
  await expect.poll(() => probe.accepted, { timeout: 20_000 }).toBeGreaterThan(beforeCut + 4);
  await page.waitForTimeout(3_000);
  await page.getByTestId('recorder-stop').click();

  const take = await finishedTake(page);
  expect(take.chunkCount).toBeGreaterThanOrEqual(9);
  await expectNothingLost(page, probe, take.takeId, take.chunkCount);
  test.info().annotations.push({
    type: 'offline mid-recording',
    description: `${take.chunkCount} chunks; ${beforeCut} uploaded before the cut`,
  });
});

test('stopped while offline: the recorder waits, then finalizes on reconnect', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  await startRecording(page);
  await page.waitForTimeout(4_000);
  await probe.goOffline();
  await page.waitForTimeout(4_000);
  await page.getByTestId('recorder-stop').click();

  // Stop can't finish the take yet: it says it's offline and keeps the take.
  const status = page.getByTestId('recorder-upload-status');
  await expect(status).toHaveAttribute('data-state', 'offline', { timeout: 10_000 });
  await page.waitForTimeout(5_000);
  await expect(page.getByTestId('recorder-uploading')).toBeVisible();
  await expect(page.getByTestId('recorder-done')).toHaveCount(0);

  await probe.goOnline();
  const take = await finishedTake(page);
  await expectNothingLost(page, probe, take.takeId, take.chunkCount);
});

test('a silent network cut (requests fail, browser still "online") recovers by retrying', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  await startRecording(page);

  await expect.poll(() => probe.accepted, { timeout: 15_000 }).toBeGreaterThanOrEqual(2);
  probe.cutSilently();
  await page.waitForTimeout(10_000);
  probe.restore();
  await page.waitForTimeout(2_000);
  await page.getByTestId('recorder-stop').click();

  // Retries back off (1, 2, 4, 8, 16 s…), so the first one after the cut may be a while.
  const take = await finishedTake(page, 60_000);
  await expectNothingLost(page, probe, take.takeId, take.chunkCount);
});

test('cut off, then the tab dies before reconnecting: recovery uploads every chunk', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  const recording = await context.newPage();
  await startRecording(recording);
  await expect.poll(() => probe.accepted, { timeout: 15_000 }).toBeGreaterThanOrEqual(2);
  await probe.goOffline();
  await recording.waitForTimeout(6_000);
  // The "crash": no unload handlers run; only what was persisted survives.
  await recording.close({ runBeforeUnload: false });
  await probe.goOnline();

  await page.goto('/record');
  const dialog = page.getByTestId('recovery-dialog');
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  await expect(dialog.getByTestId('recovery-take')).toHaveCount(1);
  await dialog.getByTestId('recovery-upload').click();
  const uploaded = dialog.getByTestId('recovery-uploaded');
  const error = dialog.getByTestId('recovery-error');
  await expect(uploaded.or(error)).toBeVisible({ timeout: 30_000 });
  await expect(error).toHaveCount(0);

  const takeId = await uploaded.getAttribute('data-server-take-id');
  const chunkCount = Number(await uploaded.getAttribute('data-chunk-count'));
  expect(chunkCount).toBeGreaterThanOrEqual(4);
  await expectNothingLost(page, probe, takeId, chunkCount);
});

test('"Stop sharing" from the browser ends the take and the upload completes', async ({
  context,
  page,
  request,
}) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);
  const probe = await NetworkProbe.attach(context);
  // Keep a handle on the shared screen, so the test can end it the way the browser's own
  // "Stop sharing" bar does (the track ends and fires `ended`).
  await page.addInitScript(() => {
    const original = MediaDevices.prototype.getDisplayMedia;
    Object.defineProperty(MediaDevices.prototype, 'getDisplayMedia', {
      configurable: true,
      writable: true,
      value: async function (this: MediaDevices, options?: DisplayMediaStreamOptions) {
        const stream = await original.call(this, options);
        (globalThis as { sharedScreen?: MediaStream }).sharedScreen = stream;
        return stream;
      },
    });
  });
  await startRecording(page);
  await page.waitForTimeout(6_000);
  await page.evaluate(() => {
    const [track] =
      (globalThis as { sharedScreen?: MediaStream }).sharedScreen?.getVideoTracks() ?? [];
    track?.stop();
    track?.dispatchEvent(new Event('ended'));
  });

  const take = await finishedTake(page);
  expect(take.chunkCount).toBeGreaterThanOrEqual(2);
  await expectNothingLost(page, probe, take.takeId, take.chunkCount);
});
