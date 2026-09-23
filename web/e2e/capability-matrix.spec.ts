import { expect, test } from '@playwright/test';

const CAPABILITIES = ['getDisplayMedia', 'mediaRecorderWebm', 'opfs', 'systemAudio'] as const;

test('debug page renders a capability matrix reflecting this engine', async ({ page }, testInfo) => {
  await page.goto('/debug');
  await expect(page.getByTestId('capability-matrix')).toBeVisible();

  const values: Record<string, string> = {};
  for (const capability of CAPABILITIES) {
    const cell = page.getByTestId(`capability-value-${capability}`);
    await expect(cell).toBeVisible();
    values[capability] = (await cell.textContent())?.trim() ?? '';
    expect(values[capability]).toMatch(/^(true|false)$/);
  }

  // getDisplayMedia is exposed as an API on every modern engine Playwright drives, regardless
  // of whether a real display picker would succeed under automation.
  expect(values['getDisplayMedia']).toBe('true');

  // systemAudio is a UA-based heuristic (see capability.service.ts): true only on the
  // Chromium-family engine.
  expect(values['systemAudio']).toBe(testInfo.project.name === 'chromium' ? 'true' : 'false');
});
