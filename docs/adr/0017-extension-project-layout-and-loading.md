# ADR-0017: The extension is its own project, bundled with esbuild, importing only `capture/`

## Status

Accepted — 2026-10-05 (Day 61, M7).

## Context

§2 and §5 call for a Manifest V3 extension for Chrome and Edge that reuses the framework-free
capture package (CLAUDE.md rule 9). The plan's Check for Day 61 is "loads unpacked in Chrome and
Edge". Branded Chrome 137+ no longer honours `--load-extension`, which is what automated tests
usually use.

## Decision

1. **`extension/` is a separate npm project** next to `web/`, not part of the Angular build: its
   outputs are plain scripts in a folder (`extension/dist/`) that the browser loads, with a
   manifest the Angular CLI knows nothing about. Same TypeScript (`strict`, no `any`), ESLint and
   Prettier settings as `web/`.
2. **esbuild** bundles each entry point (service worker, popup, later offscreen document and
   content script) into a single file. New dev dependencies, tooling only: `esbuild`,
   `@types/chrome`, `vitest` (unit tests), `@playwright/test` (loading it in real browsers).
3. **The extension imports only `web/src/app/capture`**, through the `@capture` path alias.
   ESLint forbids importing anything else from `web/src/app` and anything from `@angular/*`.
4. **Permissions stay at the §20 minimum**: `activeTab`, `tabCapture`, `offscreen`, `storage`.
   Host permissions for the Sintade origin are added with the session handoff (Day 62).
5. **Loading in tests.** Edge honours `--load-extension`. Chrome is given the extension over the
   DevTools protocol (`Extensions.loadUnpacked` on a browser-level session, with
   `--enable-unsafe-extension-debugging`), which works in branded Chrome 154. Both run headless in
   Playwright and in CI (both browsers are preinstalled on GitHub's `ubuntu-latest`).
6. Icons are generated at build time (no binary assets in git) until real artwork is supplied for
   the store listing (Day 66).

## Consequences

- `just ext-build` produces a folder anyone can "Load unpacked"; the same folder is zipped for
  the stores.
- A change to `capture/` is type-checked and tested by both projects.
- The popup and service worker use `chrome.*` APIs directly; the Angular app is untouched.
