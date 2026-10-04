import { execFileSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { signUpAndLogIn } from './support/auth';
import { anonymousPage, apiAs, recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 56 Check: "downloaded file plays offline". The owner records 5 s, opens their own link and
 * clicks Download; the saved file is a complete, self-contained H.264/AAC MP4 that FFmpeg
 * decodes end to end with no network. A stranger gets no Download button until the owner turns
 * downloads on for the link.
 */
test('the owner downloads the MP4 and it decodes offline; viewers need the link to allow it', async ({
  browser,
  page,
  request,
  browserName,
}) => {
  test.skip(browserName === 'webkit', "Playwright's Linux WebKit has no MediaRecorder (Day 22)");
  test.setTimeout(120_000);
  await signUpAndLogIn(page, request);
  const shared = await recordAndShare(page, 5);

  // The owner, signed in, opens the link once it is processed.
  const watch = await page.context().newPage();
  await watch.goto(`/s/${shared.slug}`);
  await expect(async () => {
    await watch.reload();
    await expect(watch.getByTestId('watch-video')).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 60_000, intervals: [1_000] });

  const [download] = await Promise.all([
    watch.waitForEvent('download'),
    watch.getByTestId('watch-download').click(),
  ]);
  expect(download.suggestedFilename()).toBe('Untitled recording.mp4');
  const dir = join(__dirname, '..', 'test-results', 'download', browserName);
  mkdirSync(dir, { recursive: true });
  const file = join(dir, 'saved.mp4');
  await download.saveAs(file);

  // Offline: nothing but the file. It decodes completely, as H.264 + AAC, about 5 s long.
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
  expect(probe.streams.find((s) => s.codec_type === 'video')?.codec_name).toBe('h264');
  expect(Number(probe.format.duration)).toBeGreaterThan(3);
  expect(Number(probe.format.duration)).toBeLessThan(9);

  // A stranger: no Download until the link allows it.
  const stranger = await anonymousPage(browser);
  await stranger.goto(`/s/${shared.slug}`);
  await expect(stranger.getByTestId('watch-video')).toBeVisible({ timeout: 15_000 });
  await expect(stranger.getByTestId('watch-download')).toHaveCount(0);
  const denied = await stranger.evaluate(
    async (slug) => (await fetch(`/api/v1/s/${slug}/download`)).status,
    shared.slug,
  );
  expect(denied).toBe(403);

  await apiAs(page, 'PATCH', `/recordings/${shared.recordingId}/links/${shared.linkId}`, {
    allow_download: true,
  });
  await stranger.reload();
  await expect(stranger.getByTestId('watch-download')).toBeVisible();
  const [theirs] = await Promise.all([
    stranger.waitForEvent('download'),
    stranger.getByTestId('watch-download').click(),
  ]);
  expect(theirs.suggestedFilename()).toBe('Untitled recording.mp4');
  await stranger.context().close();
});
