import { mkdirSync, writeFileSync } from 'node:fs';

import { signUpAndLogIn } from './support/auth';
import { recordAndShare } from './support/share';
import { expect, test } from './support/test';

/**
 * Day 87: makes the one recording `just load-playback` points its viewers at. Records a few
 * seconds, shares it by link, and waits until the playback grant offers the HLS ladder and the
 * scrub sprite (needs `just worker` running). Writes the slug to target/load/slug.
 * Run only through `just load-prepare` (LOAD=1).
 */
test('prepare the recording the playback load test watches', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(!process.env['LOAD'], 'only run by `just load-prepare`');
  test.skip(browserName !== 'chromium', 'one recording is enough');
  test.setTimeout(240_000);
  await signUpAndLogIn(page, request);
  const shared = await recordAndShare(page, 8, 'link');

  await expect(async () => {
    const response = await request.get(`/api/v1/s/${shared.slug}/playback`);
    expect(response.status()).toBe(200);
    const grant = (await response.json()) as { hls_url?: string; sprite_url?: string };
    if (!grant.hls_url || !grant.sprite_url) {
      // The first view of the MP4 queues the ladder and the sprite follows it.
      await page.waitForTimeout(2_000);
    }
    expect(grant.hls_url).toBeTruthy();
    expect(grant.sprite_url).toBeTruthy();
  }).toPass({ timeout: 200_000, intervals: [2_000] });

  mkdirSync('../target/load', { recursive: true });
  writeFileSync('../target/load/slug', shared.slug);
});
