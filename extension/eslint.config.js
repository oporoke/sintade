// @ts-check
import eslint from '@eslint/js';
import { defineConfig } from 'eslint/config';
import tseslint from 'typescript-eslint';

export default defineConfig([
  {
    ignores: ['dist/**', 'node_modules/**', 'test-results/**', 'playwright-report/**'],
  },
  {
    files: ['**/*.ts', '**/*.mjs'],
    extends: [eslint.configs.recommended, tseslint.configs.recommended, tseslint.configs.stylistic],
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
    },
  },
  {
    files: ['scripts/**/*.mjs', 'e2e/**/*.ts', '*.ts'],
    languageOptions: {
      globals: { Buffer: 'readonly', process: 'readonly', console: 'readonly' },
    },
  },
  {
    // The extension reuses the framework-free capture engine unchanged (CLAUDE.md rule 9) and
    // nothing else from the Angular app.
    files: ['src/**/*.ts'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              regex: '^(\\.\\./)+web/src/app/(?!capture(/|$))',
              message: 'The extension may import only web/src/app/capture (use "@capture").',
            },
            { regex: '^@angular/', message: 'No Angular in the extension.' },
          ],
        },
      ],
    },
  },
]);
