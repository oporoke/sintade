import { defineConfig } from '@playwright/test';

// Extensions load only in a persistent context (e2e/fixtures.ts). Chrome and Edge are the two
// browsers the extension targets (docs/design.md §2); both are installed on CI runners.
export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  // Shared CI runners are slower and noisier than a workstation: one retry for timing flakes.
  retries: process.env['CI'] ? 1 : 0,
  timeout: 60_000,
  reporter: process.env['CI'] ? [['github'], ['list']] : 'list',
  // The handoff and recording specs need the real app: the HTTPS dev server (proxying /api) and
  // the API, as in web/playwright.config.ts. Requires `just deps-up` + `just db-migrate`.
  webServer: [
    {
      command: 'npm run start -- --port 4200',
      cwd: '../web',
      url: 'https://localhost:4200',
      ignoreHTTPSErrors: true,
      reuseExistingServer: true,
      timeout: 60_000,
    },
    {
      command: 'cargo run -p api',
      cwd: '..',
      url: 'http://localhost:8080/healthz',
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
  projects: [
    { name: 'chrome', use: { channel: 'chrome' } },
    { name: 'edge', use: { channel: 'msedge' } },
  ],
});
