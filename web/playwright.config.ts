import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  webServer: {
    command: 'npm run start -- --port 4200',
    url: 'http://localhost:4200',
    reuseExistingServer: true,
    timeout: 60_000,
  },
  use: {
    baseURL: 'http://localhost:4200',
  },
  projects: [
    // Uses the system-installed Google Chrome via CDP (no Playwright-managed binary download
    // required). Firefox/WebKit need Playwright's own patched builds, which this sandbox's
    // network could not download (see docs/plan/PROGRESS.md) -- kept here, commented out, for
    // an environment that can fetch them.
    { name: 'chromium', use: { ...devices['Desktop Chrome'], channel: 'chrome' } },
    // { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    // { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
