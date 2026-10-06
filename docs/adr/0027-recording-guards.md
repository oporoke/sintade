# ADR-0027: Wake Lock, storage headroom and the unload guard

## Status

Accepted — 2026-10-05 (Day 77, M9).

## Decision

1. **Wake Lock** (`capture/guards.ts`, `WakeLockGuard`): a `screen` lock is requested when a take
   starts and released when it ends, is discarded or fails. The browser drops it whenever the tab
   is hidden — the normal state while another window is recorded — so it is requested again on
   `visibilitychange`. A refusal or a missing API is not fatal (the state says `off` /
   `unsupported`); recording never waits for it.
2. **Storage headroom** (`checkStorage`, `storageStatus`): `navigator.storage.estimate()` is read
   when the page opens, when a take starts, and every 15 s during it. The page warns when **80 %**
   of the quota is already used or the take (bitrate × the plan's longest take) would take it
   past 80 %, with the free megabytes. It warns and never blocks: estimates are coarse and
   differ by browser (incognito quotas are small), and a blocked recording is worse than a warned
   one. Unknown estimates show nothing.
3. **`beforeunload`** asks the browser to confirm leaving during the countdown, recording, saving
   and uploading. Chunks are already in OPFS and recovery offers them next time (Day 31/40), so
   this protects the *live* part and the upload, not the stored data.

## Consequences

Wake Lock needs a secure context and (Safari) a recent version; where it is missing the laptop can
still sleep, which the recorder cannot detect — `TODO: Verify` on Safari. Browsers show their own
generic text for the unload prompt.
