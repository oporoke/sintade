# M5 — Processing demo (Days 43–51)

Date: 2026-10-04
Branch: `feat/day-051-m5-demo` (on top of `main` @ `f0c20ec`, Day 50)
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` and `worker` binaries,
`ng serve` over HTTPS). There is still no staging VPS (M1 carry-over), so this demo runs
locally, as M2–M4's did.

## Scope

`docs/design.md` §12, weeks 9–10: "Worker binary, `ProcessTake` (concat, ffprobe, MP4
fast-start, poster), state machine, email on ready."

The demo's headline, from the plan: **record → stop → MP4 ready → email.**

Built in this milestone:

| Day | What |
| --- | --- |
| 43 | `media` crate, `renditions` + `media_jobs`, `ProcessTake` on `TakeFinalized` (manifest in the event, ADR-0012), scratch dirs, job-lock heartbeat |
| 44 | Chunks streamed, SHA-256-verified and concatenated into `source.webm` |
| 45 | ffprobe validation with creator-facing rejections; golden fixtures in `docs/fixtures/` |
| 46 | FFmpeg runner (progress, stall, deadline); fast-start MP4 (transcode, or remux for Safari H.264/AAC) |
| 47 | Poster, rendition rows, `ready`/`failed` states, `RecordingReady`/`ProcessingFailed` |
| 48 | `messaging` crate: "recording ready" email; `POST /recordings/{id}/retry` |
| 49 | Golden-file pipeline tests over every fixture (duration, codecs, seekability, poster); keyframe every 2 s |
| 50 | Per-kind job slots (`ProcessTake` capped at 1); `superfast` for takes > 10 min; 30-min 1080p in 10:05 on 4 cores (ADR-0013) |
| 51 | This demo |

## How to run it yourself

```bash
just deps-up && just db-migrate
just worker                   # terminal 1 (processes takes, sends the email)
just web                      # terminal 2 -> https://localhost:4200 (accept the self-signed cert once)
just api                      # terminal 3 -> :8080
```

### A. By hand (any desktop browser with a real screen and mic)

1. Sign up, verify via Mailpit (`http://localhost:8025`), log in, click **New recording**.
2. Choose a screen and a microphone, **Start recording**, talk for ~15 s, **Stop**.
   **Expect:** "Recording uploaded. Processing has started." within a second.
3. Watch the worker log. **Expect:** `ProcessTake` runs and finishes within a few seconds
   (about a third of the recording's length).
4. In Mailpit, **expect** an email "Your recording is ready: Untitled recording" with a link
   to `/recordings/<id>`.
5. In `psql`: `SELECT state FROM recordings WHERE id = '<id>'` is `ready`, and `renditions`
   has an `mp4/default` and a `thumbnail/poster` row. The MP4 (`mc cat`) is H.264 + AAC and
   about as long as the recording.

Failure path: `POST /api/v1/recordings/<id>/retry` on a `failed` recording requeues it
(tests: `retry_requeues_a_failed_recording_and_it_becomes_ready`).

### B. Automated (filmed)

```bash
just worker &                 # the spec needs the worker running
just demo-m5                  # Chromium and Firefox, ~40 s each
```

`web/e2e/m5-demo.spec.ts` (skipped unless `DEMO=1`) does steps 1–5 through the real `/record`
page on the fake-media harness: it waits for the recording to reach `ready`, checks the two
rendition rows, copies the stored MP4 out of MinIO and runs `ffprobe` on it, and polls
Mailpit for the email. The film and `result.json` go to `web/demo-output/m5/<engine>/`
(gitignored).

## Results (2026-10-04)

| Engine | Recorded | Chunks | Stop → finalized | Stop → `ready` | Stop → email | Stored MP4 | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Chromium (system Chrome, fake devices) | 15.1 s | 8 | 172 ms | 4.9 s | 6.2 s | H.264 + AAC, 15.1 s, 476 KB; poster 13 KB | **Pass** |
| Firefox (Playwright build, fake mic + canvas screen) | 15.2 s | 8 | 258 ms | 4.4 s | 5.5 s | H.264 + AAC, 15.3 s, 381 KB; poster 11 KB | **Pass** |
| WebKit (Playwright Linux build) | — | — | — | — | — | — | **Not demonstrated**: no `MediaRecorder` in this build (Day 22) |

The first run failed only on the spec's own expectation of the poster's rendition name
(`thumbnail/poster`, not `poster/default`); the recording itself reached `ready`.

### Pipeline checks that run in CI on every PR

| Check | Where | Result |
| --- | --- | --- |
| Every fixture → fast-start H.264 MP4: duration, codecs, size, keyframe spacing, full decode, seeks, poster | `media` `golden_pipeline` (Day 49) | Pass |
| MP4 plays and seeks in Chromium, Firefox, WebKit | `web/e2e/mp4-playback.spec.ts` (Day 46) | Pass |
| `not_a_video.webm` is rejected cleanly | `media` tests (Day 45) | Pass |
| Failed recording is retried and becomes `ready`; foreign workspace gets `404` | `media`, `api` tests, tenant-isolation table (Day 48) | Pass |

### Performance (Day 50, `just perf-long`)

30-minute 1080p30 VP9/Opus, deliberately busy picture, MP4 step pinned to 4 cores
(`taskset -c 0-3`) on this 8-core host: **10:05** wall clock (0.34× the recording), 407 MB
peak memory. `veryfast` alone took 14:58 (0.50×), hence ADR-0013.

## Exit criteria and acceptance criteria

§12 weeks 9–10 deliverables: worker binary ✅ · `ProcessTake` concat ✅ · ffprobe ✅ · MP4
fast-start ✅ · poster ✅ · state machine ✅ (`processing → ready | failed`, manual retry) ·
email on ready ✅.

| Story | Criterion | Status |
| --- | --- | --- |
| US-12 | Recovered recording plays with no gap > 2 s | ✅ for processing: `vp8_opus_paused` and `truncated_last_chunk` fixtures become seekable MP4s (Day 49). Playing it in the watch page waits for M6 |
| US-20 | Stop → link ≤ 5 s p95 for 30 min at 1080p on 10 Mbps | ⏳ stop → finalized is 0.2–0.3 s and a 15 s take is `ready` in ~4.5 s, but "link" needs share links (M6) and the p95 over 30-minute takes needs the reference machine |
| US-21 | Recording plays in any browser | 🟡 the MP4 plays and seeks in Chromium, Firefox, WebKit (Day 46); the watch page is M6 |
| §11 | `ProcessTake` ≤ 0.5× the recording at 1080p on 4 vCPU | ✅ 0.34× (30 min, 4 cores of this host; a real 4-vCPU VPS is not available yet) |

§12 MVP exit criteria touched here: "30-minute 1080p recording … uploads and plays everywhere"
(processing leg ✅, watch page ⏳) and "Stop-to-link ≤ 5 s p95" (⏳ M6).

## Carry-over

- No staging VPS (M1). §14 rows stay *In progress* until deployed; re-run `just perf-long`
  on a real 4-vCPU worker.
- WebKit recording/upload/processing demo on macOS/Safari (M3/M4 carry-over).
- Local `just e2e` still doesn't start the worker; `just demo-m5` needs `just worker`.
- HLS ladder, sprites, loudnorm tuning and SSE status are V1 (§12).
- Long takes use `superfast` (ADR-0013); revisit output size with real long recordings.
