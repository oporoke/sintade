import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { signUpAndLogIn } from './support/auth';
import { finishedTake, startRecording } from './support/loss';
import { expect, test } from './support/test';

/**
 * M5 demo (Day 51): record → stop → MP4 ready → email. Records 15 s through the real `/record`
 * page, then (with the API and a `worker` running) waits for the worker to process the take,
 * checks the recording, its MP4 and poster in the database and storage (ffprobe on the stored
 * MP4), and waits for the "recording ready" email in Mailpit. Only runs with `DEMO=1`
 * (`just demo-m5`); the film and a JSON result go to `web/demo-output/m5/<browser>/`.
 */
test.skip(!process.env['DEMO'], 'M5 demo: run with DEMO=1 (just demo-m5)');
test.skip(
  ({ browserName }) => browserName === 'webkit',
  "Playwright's Linux WebKit has no MediaRecorder (Day 22)",
);
test.use({
  video: { mode: 'on', size: { width: 1280, height: 720 } },
  viewport: { width: 1280, height: 720 },
});

const MAILPIT_API = 'http://localhost:8025/api/v1';
const RECORD_MS = 15_000;
const OUTPUT = join(__dirname, '..', 'demo-output', 'm5');
const ROOT = join(__dirname, '..', '..');

function psql(sql: string): string {
  const url = process.env['DATABASE_URL'];
  if (!url) {
    throw new Error('DATABASE_URL is not set (run through `just demo-m5`)');
  }
  return execFileSync('psql', [url, '-AtF', '|', '-c', sql], { encoding: 'utf8' }).trim();
}

/** Copies a stored object out of MinIO (the dev container's `mc`) into `path`. */
function downloadObject(key: string, path: string): void {
  const bytes = execFileSync(
    'docker',
    [
      'compose',
      'exec',
      '-T',
      'minio',
      'sh',
      '-c',
      `mc alias set l http://localhost:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null && mc cat l/sintade-dev/${key}`,
    ],
    { cwd: ROOT, maxBuffer: 512 * 1024 * 1024 },
  );
  writeFileSync(path, bytes);
}

test('record → stop → MP4 ready → email', async ({ page, request, browserName }) => {
  test.setTimeout(5 * 60_000);
  const email = await signUpAndLogIn(page, request);
  await startRecording(page, { mic: true });
  await page.waitForTimeout(RECORD_MS);
  const stoppedAt = Date.now();
  await page.getByTestId('recorder-stop').click();
  const take = await finishedTake(page);
  const recordingId = await page
    .getByTestId('recorder-done-summary')
    .getAttribute('data-recording-id');
  expect(recordingId).toBeTruthy();

  // The worker processes it: recording → ready, with an MP4 and a poster.
  await expect
    .poll(() => psql(`SELECT state FROM recordings WHERE id = '${recordingId}'`), {
      timeout: 120_000,
      intervals: [1_000],
      message: 'is `just worker` running?',
    })
    .toBe('ready');
  const readyAfterStopMs = Date.now() - stoppedAt;
  const renditions = psql(
    `SELECT kind, variant, size_bytes FROM renditions WHERE recording_id = '${recordingId}' ORDER BY kind`,
  ).split('\n');
  expect(renditions.map((row) => row.split('|').slice(0, 2).join('/'))).toEqual([
    'mp4/default',
    'thumbnail/poster',
  ]);

  // The stored MP4 is what ffprobe says it is: H.264, about as long as the recording.
  const mp4Key = psql(
    `SELECT storage_key FROM renditions WHERE recording_id = '${recordingId}' AND kind = 'mp4'`,
  );
  const dir = join(OUTPUT, browserName);
  await mkdir(dir, { recursive: true });
  const mp4Path = join(dir, 'default.mp4');
  downloadObject(mp4Key, mp4Path);
  const probe = JSON.parse(
    execFileSync(
      'ffprobe',
      ['-v', 'error', '-print_format', 'json', '-show_format', '-show_streams', mp4Path],
      { encoding: 'utf8' },
    ),
  ) as { format: { duration: string }; streams: { codec_type: string; codec_name: string }[] };
  const video = probe.streams.find((stream) => stream.codec_type === 'video');
  const audio = probe.streams.find((stream) => stream.codec_type === 'audio');
  const mp4Seconds = Number(probe.format.duration);
  expect(video?.codec_name).toBe('h264');
  expect(audio?.codec_name).toBe('aac');
  expect(Math.abs(mp4Seconds * 1000 - take.durationMs)).toBeLessThan(2_000);

  // And the creator gets the email, with a link to the recording.
  let message: { ID: string; Subject: string } | undefined;
  await expect
    .poll(
      async () => {
        const found = await request.get(`${MAILPIT_API}/search`, {
          params: { query: `to:${email} subject:"ready"` },
        });
        const body = (await found.json()) as { messages?: { ID: string; Subject: string }[] };
        message = body.messages?.[0];
        return message?.Subject ?? '';
      },
      { timeout: 60_000, intervals: [1_000] },
    )
    .toContain('Your recording is ready');
  const text = (await (await request.get(`${MAILPIT_API}/message/${message?.ID}`)).json()) as {
    Text: string;
  };
  expect(text.Text).toContain(`/recordings/${recordingId}`);
  const emailAfterStopMs = Date.now() - stoppedAt;

  const result = {
    browser: browserName,
    recordingId,
    recordedMs: take.durationMs,
    chunks: take.chunkCount,
    stopToFinalizeMs: take.stopToFinalizeMs,
    readyAfterStopMs,
    emailAfterStopMs,
    mp4: { video: video?.codec_name, audio: audio?.codec_name, seconds: mp4Seconds },
    renditions: renditions,
    emailSubject: message?.Subject,
  };
  test.info().annotations.push({ type: 'M5 demo', description: JSON.stringify(result) });
  await writeFile(join(dir, 'result.json'), `${JSON.stringify(result, null, 2)}\n`);
  const film = page.video();
  await page.close();
  await film?.saveAs(join(dir, 'recording.webm'));
});
