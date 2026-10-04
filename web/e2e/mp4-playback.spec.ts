import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { Page, Route } from '@playwright/test';

import { expect, test } from './support/test';

/**
 * Day 46 Check: "MP4 plays and seeks in every browser". The worker's own MP4 step
 * (`worker transcode`, the code `ProcessTake` runs) turns golden fixtures into fast-start MP4s:
 * Chrome's VP9/Opus (a transcode) and Safari's H.264/AAC (a remux). Each engine then plays one,
 * seeks into the middle, and keeps playing from there. The MP4s are served with HTTP Range
 * support, as object storage and the CDN serve them.
 */
const ROOT = join(__dirname, '..', '..');
const OUT = join(__dirname, '..', 'test-results', 'mp4-playback');

const CASES = [
  { fixture: 'vp9_opus_30s.webm', mp4: 'chrome-transcoded.mp4', seconds: 30 },
  { fixture: 'h264_aac_safari_20s.mp4', mp4: 'safari-remuxed.mp4', seconds: 20 },
  { fixture: 'real_chrome_vp9_opus_10s.webm', mp4: 'real-chrome.mp4', seconds: 10 },
];

test.beforeAll(() => {
  mkdirSync(OUT, { recursive: true });
  for (const { fixture, mp4 } of CASES) {
    execFileSync(
      'cargo',
      [
        'run',
        '-q',
        '-p',
        'worker',
        '--',
        'transcode',
        join('docs', 'fixtures', fixture),
        join(OUT, mp4),
      ],
      { cwd: ROOT, stdio: 'pipe', timeout: 300_000 },
    );
  }
});

/** Serves `bytes` at the route, honouring `Range` like a real object store. */
function serveWithRanges(bytes: Buffer) {
  return (route: Route) => {
    const range = /bytes=(\d*)-(\d*)/.exec(route.request().headers()['range'] ?? '');
    const headers = { 'content-type': 'video/mp4', 'accept-ranges': 'bytes' };
    if (!range) {
      return route.fulfill({ status: 200, headers, body: bytes });
    }
    const start = range[1] ? Number(range[1]) : bytes.length - Number(range[2]);
    const end =
      range[1] && range[2] ? Math.min(Number(range[2]), bytes.length - 1) : bytes.length - 1;
    return route.fulfill({
      status: 206,
      headers: { ...headers, 'content-range': `bytes ${start}-${end}/${bytes.length}` },
      body: bytes.subarray(start, end + 1),
    });
  };
}

async function canPlayH264(page: Page): Promise<boolean> {
  return page.evaluate(
    () =>
      document.createElement('video').canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"') !==
      '',
  );
}

for (const { mp4, seconds } of CASES) {
  test(`${mp4} plays and seeks`, async ({ page, browserName }) => {
    test.setTimeout(60_000);
    await page.route('**/__media/video.mp4', serveWithRanges(readFileSync(join(OUT, mp4))));
    await page.route('**/__media/player.html', (route) =>
      route.fulfill({
        contentType: 'text/html',
        body: '<!doctype html><video id="v" src="/__media/video.mp4" preload="auto" muted playsinline></video>',
      }),
    );
    await page.goto('/__media/player.html');
    // Playwright's open-source Chromium build has no H.264 (proprietary codecs); branded Chrome,
    // Edge, Firefox and Safari do. Say so rather than pass silently.
    test.skip(!(await canPlayH264(page)), `${browserName} build without H.264 decoding`);

    const metadata = await page.evaluate(async () => {
      const video = document.getElementById('v') as HTMLVideoElement;
      if (video.readyState < 1) {
        await new Promise((resolve, reject) => {
          video.addEventListener('loadedmetadata', resolve, { once: true });
          video.addEventListener('error', () => reject(new Error(`error ${video.error?.code}`)), {
            once: true,
          });
        });
      }
      return { duration: video.duration, width: video.videoWidth, height: video.videoHeight };
    });
    expect(Math.abs(metadata.duration - seconds)).toBeLessThan(0.7);
    expect(metadata.width).toBeGreaterThan(0);

    const played = await page.evaluate(async (target) => {
      const video = document.getElementById('v') as HTMLVideoElement;
      await video.play();
      const until = (check: () => boolean, ms: number) =>
        new Promise<boolean>((resolve) => {
          const started = performance.now();
          const tick = () => {
            if (check()) resolve(true);
            else if (performance.now() - started > ms) resolve(false);
            else requestAnimationFrame(tick);
          };
          tick();
        });
      const startedPlaying = await until(() => video.currentTime > 0.3, 10_000);
      const seeked = new Promise((resolve) =>
        video.addEventListener('seeked', resolve, { once: true }),
      );
      video.currentTime = target;
      await seeked;
      const landedAt = video.currentTime;
      const keptPlaying = await until(() => video.currentTime > landedAt + 0.3, 10_000);
      return { startedPlaying, landedAt, keptPlaying, readyState: video.readyState };
    }, seconds * 0.6);

    test
      .info()
      .annotations.push({
        type: 'seek',
        description: `${mp4}: landed at ${played.landedAt.toFixed(2)} s`,
      });
    expect(played.startedPlaying).toBe(true);
    expect(Math.abs(played.landedAt - seconds * 0.6)).toBeLessThan(1.5);
    expect(played.keptPlaying).toBe(true);
    expect(played.readyState).toBeGreaterThanOrEqual(2);
  });
}
