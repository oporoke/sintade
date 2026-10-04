# M4 — Ingest demo (Days 33–42)

Date: 2026-10-04
Branch: `feat/day-042-loss-e2e` (on top of `main` @ `9c5de47`, Day 41)
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` binary, `ng serve` over
HTTPS with the same-origin storage proxy from ADR-0010). There is still no staging VPS (M1
carry-over), so this demo runs locally, as M2's and M3's did.

## Scope

`docs/design.md` §12, weeks 7–8: "Recording/take/session creation, presign, ack, status,
finalize; streaming uploader with retry."

The demo's headline, from the plan: **a 10-minute recording, network cut for 60 s, nothing
lost.**

Built in this milestone:

| Day | What |
| --- | --- |
| 33 | `catalog` + `ingest` crates; `recordings`/`takes`/`chunks`; `POST /recordings` |
| 34 | Chunk presign (single + `?count=` batch), owner only |
| 35 | Chunk ack: size against storage, SHA-256, idempotent, `409` on a different hash |
| 36 | Take status + finalize (`422` with the missing indexes, recording → `processing`, `TakeFinalized`) |
| 37 | Browser `Uploader` (SubtleCrypto SHA-256, presign → PUT → ack); same-origin storage proxy (ADR-0010) |
| 38 | Retry with jittered backoff, offline pause, resume from status |
| 39 | Upload while recording; Stop drains, finalizes and clears the device copy |
| 40 | Recovery upload (what the server lacks, or a new recording for an offline take) |
| 41 | Free tier: 50 recordings, 10-minute takes (`402`); `SweepStaleUploads` (ADR-0011) |
| 42 | Loss-scenario e2e tests; this demo |

## How to run it yourself

```bash
just deps-up && just db-migrate
just api                      # terminal 1 -> :8080
just worker                   # terminal 2 (verification email; the hourly stale-upload sweep)
just web                      # terminal 3 -> https://localhost:4200 (accept the self-signed cert once)
```

### A. By hand (any desktop browser with a real screen and mic)

1. Sign up, verify via Mailpit (`http://localhost:8025`), log in, click **New recording**.
2. Choose a screen and a microphone, **Start recording**.
3. After ~3 minutes, disconnect the network (turn Wi-Fi off, or DevTools → Network →
   **Offline**). **Expect:** recording carries on; nothing changes on screen.
4. After 60 s, reconnect. **Expect:** in DevTools → Network, a burst of chunk `PUT`s and `ack`s
   as the backlog catches up within seconds.
5. Keep recording. At 9:59 **expect** the recorder to stop by itself with "Your plan allows
   recordings of up to 10:00, so this one stopped there." and, within a second, "Recording
   uploaded".
6. `GET /api/v1/takes/<take id>/status` (the take id is on the done summary's
   `data-take-id`) **expects** `finalized: true` and `received` = `0..n` with no gaps.

Variants worth trying: press **Stop** while offline (the page shows "Uploading" and waits;
reconnect and it finishes), or kill the tab while offline and reopen `/record` (the recovery
dialog's **Upload** sends what the server lacks).

### B. Automated (filmed)

```bash
just demo-m4                  # Chromium and Firefox, ~12 min each; or: just demo-m4 --project=firefox
```

`web/e2e/m4-demo.spec.ts` (skipped unless `DEMO=1`) does steps 1–6 through the real `/record`
page on the fake-media harness, with the browser offline from 3:00 to 4:00. A probe in the test
hashes every chunk body that reaches storage; at the end, the server's record of every chunk
(size and SHA-256 from `GET /takes/{id}/status`) must equal what left the browser, for every
index the recorder made. The film and `result.json` go to `web/demo-output/m4/<engine>/`
(gitignored).

## Results (2026-10-04)

| Engine | Take | Chunks (recorder / server) | Bytes | Before cut | During 60 s cut | Backlog caught up | Stop → finalized | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Chromium (system Chrome, fake devices) | 9:59.0 (599 004 ms), stopped by the plan limit | 294 / 294, all hashes match | 39.3 MB | 88 chunks | 0 accepted | 5.0 s after reconnect | 588 ms | **Pass** |
| Firefox (Playwright build, fake mic + canvas screen) | 9:59.1 (599 147 ms), stopped by the plan limit | 297 / 297, all hashes match | 25.7 MB | 89 chunks | 0 accepted | 9.0 s after reconnect | 242 ms | **Pass** |
| WebKit (Playwright Linux build) | — | — | — | — | — | — | — | **Not demonstrated**: no `MediaRecorder` in this build (Day 22); carried over to the macOS/Safari check with M3's |

Every `PUT` storage accepted was a distinct chunk (no duplicate uploads): 294 and 297.

The first Chromium run also passed every assertion (294/294 chunks, hashes match, backlog in
11.0 s, stop → finalized in 407 ms) but failed copying its film before Playwright had finished
writing it; the spec now uses `video.saveAs()`, and the table shows the clean re-run.

### Loss scenarios in CI (`web/e2e/upload-loss.spec.ts`, every PR)

| Scenario | Chromium | Firefox |
| --- | --- | --- |
| Offline mid-recording: chunks wait on the device and upload on reconnect | Pass | Pass |
| Stopped while offline: the recorder waits ("offline"), then finalizes on reconnect | Pass | Pass |
| Silent cut (API and storage unreachable, browser still "online"): backoff retries recover | Pass | Pass |
| Cut off, then the tab dies before reconnecting: recovery uploads every chunk | Pass | Pass |
| The browser's "Stop sharing" ends the take and the upload completes | Pass | Pass |

Each one ends with the same check as the demo: finalized, `received` = `0..n`, and every
server size/SHA-256 equal to the bytes the browser sent. Earlier days' resilience tests still
run: `upload-resilience.spec.ts` (throttled, `503`, offline; all three engines on `/debug`),
`recovery-upload.spec.ts` (crash with storage cut, offline-created take),
`recorder-upload.spec.ts` (stop → finalize < 2 s at 10 Mbps).

## Exit criteria and acceptance criteria

§12 weeks 7–8 deliverables: recording/take creation ✅ · presign ✅ · ack ✅ · status ✅ ·
finalize ✅ · streaming uploader with retry ✅ · upload session ➖ (no `upload_sessions` table:
ADR-0009; not needed by any day so far, ADR-0011).

| Story | Criterion | Status |
| --- | --- | --- |
| US-11 | "Stop sharing" stops recording and the upload completes as with Stop | ✅ `upload-loss.spec.ts` |
| US-12 | Every chunk is in OPFS before its upload starts | ✅ by construction: the uploader reads each chunk back from the `ChunkStore` (`store.get`), so an unstored chunk can't be uploaded; e2e tests kill tabs with uploads cut and recover every chunk |
| US-12 | Tab killed mid-recording → dialog offers Upload or Discard with date and duration | ✅ M3 demo (dialog) + Day 40 / Day 42 (Upload) |
| US-12 | Recovered recording plays with no gap > 2 s | ⏳ needs processing and playback (M5–M6) |
| US-20 | Network drops for 60 s → chunks queue locally and upload on reconnect, no loss | ✅ this demo |
| US-20 | Re-sent chunk with the same hash is a no-op; a different hash is `409` | ✅ Day 35 integration tests (`routes::takes`, `ingest` ack tests) |
| US-20 | Stop → link ≤ 5 s p95 for 30 min at 1080p on 10 Mbps | ⏳ stop → finalize is 0.2–0.4 s here (Day 39: ~240 ms at 10 Mbps), but "link" needs share links (M6) and the 30-min/1080p/p95 run needs the reference machine (Day 50) |

## Carry-over

- No staging VPS (M1). §14 rows stay *In progress* until deployed.
- WebKit recording/upload not demonstrated on Linux; macOS/Safari check (M3 carry-over) now
  also covers upload and the network cut.
- Real-device network unplug on Safari (Day 38 deferred).
- Server-side `max_resolution` (ADR-0011) and chunk-hash verification against bytes in the
  worker (Day 35 deferred) arrive with M5's `ProcessTake`.
