import { createHash } from 'node:crypto';

import { BrowserContext, Page } from '@playwright/test';

import { expect } from './test';

/** What the browser delivered to storage for one chunk (the last PUT storage accepted). */
export interface SentChunk {
  sizeBytes: number;
  sha256: string;
}

/**
 * Watches every chunk PUT the context's pages make to storage and keeps the size and SHA-256
 * of the bytes storage accepted, by chunk index. That's the ground truth "nothing lost" is
 * checked against: the server's record of each chunk must match what actually left the
 * browser. A `cut` makes storage and the API unreachable without telling the browser it's
 * offline (a dead Wi-Fi uplink, a captive portal); `offline` is the browser's offline mode.
 */
export class NetworkProbe {
  readonly sent = new Map<number, SentChunk>();
  /** Chunk PUTs storage accepted, retries included. */
  accepted = 0;
  private silentCut = false;

  private constructor(private readonly context: BrowserContext) {}

  static async attach(context: BrowserContext): Promise<NetworkProbe> {
    const probe = new NetworkProbe(context);
    await context.route('**/sintade-dev/**', async (route) => {
      const request = route.request();
      if (probe.silentCut) {
        return route.abort('internetdisconnected');
      }
      const idx = chunkIndex(request.url());
      if (request.method() !== 'PUT' || idx === null) {
        return route.continue();
      }
      const body = request.postDataBuffer() ?? Buffer.alloc(0);
      const response = await route.fetch().catch(() => null);
      if (!response) {
        return route.abort('connectionfailed');
      }
      if (response.ok()) {
        probe.accepted += 1;
        probe.sent.set(idx, {
          sizeBytes: body.length,
          sha256: createHash('sha256').update(body).digest('hex'),
        });
      }
      return route.fulfill({ response });
    });
    await context.route('**/api/v1/**', (route) =>
      probe.silentCut ? route.abort('internetdisconnected') : route.continue(),
    );
    return probe;
  }

  /** The browser goes offline (`navigator.onLine` false, `offline` event). */
  async goOffline(): Promise<void> {
    await this.context.setOffline(true);
  }

  async goOnline(): Promise<void> {
    await this.context.setOffline(false);
  }

  /** Every request to the API and storage fails, while the browser still thinks it's online. */
  cutSilently(): void {
    this.silentCut = true;
  }

  restore(): void {
    this.silentCut = false;
  }
}

/** `…/chunks/000012.webm?X-Amz-…` → 12. */
function chunkIndex(url: string): number | null {
  const match = /\/chunks\/(\d+)\.[a-z0-9]+(?:\?|$)/.exec(url);
  return match ? Number(match[1]) : null;
}

interface StatusBody {
  finalized: boolean;
  received: number[];
  chunks: { idx: number; size_bytes: number; sha256: string }[];
}

/**
 * The take is finalized with every chunk the recorder made, `0..chunkCount`, and each one the
 * server recorded is byte-for-byte what the browser sent (size and SHA-256).
 */
export async function expectNothingLost(
  page: Page,
  probe: NetworkProbe,
  takeId: string | null,
  chunkCount: number,
): Promise<StatusBody> {
  expect(takeId, 'the take has a server id').toBeTruthy();
  expect(chunkCount, 'the recorder made chunks').toBeGreaterThan(0);
  const response = await page.request.get(`/api/v1/takes/${takeId}/status`);
  expect(response.ok()).toBe(true);
  const status = (await response.json()) as StatusBody;
  const all = Array.from({ length: chunkCount }, (_, idx) => idx);
  expect(status.finalized).toBe(true);
  expect(status.received).toEqual(all);
  for (const chunk of status.chunks) {
    const sent = probe.sent.get(chunk.idx);
    expect(sent, `chunk ${chunk.idx} went through storage`).toBeDefined();
    expect({ idx: chunk.idx, size: chunk.size_bytes, sha256: chunk.sha256 }).toEqual({
      idx: chunk.idx,
      size: sent?.sizeBytes,
      sha256: sent?.sha256,
    });
  }
  return status;
}

/** Chooses the screen (and, if asked, the first microphone) on `/record` and starts. */
export async function startRecording(
  page: Page,
  options: { mic?: boolean; countdown?: boolean } = {},
): Promise<void> {
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  if (options.mic) {
    await page.getByTestId('recorder-mic-select').selectOption({ index: 1 });
    await expect(page.getByTestId('recorder-mic-info')).toContainText('working');
  }
  await page.getByTestId('recorder-start').click();
  if (!options.countdown) {
    await page.keyboard.press('Escape'); // skip the countdown
  }
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording', { timeout: 10_000 });
}

/** Waits for the done screen and returns what the recorder reports about the take. */
export async function finishedTake(page: Page, timeout = 30_000) {
  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout });
  await expect(page.getByTestId('recorder-upload-notice')).toHaveCount(0);
  await expect(summary).toHaveAttribute('data-uploaded', 'true');
  return {
    takeId: await summary.getAttribute('data-take-id'),
    chunkCount: Number(await summary.getAttribute('data-chunk-count')),
    durationMs: Number(await summary.getAttribute('data-duration-ms')),
    stopToFinalizeMs: Number(await summary.getAttribute('data-stop-to-finalize-ms')),
  };
}
