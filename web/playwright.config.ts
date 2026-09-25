import { defineConfig, devices } from '@playwright/test';

// Locally, this sandbox's network can't download Playwright's own Chromium build (see
// docs/plan/PROGRESS.md), so local runs fall back to the system-installed Google Chrome via
// CDP. CI installs Playwright's real browsers (`playwright install --with-deps`), so it uses
// Playwright's own Chromium there instead.
const chromiumChannel = process.env['CI'] ? undefined : 'chrome';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  webServer: [
    {
      // `ng serve` runs over HTTPS with a self-signed cert and proxies /api to :8080 (ADR-0006):
      // WebKit on Linux drops the API's `Secure` cookies over plain http://localhost.
      command: 'npm run start -- --port 4200',
      url: 'https://localhost:4200',
      ignoreHTTPSErrors: true,
      reuseExistingServer: true,
      timeout: 60_000,
    },
    // Day 17's signup-to-login e2e test needs the real API (and a running `worker` -- started
    // as a separate CI step, since it has no HTTP endpoint for Playwright to poll for
    // readiness). Requires `just deps-up` + `just db-migrate` already done, same as `just test`.
    {
      command: 'cd .. && cargo run -p api',
      url: 'http://localhost:8080/healthz',
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
  use: {
    baseURL: 'https://localhost:4200',
    ignoreHTTPSErrors: true,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], channel: chromiumChannel } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
