# M6 — Share, watch, library demo (Days 52–60)

Date: 2026-10-05
Branch: `feat/day-060-tenant-sweep-m6-demo` (on top of `main` @ `52db0e4`, Day 59)
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` and `worker` binaries,
`ng serve` over HTTPS with the same-origin storage proxy). There is still no staging VPS (M1
carry-over), so this demo runs locally, as M2–M5's did.

## Scope

`docs/design.md` §12, weeks 10–11: "Share links (private/link/public), watch page, playback
grants, basic player, library list."

The demo's headline, from the plan: **the full record → share → watch loop.**

Built in this milestone:

| Day | What |
| --- | --- |
| 52 | `sharing` crate, `share_links`, create/list/update/revoke endpoints, 71-bit slugs (ADR-0014) |
| 53 | `sharing::decide` access table, `delivery` crate, `GET /s/{slug}` and `/playback` with 15-minute signed URLs; `Access::Viewer` (ADR-0015) |
| 54 | Watch page `/s/:slug`: poster, player, speed, fullscreen, ←/→ seek |
| 55 | The link is created and copied when the upload finishes; Share dialog |
| 56 | MP4 download as a signed attachment URL (owner, and viewers when the link allows it) |
| 57 | Library: `GET /recordings` (keyset cursor, 24 per page, signed thumbnails) and `/library` |
| 58 | Inline rename, trash (links die on the next request), `PurgeRecording` after 30 days |
| 59 | Processing status on the watch page; the original plays as a preview while the MP4 is made (ADR-0016) |
| 60 | Tenant-isolation harness covers every route, classification and CSRF lints; this demo |

## How to run it yourself

```bash
just deps-up && just db-migrate
just worker                   # terminal 1 (processes takes, sends the email, purges the trash)
just web                      # terminal 2 -> https://localhost:4200 (accept the self-signed cert once)
just api                      # terminal 3 -> :8080
```

### A. By hand (any desktop browser with a real screen and mic)

1. Sign up, verify via Mailpit (`http://localhost:8025`), log in, **New recording**, record ~10 s,
   **Stop**. **Expect:** "Recording uploaded" and, under it, "Link copied: …/s/<slug>".
2. Paste the link in a private window (no session). **Expect:** the video plays within a couple of
   seconds; first a "Preview — the full-quality version is still being prepared" banner, then,
   without reloading, the banner disappears and the MP4 is playing.
3. Try speed 1.5×, ← / → (5 s), space, F (fullscreen).
4. Back as the creator: **Share…** → "Only me". **Expect:** the private window now says "This link
   doesn't work" (reload). Set it back to "Anyone with the link": it plays again.
5. **Your recordings** (`/library`): the card has a thumbnail, length and date. Click the title,
   type "Sprint demo", Enter. Reload the private window: the new title shows.
6. **Download**: `Sprint demo.mp4` saves and plays offline. In the private window there's no
   Download button until **Share…** → "Let viewers download the video" is ticked.
7. **Delete** → **Yes**. **Expect:** the card disappears; the private window's next reload says
   "This link doesn't work". (After 30 days `PurgeRecording` removes the files for good.)

### B. Automated (filmed)

```bash
just worker &                 # the spec needs the worker running
just demo-m6                  # Chromium and Firefox, ~1 min each
```

`web/e2e/m6-demo.spec.ts` (skipped unless `DEMO=1`) does steps 1–7 through the real pages on the
fake-media harness, with the creator and the stranger in separate browser contexts. The films
(`owner.webm`, `viewer.webm`), the downloaded MP4 and `result.json` go to
`web/demo-output/m6/<engine>/` (gitignored).

## Results (2026-10-05)

| Engine | Recorded | Stop → link | Stop → playable (stranger) | First frame | Stop → MP4 on screen | Download | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Chromium (system Chrome, fake devices) | 10.06 s, 5 chunks | 261 ms | 1.68 s (preview first) | 708 ms | 6.55 s | `Sprint demo.mp4`, H.264 + AAC, 10.1 s, decodes offline | **Pass** |
| Firefox (Playwright build, fake mic + canvas screen) | 10.11 s, 5 chunks | 3.06 s | 4.57 s (preview first) | 648 ms | 6.20 s | same, 10.2 s | **Pass** |
| WebKit (Playwright Linux build) | — | — | — | — | — | — | **Not demonstrated**: no `MediaRecorder` in this build (Day 22) |

Both runs: link copied to the clipboard (read back on Chromium), preview → MP4 switch without a
reload, private link `404` to the stranger and back, inline rename visible to the stranger,
download blocked for viewers until allowed, trash kills the link on the next request.

Firefox's stop → link of 3.06 s is the upload backlog (stop → finalized took 2.9 s on this run;
earlier Firefox runs finalize in 0.2–0.3 s), still inside the 5 s target but worth watching on the
reference machine.

### Checks that run in CI on every PR

| Check | Where | Result |
| --- | --- | --- |
| Slugs are 71 bits, unique, uniform; revoke is immediate | `sharing` tests (Day 52) | Pass |
| Private link `404` to anonymous and to another workspace; owner allowed | `watch` tests, harness `viewer_routes_hide_private_links_from_other_tenants` (Days 53, 60) | Pass |
| First frame ≤ 1.5 s; speed, seek, fullscreen | `web/e2e/watch-page.spec.ts` (Day 54) | Pass (405 ms Firefox) |
| Link copied after stop and working; Share dialog | `web/e2e/share-after-stop.spec.ts` (Day 55) | Pass |
| Downloaded file plays offline | `web/e2e/download.spec.ts` (Day 56) | Pass |
| 1,000 recordings list in ≤ 150 ms | `recordings` test (Day 57) | Pass (6 ms first page, 11 ms slowest) |
| Rename in place; trashed link dies at once; purge removes the storage prefix | `web/e2e/trash.spec.ts`, `catalog` tests (Day 58) | Pass |
| Stop → playable ≤ 5 s; preview → MP4 switch | `web/e2e/processing-status.spec.ts` (Day 59) | Pass (1.3–2.6 s) |
| Every workspace route hides other tenants' data **and leaves it unchanged**; every route is classified by what its handler takes; every state-changing session route checks CSRF | `bin/api/src/tenant_isolation.rs` (Day 60) | Pass |

## Exit criteria and acceptance criteria

§12 weeks 10–11 deliverables: share links ✅ · watch page ✅ · playback grants ✅ · basic player ✅
· library list ✅ (plus rename, trash and purge, which §9 lists for the library).

| Story | Criterion | Status |
| --- | --- | --- |
| US-30 | Link copied to clipboard on stop by default | ✅ `share-after-stop.spec.ts`, this demo |
| US-30 | Private / link / public | 🟡 private and link verified; `public` is stored and works like `link`, but listing and indexing it is V1 |
| US-30 | Visibility change takes effect on the next playback request | ✅ this demo (URLs already issued last ≤ 15 min) |
| US-31 | First frame ≤ 1.5 s p75 on 10 Mbps | 🟡 0.4–0.7 s locally, unthrottled and few samples; the p75 on a throttled link needs the reference machine |
| US-31 | Speed 0.5–2×, fullscreen, keyboard seek | ✅ |
| US-31 | Unauthorised viewer gets `404` on private recordings | ✅ |
| US-40 | List newest first with thumbnail, title, duration, date; 24 per page | ✅ |
| US-40 | Rename inline; delete moves to trash; links stop at once | ✅ |
| US-40 | Trash purged after 30 days, including the storage prefix | ✅ (test with real MinIO and a live run) |
| US-21 | MP4 H.264/AAC with `faststart` for every take; poster; `ready`; "ready" email | ✅ (M5) |
| US-21 | Output within 100 ms of the recorded duration | ⏳ measured to ~±0.1 s on the demo files (10.1 s vs 10.06 s) but the golden tests allow ±1 s; tighten |
| US-21 | Processing fails 5 times → `failed`, DLQ, **retry button** | 🟡 state, DLQ and `POST /recordings/{id}/retry` exist; no button in the UI yet |
| US-12 | Recovered recording plays with no gap > 2 s | ⏳ plays and seeks (golden `truncated_last_chunk`), gap size not measured |
| US-20 | Stop → link ≤ 5 s p95 for 30 min at 1080p on 10 Mbps | ⏳ 0.26–3.1 s here for a 10 s take; the p95 over 30-minute takes needs the reference machine |

§12 MVP exit criteria touched: "Stop-to-link ≤ 5 s p95" and "30-minute 1080p recording … plays
everywhere" are still open (reference machine, Safari).

## Carry-over

- No staging VPS (M1). §14 rows stay *In progress* until deployed; first-frame, stop-to-link and
  the 30-minute runs need the reference machine.
- WebKit/Safari recording and watching not demonstrated on Linux (macOS/Safari check). Safari
  viewers get the "still being processed" notice instead of a WebM preview (`canPlayType`).
- Retry button for failed recordings in the library (the endpoint exists since Day 48).
- Restore from trash (V1, §9): trashing is final from the UI; the purge runs after 30 days.
- `public` visibility: listing/indexing (V1).
- Password-protected links, expiry UI and invites (V1); the API accepts `expires_at`.
- Per-slug rate limit and the `cargo audit` advisories (M8).
- Local `just e2e` still doesn't start the worker; `just demo-m5`/`demo-m6` need `just worker`.
