# M8 — Hardening and launch (Days 67–70)

Date: 2026-10-05
Environment: **local only.** There is no staging or production host, domain, mail provider or
Sentry project (M1 carry-over, `docs/runbooks/provision-host.md` §0). Everything below that can be
shown without them is shown; what needs them is listed as open, not claimed.

## What M8 delivered

| Day | What | Proof |
| --- | --- | --- |
| 67 | Rate limit on every route; spoof-proof client address; full header set (API and nginx web image); ZAP, gitleaks, cargo/npm audit clean and blocking in CI | ZAP web baseline 0 FAIL / 0 WARN (4 accepted), API scan 118/118; gitleaks 41 → 0; audits 0; ADR-0022, `docs/security/zap-2026-10-05.md` |
| 68 | Postgres point-in-time recovery, nightly base backups, off-site mirror (versioned), restore drill, runbooks | `restore-drill.sh` passes in 43 s and in CI; ADR n/a, `deploy/backup/`, `docs/runbooks/` |
| 69 | Production stack (Caddy TLS, nginx web, API, worker, Postgres, MinIO), `deploy.sh`, `deploy-production` job behind an approval, `api migrate`, Sentry, `OpsWatchdog` alerts | `just prod-smoke`: the real images through `deploy.sh`, `/readyz` over TLS, a chunk uploaded through Caddy to MinIO and acknowledged; ADR-0023 |
| 70 | Launch rehearsal, exit-criteria review, launch kit | below |

## The launch rehearsal (Day 70)

`just rehearse-launch` runs against the API at its **production rate limits** (`RATE_LIMIT_SCALE=1`),
with the worker running, in Chromium and Firefox (fake screen and microphone devices).

### Kill the tab at minute 9, reopen, get everything back

A recording with a microphone runs for 9:00; the tab is closed with no unload handlers (a crash);
Sintade is reopened, the recovery dialog offers the take, **Upload** sends what the server lacks
and finalizes; the recording is shared, processed, played, and the downloaded MP4 decoded frame by
frame.

| Browser | Recorded before the kill | Chunks | Recovered length | Lost at the end | Longest gap between video frames | Processing after recovery |
| --- | --- | --- | --- | --- | --- | --- |
| Chromium | 9:00.06 | 264 | 8:58.50 | 1.56 s | 0.10 s | 58 s |
| Firefox | 9:00.01 | 267 | 8:59.90 | 0.11 s | 0.07 s | 48 s |

The 1.5 s at the end is the last unflushed 2 s timeslice; nothing in the middle is missing
(`ffmpeg -xerror` decodes the whole file; the criterion's "minute 10" is the free plan's limit, so
the kill is at 9:00).

### Stop → link, 20 recordings each (4 s recordings, signed in; a second page opens the link)

| Browser | Stop → link p50 / p95 | Stop → playable p50 / p95 | First frame p75 |
| --- | --- | --- | --- |
| Chromium | 0.35 s / **0.42 s** | 0.71 s / **1.86 s** | 1.12 s |
| Firefox | 0.37 s / **0.92 s** | 1.61 s / **1.98 s** | 0.26 s |

Criteria: stop → link ≤ 5 s p95 ✅; stop → playable ≤ 5 s ✅; first frame ≤ 1.5 s p75 ✅ (§15 US-31).
This is a workstation, not the reference setup, and 4 s takes, not 30 minutes.

### Dropping the network for 60 s (re-run at production limits)

`m4-demo.spec.ts` (10-minute recording, offline from 3:00 to 4:00): **294 / 294 chunks** on the
server, every hash equal to what left the browser, backlog caught up 15 s after reconnecting.

## MVP exit criteria (docs/design.md §12)

| Criterion | State |
| --- | --- |
| 30-minute 1080p recording on Chrome, Edge, Firefox, Safari uploads and plays everywhere | **Open.** A 30-min 1080p take *processes* in 10 min on 4 cores; 10-min recordings upload and play in Chromium and Firefox. Not shown: a 30-minute browser recording (the free plan stops at 10:00), Edge as a recording browser, Safari |
| Killing the tab at minute 10 and reopening recovers the recording with no gaps | ✅ shown at minute 9 (above), Chromium and Firefox |
| Dropping network for 60 s mid-recording loses nothing | ✅ |
| Stop-to-link ≤ 5 s p95 on the reference setup | ✅ locally (0.4–0.9 s p95); **no reference setup exists** to repeat it on |
| Cross-tenant access tests pass; no public bucket paths | ✅ generated harness on every PR; `the_bucket_is_private_and_a_signature_opens_one_object` (anonymous read, list, write, delete refused; a signature opens one key only) |
| Extension installed from the Chrome Web Store starts a recording from any tab with click highlights visible | **Open.** Works installed unpacked (M7); not in a store |

## The launch itself: not done

"Production launch with 10 developer users" needs a production host and ten people. Neither can be
produced from here. `docs/launch/go-no-go.md` lists exactly what is proven, what is open and what
a person has to do (provision, first real restore, legal review, store submission, device checks);
`docs/launch/invite.md` is the invitation and the three follow-up questions.

## Bugs found by the rehearsal

- The watch page polled a processing recording twice a second, which at the designed public-watch
  limit (120/min/address) would have used the whole budget of one open tab. It now backs off from
  1 s to 8 s and treats a `429` as "wait", not as an error.
- A Day 39 test expected five 2-second chunks from a 10 s Firefox recording and got four on a slow
  CI run: it now records 11 s.
