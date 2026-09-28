import { copyFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';

import { Page } from '@playwright/test';

import { signUpAndLogIn } from './support/auth';
import { keepTakesOnDevice } from './support/offline';
import { expect, test } from './support/test';

/**
 * M3 demo (Day 32): "2-minute local recording on three browsers; crash and recover". Long, so
 * it only runs with `DEMO=1` (`just demo-m3`). Playwright films every page, and each take the
 * recorder saved is downloaded next to the films in `web/demo-output/<browser>/`.
 */
test.skip(!process.env['DEMO'], 'M3 demo: run with DEMO=1 (just demo-m3)');
test.use({
  video: { mode: 'on', size: { width: 1280, height: 720 } },
  viewport: { width: 1280, height: 720 },
});
test.describe.configure({ mode: 'serial' });

const RECORD_MS = 120_000;
const PAUSE_MS = 5_000;
const OUTPUT = join(__dirname, '..', 'demo-output');

async function outputDir(browserName: string): Promise<string> {
  const dir = join(OUTPUT, browserName);
  await mkdir(dir, { recursive: true });
  return dir;
}

/** Keeps the page's film once the test has closed it. */
async function keepFilm(page: Page, browserName: string, name: string): Promise<void> {
  const video = page.video();
  await page.close();
  if (video) {
    await copyFile(await video.path(), join(await outputDir(browserName), `${name}.webm`));
  }
}

/** Chooses the screen and first mic, then starts, letting the 3-2-1 countdown run. */
async function startTake(page: Page): Promise<boolean> {
  // M3 shows on-device recording; since Day 39 an uploaded take leaves the device.
  await keepTakesOnDevice(page);
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-mic-select').selectOption({ index: 1 });
  await expect(page.getByTestId('recorder-mic-info')).toContainText('working');
  await page.waitForTimeout(1500); // let the mic meter show on film
  await page.getByTestId('recorder-start').click();
  const status = page.getByTestId('recorder-status');
  const problem = page.getByTestId('recorder-problem-title');
  await expect(status.or(problem)).toBeVisible({ timeout: 10_000 });
  return status.isVisible();
}

test('two-minute recording with a pause', async ({ page, request, browserName }) => {
  test.setTimeout(RECORD_MS + 120_000);
  await signUpAndLogIn(page, request);

  if (!(await startTake(page))) {
    // Playwright's Linux WebKit has no MediaRecorder (Day 22): the film shows the clear problem.
    await expect(page.getByTestId('recorder-problem-title')).toHaveText(
      "This browser can't record",
    );
    await page.waitForTimeout(3000);
    await keepFilm(page, browserName, 'recording');
    expect(browserName, 'only the Linux WebKit build may lack MediaRecorder').toBe('webkit');
    return;
  }

  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(RECORD_MS / 2);
  await page.getByTestId('recorder-pause').click();
  await expect(page.getByTestId('recorder-status')).toHaveText('Paused');
  await page.waitForTimeout(PAUSE_MS);
  await page.getByTestId('recorder-pause').click(); // the same button now reads "Resume"
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(RECORD_MS / 2);
  await page.getByTestId('recorder-stop').click();

  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 15_000 });
  // The pause is not part of the take.
  const durationMs = Number(await summary.getAttribute('data-duration-ms'));
  expect(durationMs).toBeGreaterThan(RECORD_MS - 2_000);
  expect(durationMs).toBeLessThan(RECORD_MS + 5_000);
  test.info().annotations.push({ type: 'take', description: `${durationMs} ms` });

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByTestId('recorder-download').click(),
  ]);
  await download.saveAs(join(await outputDir(browserName), 'take-2min.webm'));
  await page.waitForTimeout(2000);
  await keepFilm(page, browserName, 'recording');
});

test('crash mid-recording, then recover', async ({ context, page, request, browserName }) => {
  test.setTimeout(90_000);
  await signUpAndLogIn(page, request);
  const recording = await context.newPage();
  await page.close();

  if (!(await startTake(recording))) {
    await keepFilm(recording, browserName, 'crash-before');
    expect(browserName, 'only the Linux WebKit build may lack MediaRecorder').toBe('webkit');
    return;
  }
  await expect(recording.getByTestId('recorder-status')).toHaveText('Recording');
  await recording.waitForTimeout(10_000);

  // The "crash": the tab dies with no unload handlers; only what was persisted survives.
  const film = recording.video();
  await recording.close({ runBeforeUnload: false });
  if (film) {
    await copyFile(await film.path(), join(await outputDir(browserName), 'crash-before.webm'));
  }

  const reopened = await context.newPage();
  await reopened.goto('/record');
  const dialog = reopened.getByTestId('recovery-dialog');
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  const summary = dialog.getByTestId('recovery-summary').first();
  await expect(summary).toHaveText(/^Unfinished recording from .+, \d+ s$/);
  const seconds = Number(/(\d+) s$/.exec((await summary.textContent()) ?? '')?.[1]);
  expect(seconds).toBeGreaterThanOrEqual(6);
  expect(seconds).toBeLessThanOrEqual(12);
  test.info().annotations.push({ type: 'recovered', description: `${seconds} s` });
  await reopened.waitForTimeout(3000);

  // Upload arrives with the uploader (Days 37–39); discarding clears it for good.
  await dialog.getByTestId('recovery-discard').first().click();
  await expect(dialog.getByTestId('recovery-summary')).toHaveCount(0);
  await reopened.waitForTimeout(1500);
  await keepFilm(reopened, browserName, 'crash-after');
});
