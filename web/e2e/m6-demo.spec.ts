import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { apiAs } from './support/share';
import { expect, test } from './support/test';

/**
 * M6 demo (Day 60): the whole loop. A creator records 10 s on `/record` and stops; the link is
 * made and copied; a stranger opens it seconds later and watches (first the original as a
 * preview, then the MP4, switched by the page itself); the creator turns the link private (the
 * stranger loses it) and back, finds the recording in the library, renames it in place,
 * downloads the MP4 (decoded offline), allows downloads for viewers, and finally moves it to
 * the trash, which kills the link at once. Only runs with `DEMO=1` (`just demo-m6`, with
 * `just worker` running); films and a JSON result go to `web/demo-output/m6/<browser>/`.
 */
test.skip(!process.env['DEMO'], 'M6 demo: run with DEMO=1 (just demo-m6)');
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);
test.use({
  video: { mode: 'on', size: { width: 1280, height: 720 } },
  viewport: { width: 1280, height: 720 },
});

const OUTPUT = join(__dirname, '..', 'demo-output', 'm6');

test('record → share → watch → library → download → trash', async ({
  browser,
  context,
  page,
  request,
  browserName,
}) => {
  test.setTimeout(240_000);
  const dir = join(OUTPUT, browserName);
  mkdirSync(dir, { recursive: true });
  if (browserName === 'chromium') {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  }
  const email = await signUpAndLogIn(page, request);

  // 1. Record 10 s and stop; the link is created and copied.
  await startRecording(page, { mic: true });
  await page.waitForTimeout(10_000);
  const stoppedAt = Date.now();
  await page.getByTestId('recorder-stop').click();
  const take = await finishedTake(page);
  const line = page.getByTestId('recorder-share-link');
  await expect(line).toBeVisible({ timeout: 10_000 });
  const stopToLinkMs = Date.now() - stoppedAt;
  const url = (await line.locator('a').getAttribute('href')) ?? '';
  expect(url).toMatch(/\/s\/[0-9A-Za-z]{12}$/);
  const copied = (await line.getAttribute('data-copied')) === 'true';
  if (browserName === 'chromium') {
    expect(copied).toBe(true);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);
  }
  const recordingId = await page
    .getByTestId('recorder-done-summary')
    .getAttribute('data-recording-id');
  const slug = url.split('/').pop() ?? '';
  const links = (await apiAs(page, 'GET', `/recordings/${recordingId}/links`)).body as {
    id: string;
  }[];
  const linkId = links[0].id;

  // 2. A stranger opens it straight away.
  const strangerContext = await browser.newContext({
    ignoreHTTPSErrors: true,
    viewport: { width: 1280, height: 720 },
    recordVideo: { dir: join(dir, 'viewer'), size: { width: 1280, height: 720 } },
  });
  const stranger = await strangerContext.newPage();
  await stranger.goto(url);
  const video = stranger.getByTestId('watch-video');
  const player = stranger.getByTestId('watch-player');
  await expect(video).toBeVisible({ timeout: 30_000 });
  await expect(player).toHaveAttribute('data-first-frame-ms', /^\d+$/, { timeout: 30_000 });
  const stopToPlayableMs = Date.now() - stoppedAt;
  const firstFrameMs = Number(await player.getAttribute('data-first-frame-ms'));
  const previewedFirst = (await stranger.getByTestId('watch-preview').count()) > 0;
  await expect(stranger.getByTestId('watch-preview')).toHaveCount(0, { timeout: 60_000 });
  const stopToMp4Ms = Date.now() - stoppedAt;
  await expect(video).toHaveAttribute('src', /mp4\/default\.mp4/);
  await expect(stranger.getByTestId('watch-title')).toHaveText('Untitled recording');

  // Play it: speed, seek, play/pause.
  await stranger.getByTestId('watch-speed').selectOption('1.5');
  await player.focus();
  await video.evaluate((el: HTMLVideoElement) => {
    el.pause();
    el.currentTime = 0;
  });
  await stranger.keyboard.press('ArrowRight');
  expect(await video.evaluate((el: HTMLVideoElement) => el.currentTime)).toBeCloseTo(5, 0);
  await stranger.keyboard.press(' ');
  await expect.poll(() => video.evaluate((el: HTMLVideoElement) => !el.paused)).toBe(true);
  await stranger.waitForTimeout(1_500);

  // 3. Private: gone for the stranger; back to anyone-with-the-link: back.
  await page.getByTestId('recorder-share').click();
  const dialog = page.getByTestId('share-dialog');
  await expect(dialog.getByTestId('share-url')).toHaveValue(url);
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    dialog.getByTestId('share-visibility').selectOption('private'),
  ]);
  await stranger.reload();
  await expect(stranger.getByTestId('watch-not-found')).toBeVisible();
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    dialog.getByTestId('share-visibility').selectOption('link'),
  ]);
  await dialog.getByTestId('share-close').click();
  await stranger.reload();
  await expect(stranger.getByTestId('watch-title')).toBeVisible();

  // 4. The library: thumbnail, rename in place.
  await page.goto('/library');
  const card = page.locator(`[data-recording-id="${recordingId}"]`);
  await expect(card).toBeVisible();
  await expect(card.locator('img')).toBeVisible();
  expect(
    await card
      .locator('img')
      .evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0),
  ).toBe(true);
  await card.getByTestId('library-rename').click();
  const input = card.getByTestId('library-title-input');
  await input.fill('Sprint demo');
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'PATCH' && r.ok()),
    input.press('Enter'),
  ]);
  await expect(card.getByTestId('library-title')).toHaveText('Sprint demo');
  await stranger.reload();
  await expect(stranger.getByTestId('watch-title')).toHaveText('Sprint demo');

  // 5. Download the MP4: it decodes with no network, and viewers can't until it's allowed.
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    card.getByTestId('library-download').click(),
  ]);
  expect(download.suggestedFilename()).toBe('Sprint demo.mp4');
  const file = join(dir, 'Sprint demo.mp4');
  await download.saveAs(file);
  execFileSync('ffmpeg', ['-v', 'error', '-xerror', '-i', file, '-f', 'null', '-'], {
    stdio: 'pipe',
  });
  const probe = JSON.parse(
    execFileSync(
      'ffprobe',
      ['-v', 'error', '-print_format', 'json', '-show_format', '-show_streams', file],
      { encoding: 'utf8' },
    ),
  ) as { format: { duration: string }; streams: { codec_type: string; codec_name: string }[] };
  const downloadedSeconds = Number(probe.format.duration);
  expect(downloadedSeconds).toBeGreaterThan(8);
  expect(downloadedSeconds).toBeLessThan(13);
  await expect(stranger.getByTestId('watch-download')).toHaveCount(0);
  await apiAs(page, 'PATCH', `/recordings/${recordingId}/links/${linkId}`, {
    allow_download: true,
  });
  await stranger.reload();
  await expect(stranger.getByTestId('watch-download')).toBeVisible();

  // 6. Trash it: the link dies at once.
  await card.getByTestId('library-trash').click();
  await Promise.all([
    page.waitForResponse((r) => r.request().method() === 'DELETE' && r.status() === 204),
    card.getByTestId('library-trash-yes').click(),
  ]);
  await expect(card).toHaveCount(0);
  await stranger.reload();
  await expect(stranger.getByTestId('watch-not-found')).toBeVisible();
  await stranger.waitForTimeout(1_500);

  const result = {
    browser: browserName,
    user: email.replace(/^.*@/, '@'),
    recordingId,
    slug,
    recordedMs: take.durationMs,
    chunks: take.chunkCount,
    clipboardCopied: copied,
    stopToFinalizeMs: take.stopToFinalizeMs,
    stopToLinkMs,
    stopToPlayableMs,
    firstFrameMs,
    previewedFirst,
    stopToMp4Ms,
    downloadedSeconds,
    downloadedCodecs: probe.streams.map((s) => s.codec_name),
  };
  test.info().annotations.push({ type: 'M6 demo', description: JSON.stringify(result) });
  writeFileSync(join(dir, 'result.json'), `${JSON.stringify(result, null, 2)}\n`);
  const ownerFilm = page.video();
  const viewerFilm = stranger.video();
  await page.close();
  await strangerContext.close();
  await ownerFilm?.saveAs(join(dir, 'owner.webm'));
  await viewerFilm?.saveAs(join(dir, 'viewer.webm'));
});
