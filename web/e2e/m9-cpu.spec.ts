import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { cpus } from 'node:os';

import type { Browser } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';
import { keepTakesOnDevice } from './support/offline';
import { expect, test } from './support/test';

/**
 * Day 78 profile: client CPU while recording (docs/design.md §11: "Client CPU during 1080p30
 * recording, no camera <= 25% on a 4-core laptop", i.e. one core's worth). Chromium only; run with
 * `just profile-capture`.
 *
 * The "screen" is an animated 1920x1080 canvas at the scenario's frame rate and the camera is
 * Chromium's fake device. CPU is the user+system time of every process of the test browser
 * (renderer, GPU, utility, network), read from /proc in 8 s windows, in cores (CPU seconds
 * per wall second); 25 % of a 4-core laptop is 1.0 core. Each scenario is also run with the same
 * sources but *no recording* (the harness: drawing the fake screen, running the fake camera), and
 * the report subtracts it, because that work is the test's, not the product's. A laptop will
 * differ (hardware encoders, a real capture pipeline): `TODO: Verify` on hardware.
 */
/** Three windows per measurement; the lowest is reported, since a busy machine only adds CPU time. */
const WINDOWS = 3;
const WINDOW_MS = 8_000;
const WARMUP_MS = 4_000;
/** §11: 25 % of a 4-core laptop at 1080p30 = one core. */
const TARGET_CORES = 1.0;
/**
 * 1080p60 has twice the pixel rate and §11 states no target for it. This guardrail (two cores,
 * 50 % of four) is an engineering budget, not a spec figure: a judgment call for the owner to
 * confirm (docs/demos/M09-capture-upgrades.md).
 */
const TARGET_CORES_60 = 2.0;

interface Scenario {
  name: string;
  fps: 30 | 60;
  camera: boolean;
}

const SCENARIOS: Scenario[] = [
  { name: '1080p30, no camera (the §11 target)', fps: 30, camera: false },
  { name: '1080p30 with camera', fps: 30, camera: true },
  { name: '1080p60, no camera', fps: 60, camera: false },
  { name: '1080p60 with camera (the M9 demo)', fps: 60, camera: true },
];

/** Total user+system clock ticks of the test browser's processes. */
function browserTicks(): { ticks: number; processes: number } {
  const pids = execFileSync('pgrep', ['-f', 'playwright_chromiumdev_profile'], {
    encoding: 'utf8',
  })
    .split('\n')
    .filter(Boolean);
  let ticks = 0;
  let processes = 0;
  for (const pid of pids) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
      // Fields after the ")" that closes the command name: utime is field 14, stime 15.
      const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
      ticks += Number(fields[11]) + Number(fields[12]);
      processes += 1;
    } catch {
      // The process ended between pgrep and the read.
    }
  }
  return { ticks, processes };
}

const clockTicks = () => Number(execFileSync('getconf', ['CLK_TCK'], { encoding: 'utf8' }));

/** Cores used by the browser, after the warm-up: the lowest of `WINDOWS` windows. */
async function sampleCores(wait: (ms: number) => Promise<void>) {
  await wait(WARMUP_MS);
  let best = Number.POSITIVE_INFINITY;
  let processes = 0;
  let seconds = 0;
  for (let i = 0; i < WINDOWS; i += 1) {
    const before = browserTicks();
    const startedAt = Date.now();
    await wait(WINDOW_MS);
    const after = browserTicks();
    const elapsed = (Date.now() - startedAt) / 1000;
    const cores = (after.ticks - before.ticks) / clockTicks() / elapsed;
    if (cores < best) {
      best = cores;
      seconds = elapsed;
    }
    processes = after.processes;
  }
  return { cores: best, processes, seconds };
}

const fakeScreen = (fps: number) => {
  navigator.mediaDevices.getDisplayMedia = async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 1920;
    canvas.height = 1080;
    const ctx = canvas.getContext('2d');
    let n = 0;
    setInterval(() => {
      if (!ctx) return;
      n += 1;
      ctx.fillStyle = `hsl(${(n * 3) % 360} 40% 30%)`;
      ctx.fillRect(0, 0, 1920, 1080);
      ctx.fillStyle = '#fff';
      ctx.fillRect((n * 17) % 1700, (n * 11) % 900, 200, 120);
    }, 1000 / fps);
    return canvas.captureStream(fps);
  };
};

/** The sources alone: the fake screen is drawn and captured, the camera runs; nothing records. */
async function harnessCores(browser: Browser, scenario: Scenario) {
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  try {
    const page = await context.newPage();
    await page.goto('/login');
    await page.evaluate(
      async ({ fps, camera }) => {
        const screen = await (async () => {
          const canvas = document.createElement('canvas');
          canvas.width = 1920;
          canvas.height = 1080;
          const ctx = canvas.getContext('2d');
          let n = 0;
          setInterval(() => {
            if (!ctx) return;
            n += 1;
            ctx.fillStyle = `hsl(${(n * 3) % 360} 40% 30%)`;
            ctx.fillRect(0, 0, 1920, 1080);
            ctx.fillStyle = '#fff';
            ctx.fillRect((n * 17) % 1700, (n * 11) % 900, 200, 120);
          }, 1000 / fps);
          return canvas.captureStream(fps);
        })();
        // Something has to consume the stream, as the recorder page's preview does.
        const preview = document.createElement('video');
        preview.muted = true;
        preview.srcObject = screen;
        document.body.appendChild(preview);
        await preview.play();
        if (camera) {
          const cam = await navigator.mediaDevices.getUserMedia({ video: true });
          const camPreview = document.createElement('video');
          camPreview.muted = true;
          camPreview.srcObject = cam;
          document.body.appendChild(camPreview);
          await camPreview.play();
        }
      },
      { fps: scenario.fps, camera: scenario.camera },
    );
    return await sampleCores((ms) => page.waitForTimeout(ms));
  } finally {
    await context.close();
  }
}

test('client CPU while recording', async ({ browser, request, browserName }) => {
  test.skip(browserName !== 'chromium', 'CPU is measured on Chromium');
  test.setTimeout(15 * 60_000);
  const results: Record<string, unknown>[] = [];
  let mimeType = '';

  for (const scenario of SCENARIOS) {
    const harness = await harnessCores(browser, scenario);

    // A fresh browser context per scenario: no state carried over.
    const context = await browser.newContext({ ignoreHTTPSErrors: true });
    const page = await context.newPage();
    await page.addInitScript(fakeScreen, scenario.fps);
    await signUpAndLogIn(page, request);
    await keepTakesOnDevice(page);
    await page.goto('/record');
    await page.getByTestId('recorder-fps').selectOption(String(scenario.fps));
    await page.getByTestId('recorder-choose-screen').click();
    await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
    if (scenario.camera) {
      await page.getByTestId('recorder-camera').check();
      await expect(page.getByTestId('recorder-camera-preview')).toBeVisible();
    }
    await page.getByTestId('recorder-start').click();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
    const recording = await sampleCores((ms) => page.waitForTimeout(ms));
    await page.getByTestId('recorder-stop').click();
    const summary = page.getByTestId('recorder-done-summary');
    await expect(summary).toBeVisible({ timeout: 30_000 });
    mimeType = await page.evaluate(
      async (id) => {
        const root = await navigator.storage.getDirectory();
        const take = await (
          await root.getDirectoryHandle('sintade-chunks')
        ).getDirectoryHandle(id ?? '');
        const file = await (await take.getFileHandle('take.json')).getFile();
        return (JSON.parse(await file.text()) as { mimeType: string }).mimeType;
      },
      await summary.getAttribute('data-take-id'),
    );
    await context.close();

    const net = Math.max(0, recording.cores - harness.cores);
    results.push({
      scenario: scenario.name,
      mimeType,
      totalCores: Number(recording.cores.toFixed(2)),
      harnessCores: Number(harness.cores.toFixed(2)),
      netCores: Number(net.toFixed(2)),
      netPercentOfFourCores: Math.round((net / 4) * 100),
      seconds: Number(recording.seconds.toFixed(1)),
      limitCores: scenario.fps === 60 ? TARGET_CORES_60 : TARGET_CORES,
    });
  }

  mkdirSync('demo-output/m9', { recursive: true });
  const report = {
    hostCores: cpus().length,
    targetCores: TARGET_CORES,
    targetCores60: TARGET_CORES_60,
    results,
  };
  writeFileSync('demo-output/m9/cpu.json', JSON.stringify(report, null, 2));
  test.info().annotations.push({ type: 'cpu', description: JSON.stringify(results) });
  console.log(`CPU REPORT ${JSON.stringify(report)}`);
  for (const result of results) {
    expect(result.netCores as number, String(result.scenario)).toBeLessThanOrEqual(
      result.limitCores as number,
    );
  }
});
