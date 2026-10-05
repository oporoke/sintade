import { defineConfig } from '@playwright/test';

// Extensions load only in a persistent context (e2e/fixtures.ts). Chrome and Edge are the two
// browsers the extension targets (docs/design.md §2); both are installed on CI runners.
export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  reporter: process.env['CI'] ? [['github'], ['list']] : 'list',
  projects: [
    { name: 'chrome', use: { channel: 'chrome' } },
    { name: 'edge', use: { channel: 'msedge' } },
  ],
});
