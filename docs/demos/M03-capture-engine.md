# M3 — Capture engine demo (Days 21–32)

Date: 2026-09-28
Branch: `feat/day-032-m3-demo` (on top of `main` @ `95642a3`, Day 31)
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` + `worker` binaries,
`ng serve` over HTTPS). There is still no staging VPS (M1 carry-over), so this demo runs
locally, as M2's did.

## Scope

`docs/design.md` §12, weeks 5–7: "Capability detection, sources, mixer, `ChunkRecorder`, OPFS
`ChunkStore`, controls, countdown, recovery dialog; capture kept as a standalone package."

The demo's headline, from the plan: **a 2-minute local recording on three browsers; crash and
recover.** Recordings stay on the device in this milestone. Upload arrives in M4 (Days 33–39).

## How to run it yourself

```bash
just deps-up && just db-migrate
just api                      # terminal 1 -> :8080
just worker                   # terminal 2 (verification email via Mailpit)
just web                      # terminal 3 -> https://localhost:4200 (accept the self-signed cert once)
```

### A. By hand (any desktop browser with a real screen and mic)

1. Sign up, verify via Mailpit (`http://localhost:8025`), log in, click **New recording**.
2. **Choose screen, window or tab** and pick something to share. **Expect:** a live preview.
3. Pick a microphone. **Expect:** "working" and a moving level meter. On Firefox and Safari the
   **Include system audio** box is disabled, with a one-line reason.
4. **Start recording.** **Expect:** 3-2-1 countdown (`Esc` skips it), then `Recording` and a
   running timer.
5. Record 1 minute, **Pause** for a few seconds, **Resume**, record 1 more minute, **Stop**.
   **Expect:** "2:00 saved on this device" (the pause is not counted) and **Save a copy**
   downloads a file that plays.
6. Start another recording, wait ~10 s, then kill the tab (close it, or end the browser
   process). Reopen `https://localhost:4200/record`. **Expect:** the dialog "Unfinished
   recording from <time>, <n> s" with **Save a copy** and **Discard** (**Upload** disabled
   until the uploader exists).

### B. Automated (filmed)

```bash
just demo-m3                  # all three engines; or: just demo-m3 --project=webkit
```

`web/e2e/m3-demo.spec.ts` (skipped unless `DEMO=1`) does steps 1–6 through the real `/record`
page on the Day 31 fake-media harness and films each page. The videos and the downloaded take
go to `web/demo-output/<engine>/` (gitignored). `web/demo-output/index.html` plays them side by
side.

## Results (2026-09-28)

| Engine | 2-minute recording with a 5 s pause | Crash at ~10 s, reopen, recover |
| --- | --- | --- |
| Chromium (system Chrome, fake devices) | **Pass.** Take duration asserted within 118–125 s, so the pause was excluded. Downloaded `take-2min.webm` is 7.8 MB | **Pass.** Recovery dialog "Unfinished recording from …, n s" with n in 6–12; **Discard** clears it |
| Firefox (Playwright build, fake mic + canvas screen) | **Pass.** Same assertions; `take-2min.webm` is 5.8 MB | **Pass.** Same assertions |
| WebKit (Playwright Linux build) | **Not demonstrated.** This build has no `MediaRecorder` (known since Day 22), so the recorder shows "This browser can't record". The test asserts that, and only WebKit may take that path | **Not demonstrated**, for the same reason |

`just demo-m3`: Chromium + Firefox 4 passed (5.9 min); WebKit 2 passed (can't-record path).

### Can WebKit on Linux be made to record? (investigated, no)

- `libwebkitgtk-6.0.so` contains `MediaRecorderEnabled`, so the code is compiled in, but it is
  switched off.
- Playwright's `Page.overrideSetting` doesn't include that setting.
- The launched MiniBrowser accepts `--features=…`, but MediaRecorder isn't one of its 577
  toggleable features. `MediaRecorder`, `MediaRecorderEnabled` and `MediaRecorderEnabledWebM`
  all left `typeof MediaRecorder === 'undefined'`.
- Faking `MediaRecorder` would fake the thing under test, so that was ruled out.

Real Safari and Playwright's macOS WebKit have `MediaRecorder`, so the WebKit recording demo
moves to macOS (carry-over below).

### Harness fix found while running the demo

Once WebKit could launch locally (the user installed its host libraries on 2026-09-28), every
WebKit capture test failed on "Screen sharing was cancelled or blocked". The Day 31 harness
replaced `navigator.mediaDevices.getDisplayMedia` on the instance an init script sees, but WebKit
gives the page a different `MediaDevices` object, so the real picker ran and refused. It failed
the same way with the committed harness, so it wasn't a regression. The harness now patches
`MediaDevices.prototype`, which works in all three engines. The fake-media context init script
also moved from page to context, so tabs a test opens itself get it (the crash demo needs this).

### Quality gates

- `just check`: green. `just test`: green (Rust suites + 167 Angular unit tests).
- e2e per engine (run separately; see Known issues): Chromium 24 passed, 2 skipped; Firefox 24
  passed, 2 skipped; WebKit 23 passed, 3 skipped. `signup-to-login` first hit the local
  3-signups/hour limit after today's demo runs. Only local `signup:%` rows in
  `rate_limit_buckets` were cleared, then it passed on Firefox and WebKit.

## Acceptance criteria (§15) covered by M3

| Criterion | Result |
| --- | --- |
| US-10 Mic list populated after permission; last choice remembered | Pass (Days 21, 27, 31; Chromium + Firefox e2e) |
| US-10 System audio disabled with a one-line reason where unsupported | Pass (Day 27 e2e, all three engines) |
| US-10 Denied permissions show per-browser recovery, not a blank screen | Pass (Day 30 e2e, all three engines) |
| US-11 3-2-1 countdown, skippable with `Esc` | Pass (Day 28 e2e, all three engines) |
| US-11 Timer excludes paused time | Pass (Days 24, 29; this demo's 2-minute take) |
| US-11 "Stop sharing" stops recording **and upload completes** | Half: recording stops (Day 24); upload is M4 |
| US-12 Every chunk is in OPFS before its upload starts | Half: every chunk is persisted as recorded (Days 25–26, 31); upload is M4 |
| US-12 Tab killed mid-recording → dialog with Upload or Discard, date and duration | Pass for date, duration and Discard (this demo, Chromium + Firefox); Upload is disabled until M4 |
| US-12 Recovered recording plays with no gap > 2 s | Not yet shown end to end: Day 26 checks the recovered duration is within one 2 s slice, but the recovered file's playback is checked in M4 |

§12 exit criterion "Killing the tab at minute 10 and reopening recovers the recording with no
gaps" stays open until the uploader and processing exist.

## Carry-over

- **WebKit recording on macOS:** the manual Safari check (§18), or `just demo-m3 --project=webkit`
  on a Mac: 2-minute recording and crash recovery.
- Staging VPS (from M1): still none; this demo ran locally.
- US-11/US-12 upload halves and the recovered-playback check: M4 (Days 33–39).
