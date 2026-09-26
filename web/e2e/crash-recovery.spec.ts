import { Page, expect, test } from '@playwright/test';

/**
 * Day 26 Check: "Kill tab mid-recording, reopen, dialog shows correct duration". A tab records
 * (journaled to the chunk store); once ≥ 4 s are persisted the tab is killed; a new tab must
 * offer the take with the recorded length.
 *
 * "Killed" = closed abruptly, with no beforeunload/cleanup from the app. (A CDP renderer crash
 * would be more literal, but in headless Chromium it takes the whole context's shared renderer
 * down, and the reopened tab must share that context's storage.) Recovery never relies on
 * unload handlers, only on what was already persisted, so the two are equivalent here.
 */

/** Starts the persisted recording; returns false when this engine can't record (WebKit only). */
async function startRecording(page: Page, browserName: string): Promise<boolean> {
  await page.goto('/debug');
  await page.getByTestId('crash-start').click();
  const status = page.getByTestId('crash-status');
  const error = page.getByTestId('crash-error');
  await expect(status.or(error)).toBeVisible({ timeout: 15_000 });
  if (await error.isVisible()) {
    await expect(error).toHaveText('MediaRecorder is not available in this browser');
    expect(browserName, 'only the Linux WebKit build may lack MediaRecorder').toBe('webkit');
    return false;
  }
  return true;
}

async function persistedMs(page: Page): Promise<number> {
  return Number(await page.getByTestId('crash-status').getAttribute('data-persisted-ms'));
}

test('killing a tab mid-recording leaves a recoverable take with the right length', async ({
  context,
  browserName,
}) => {
  const recording = await context.newPage();
  if (!(await startRecording(recording, browserName))) {
    return;
  }
  const takeId = await recording.getByTestId('crash-status').getAttribute('data-take-id');
  await expect.poll(() => persistedMs(recording), { timeout: 20_000 }).toBeGreaterThanOrEqual(4000);
  const persistedBeforeKill = await persistedMs(recording);

  await recording.close({ runBeforeUnload: false });

  const reopened = await context.newPage();
  await reopened.goto('/debug');
  const dialog = reopened.getByTestId('recovery-dialog');
  await expect(dialog).toBeVisible();
  const take = dialog.locator(`[data-take-id="${takeId}"]`);
  await expect(take).toHaveCount(1);

  // The journal holds the length up to the last persisted chunk: what was on disk at the kill
  // (or one more 2 s slice if it landed in between).
  const durationMs = Number(await take.getAttribute('data-duration-ms'));
  expect(durationMs).toBeGreaterThanOrEqual(persistedBeforeKill);
  expect(durationMs).toBeLessThanOrEqual(persistedBeforeKill + 2_100);
  await expect(take.getByTestId('recovery-summary')).toHaveText(
    new RegExp(`^Unfinished recording from .+, ${Math.round(durationMs / 1000)} s$`),
  );
  await reopened.screenshot({
    path: test.info().outputPath('recovery-dialog.png'),
    fullPage: true,
  });

  // Discard removes it for good.
  await take.getByTestId('recovery-discard').click();
  await expect(take).toHaveCount(0);
  await reopened.reload();
  await expect(
    reopened.getByTestId('recovery-dialog').locator(`[data-take-id="${takeId}"]`),
  ).toHaveCount(0);
});

test('a take still recording in another tab is not offered for recovery', async ({
  context,
  browserName,
}) => {
  const recording = await context.newPage();
  if (!(await startRecording(recording, browserName))) {
    return;
  }
  const takeId = await recording.getByTestId('crash-status').getAttribute('data-take-id');
  await expect.poll(() => persistedMs(recording), { timeout: 20_000 }).toBeGreaterThan(0);

  const other = await context.newPage();
  await other.goto('/debug');
  await expect(other.getByTestId('store-auto')).not.toHaveText(/…$/); // app finished loading
  await other.waitForTimeout(500);
  await expect(other.locator(`[data-take-id="${takeId}"]`)).toHaveCount(0);
  await recording.close();
});
