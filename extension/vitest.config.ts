import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@capture': fileURLToPath(new URL('../web/src/app/capture/index.ts', import.meta.url)),
    },
  },
  test: { include: ['src/**/*.test.ts', 'scripts/**/*.test.ts'] },
});
