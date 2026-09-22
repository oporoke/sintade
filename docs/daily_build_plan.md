# Sintade — Daily Build Plan

The whole platform broken into working days. Every day ends with one concrete, demonstrable deliverable merged to `main` and green in CI; the next day starts from exactly that build. Nothing is "90% done" overnight.

### Rules

1. **One deliverable per day.** It is runnable, testable or visible — an endpoint answering, a screen rendering, a job producing a file. The *Check* column is how you prove it before stopping.
2. **Each day builds on the previous one.** A day's deliverable is the starting point of the next; if a day slips, the plan shifts, it does not skip.
3. **`main` is always deployable.** Unfinished UI hides behind feature flags; from Day 10 onward every merge auto-deploys to staging.
4. **Update the progress tracker** (main tab, section 14) when a milestone closes, and the known-issues log when something is deferred.
5. **Milestone days are demo days.** The last day of each milestone ends with an end-to-end demo of everything built so far.

### Assumptions

- One full-time developer, 5 working days a week; a day ≈ 6 focused hours.
- Design document is the spec: sections referenced as "§N" point to the main tab.
- Estimates are realistic for a solo developer familiar with Rust/Axum and Angular; buffer is built into milestone-closing days.

### Overview

| Phase | Days | Weeks | Ends with |
| --- | --- | --- | --- |
| 1 — MVP | 1–70 | 1–14 | Record, upload, process, share, watch, library, extension, in production with 10 developer users |
| 2 — V1 | 71–150 | 15–30 | Competitive product: teams, editing, transcripts, HLS, engagement, payments |
| 3 — V2 | 151–215 | 31–43 | Differentiators and scale: AI, public API, integrations, SSO, GPU workers |
| 4 — Future | 216–335 | 44–67 | Camtasia parity in the browser |

## Phase 1 — MVP (Days 1–70)

Ends with Sintade in production: record in the browser or extension, upload while recording, MP4 ready seconds after stop, share a link, manage a library.

### M1 — Foundation (Days 1–10)

| Day | Deliverable | Check |
| --- | --- | --- |
| 1 | Repo, Cargo workspace (`core`, `platform`, `bin/api`, `bin/worker`), `rust-toolchain.toml`, `justfile`, README stub | `cargo build` and `just check` pass |
| 2 | `compose.yml` with Postgres, MinIO (bucket + CORS), Mailpit; `.env.example`; `just deps-up` | All three containers healthy |
| 3 | `platform`: config loader, `PgPool`, JSON `tracing`, sqlx migration wiring; API serves `/healthz`, `/readyz` | `curl /readyz` returns 200 with DB check |
| 4 | `core`: UUIDv7 newtype IDs, `AppError` → problem+json, `Clock`; API middleware (request ID, trace, CORS, body limit, timeout) | Unknown route returns problem+json with a request ID |
| 5 | `ObjectStore` trait + MinIO adapter (presign PUT/GET, head, delete\_prefix) | Integration test uploads through a presigned URL |
| 6 | `jobs` table + `JobQueue` (enqueue, `SKIP LOCKED` poll, backoff, DLQ); worker runs a `Noop` job | Failing job reaches DLQ after 5 attempts |
| 7 | `outbox_events` + transactional writer; worker outbox relay + subscriber registry; LISTEN/NOTIFY helper | Event written in a transaction reaches its subscriber exactly once |
| 8 | `Mailer` trait + SMTP adapter; `SendEmail` job | Test email appears in Mailpit |
| 9 | Angular shell: routing, `ApiClient`, error interceptor, theme tokens, `CapabilityService` | Debug page shows correct capability matrix in Chrome, Firefox, Safari |
| 10 | CI: fmt, clippy, sqlx check, tests, lint, Angular build; images for api/worker/web; auto-deploy to staging VPS | **Demo:** staging shell loads, `/readyz` green |

### M2 — Identity (Days 11–20)

| Day | Deliverable | Check |
| --- | --- | --- |
| 11 | Identity migrations (`users`, `credentials`, `sessions`, `email_tokens`); `Email` and `Password` domain types with breached-list check | Unit tests for every validation rule |
| 12 | `POST /auth/register`: argon2id, personal workspace (`workspaces`, `memberships`) in the same transaction, verification email | Register → email in Mailpit, rows exist |
| 13 | `POST /auth/login`, cookies, `SessionClaims` extractor, `GET /me` | Login then `/me` returns user and workspace |
| 14 | Refresh rotation with family reuse detection; logout | Replaying an old refresh token revokes the family |
| 15 | Verify-email, forgot-password and reset flows (hashed tokens, TTLs, single use, session revocation) | Integration tests for all three flows |
| 16 | Rate limits on login/signup, CSRF double-submit, security headers | 6th login in a minute → `429`; headers present |
| 17 | Angular signup, login, verify, forgot/reset screens; silent refresh; route guards | Full signup-to-login through the UI |
| 18 | `WorkspaceContext` extractor, `Permission` enum, `authorize()`; generated tenant-isolation test harness | Harness runs green over current routes |
| 19 | Profile settings, logout everywhere; OpenAPI (utoipa) + generated TS DTOs in CI | Frontend compiles against generated DTOs |
| 20 | Buffer, fixes, tracker update | **Demo:** new user signs up on staging, verifies, logs in |

### M3 — Capture engine (Days 21–32)

| Day | Deliverable | Check |
| --- | --- | --- |
| 21 | Framework-free `capture` package; `SourceManager` (display + mic streams, device enumeration) | Debug page previews chosen screen and mic |
| 22 | `AudioMixer`: mic + display audio into one track; level analyser | Test clip contains both audio sources |
| 23 | `ChunkRecorder`: MIME selection, 2 s timeslice, `chunks$`, `state$` | Concatenated chunks play as a valid file |
| 24 | Pause/resume, timer excluding pauses, `track.onended` stop | Clip with a pause plays correctly |
| 25 | `ChunkStore` on OPFS with IndexedDB fallback | Chunks survive a page reload |
| 26 | Orphan detection + recovery dialog (Upload / Discard) | Kill tab mid-recording, reopen, dialog shows correct duration |
| 27 | Recorder UI: source picker, mic selector, system-audio toggle with unsupported reasons | Toggle disabled with reason on Safari/Firefox |
| 28 | Device check (mic meter) and 3-2-1 countdown (Esc to skip) | Countdown and meter work in all engines |
| 29 | Control bar: start, pause, resume, stop, timer; keyboard and screen-reader labels | Recorder operable by keyboard only |
| 30 | Permission-denied help per browser, capture error states, mobile view-only notice | Denying permission never shows a blank screen |
| 31 | Playwright harness with fake media on Chromium, Firefox, WebKit | E2E: record 5 s, chunks present in OPFS |
| 32 | Buffer and polish | **Demo:** 2-minute local recording on three browsers; crash and recover |

### M4 — Ingest (Days 33–42)

| Day | Deliverable | Check |
| --- | --- | --- |
| 33 | `recordings`, `takes`, `chunks` migrations; `POST /recordings` creates recording + take | Endpoint returns IDs; rows scoped by workspace |
| 34 | Presign chunk endpoint (single + `?count=` batch) with ownership check | `curl` PUT to returned URL lands in MinIO |
| 35 | Ack endpoint: size + SHA-256, idempotent, `409` on mismatch | Idempotency and conflict tests pass |
| 36 | Status + finalize endpoints (contiguity, `422` with missing list, state → processing, `TakeFinalized`) | Finalize with a gap returns the missing indexes |
| 37 | Client `Uploader`: SubtleCrypto SHA-256, presign → PUT → ack queue | Uploaded chunk hashes match server records |
| 38 | Backoff retry, offline detection, resume from status endpoint | Throttled/offline network test recovers |
| 39 | Streaming upload wired into recorder; drain progress on stop; finalize; clear OPFS | Stop → finalize in under 2 s on 10 Mbps |
| 40 | Recovery upload path: orphan → status → missing chunks → finalize | Recovered take finalizes with no gaps |
| 41 | `billing` stub with free-tier entitlements enforced at create/finalize; `SweepStaleUploads` job | 51st recording and 11-minute take are rejected |
| 42 | E2E tests for loss scenarios | **Demo:** 10-minute recording, network cut for 60 s, nothing lost |

### M5 — Processing (Days 43–51)

| Day | Deliverable | Check |
| --- | --- | --- |
| 43 | `media` crate, `renditions` migration, `ProcessTake` subscribed to `TakeFinalized`, scratch manager | Job starts on finalize and cleans its scratch dir |
| 44 | Download chunks, verify hashes, concatenate `source.webm`, store it | Source file plays and matches chunk hashes |
| 45 | ffprobe validation + rejection; golden fixtures committed | `not_a_video.webm` is rejected cleanly |
| 46 | FFmpeg runner (timeouts, progress) + fast-start MP4; Safari remux path | MP4 plays and seeks in every browser |
| 47 | Poster, rendition rows, state → `ready`/`failed`, `RecordingReady`/`ProcessingFailed` | State transitions covered by tests |
| 48 | "Recording ready" email; retry action for failed recordings | Email arrives; retry reprocesses |
| 49 | Golden-file pipeline tests for all fixtures in CI | CI asserts duration, codecs, seekability |
| 50 | Performance pass on reference 4 vCPU; per-kind concurrency | 30-min 1080p processed in ≤ 15 min |
| 51 | Buffer | **Demo:** record → stop → MP4 ready → email |

### M6 — Share, watch, library (Days 52–60)

| Day | Deliverable | Check |
| --- | --- | --- |
| 52 | `share_links` migration; create/update/revoke endpoints; 70-bit slug generator | Slugs unique; revoke takes effect immediately |
| 53 | `can_view` + `grant`; `GET /s/{slug}` and `/s/{slug}/playback` with 15-min signed URLs | Private link returns `404` to others |
| 54 | Watch page: poster, player, speed, fullscreen, keyboard seek | First frame ≤ 1.5 s on staging |
| 55 | Share dialog: visibility, link auto-copied on stop | Link copied and working after stop |
| 56 | Owner MP4 download via signed URL | Downloaded file plays offline |
| 57 | Library list: cursor pagination (24), thumbnails, duration, date, empty state | 1,000 seeded recordings list in ≤ 150 ms |
| 58 | Inline rename, trash, `PurgeRecording` after 30 days | Trashed link dies at once; purge removes storage prefix |
| 59 | Processing status on watch page; WebM fallback playback while MP4 builds | Stop-to-playable ≤ 5 s |
| 60 | Tenant-isolation harness covers every route | **Demo:** full record-share-watch loop on staging |

### M7 — Browser extension (Days 61–66)

| Day | Deliverable | Check |
| --- | --- | --- |
| 61 | MV3 scaffold: manifest, service worker, popup, build pipeline | Loads unpacked in Chrome and Edge |
| 62 | Session handoff from web app to extension | Extension calls `/me` as the signed-in user |
| 63 | Offscreen document running `capture`; tab recording through the same ingest | Tab recording from the extension plays on Sintade |
| 64 | Content script: click highlights and keystroke overlay | Highlights visible in the output video |
| 65 | Popup UX: sources, start/stop, link copied, error states | Recording from any tab in ≤ 3 clicks |
| 66 | Store assets, privacy policy, submit to Chrome Web Store and Edge Add-ons | Submission accepted for review |

### M8 — Hardening and launch (Days 67–70)

| Day | Deliverable | Check |
| --- | --- | --- |
| 67 | Rate limits on all routes, header audit, ZAP baseline, gitleaks, cargo/npm audit clean | Zero high findings |
| 68 | Postgres PITR + nightly base backup; MinIO off-site mirror; restore drill; runbooks | Staging restored from backup |
| 69 | Production API, worker and MinIO hosts; TLS, DNS, `deploy-production` job; Sentry and alerts | Production `/readyz` green |
| 70 | Production launch with 10 developer users | **Demo:** all MVP exit criteria (main tab §12) ticked |

## Phase 2 — V1 (Days 71–150)

Starts from the production MVP. Ends with a product teams pay for: webcam, adaptive playback, editing, transcripts, workspaces, engagement, account security and TZS payments.

### M9 — Capture upgrades (Days 71–78)

| Day | Deliverable | Check |
| --- | --- | --- |
| 71 | `Compositor`: canvas screen + webcam bubble (circle/rounded), OffscreenCanvas where available | Bubble appears in the output video |
| 72 | Drag/resize bubble during recording; camera-only mode | Position changes are recorded live |
| 73 | Per-source volume and mute; live meters for every source | Muted source is silent in output |
| 74 | Noise suppression/AEC/AGC toggles; quality presets (720p/1080p/4K, 30/60 fps, bitrate) capped by entitlements | Free user cannot pick 4K |
| 75 | Region crop; cursor show/hide | Cropped region fills the output frame |
| 76 | Restart/discard take; keyboard shortcuts; plan max duration auto-stop with warning | Auto-stop at plan limit uploads cleanly |
| 77 | Wake Lock, storage-quota pre-check, `beforeunload` guard | Warning at 80% quota; laptop does not sleep |
| 78 | CPU profiling and fixes | **Demo:** 1080p60 with webcam within CPU target |

### M10 — Adaptive playback and live status (Days 79–88)

| Day | Deliverable | Check |
| --- | --- | --- |
| 79 | `BuildHls` job: 360p/720p/1080p fMP4 ladder + master playlist | Ladder validates with ffprobe |
| 80 | Lazy ladder (only after first view); `RenditionReady` events | Unviewed recordings have no HLS objects |
| 81 | `GenerateSprite` + `sprite.vtt`; animated WebP preview | Sprite frames align with timestamps |
| 82 | SSE endpoint on LISTEN/NOTIFY; live status in library and watch page | Status updates without refresh |
| 83 | Player v2: hls.js, native HLS on Safari, quality selector, MP4 fallback | Quality switch mid-play without stall |
| 84 | Hover sprite preview, resume position, player shortcuts | Reopening resumes at last position |
| 85 | Owner-defined chapters/markers + player chapter UI | Clicking a chapter seeks correctly |
| 86 | Signed HLS segment access (token per manifest) | Segment URLs expire; no hotlinking |
| 87 | Delivery rendition cache + load test on playback path | 200 concurrent viewers without errors |
| 88 | Buffer | **Demo:** 1-hour recording plays adaptively with previews and chapters |

### M11 — Editor (Days 89–97)

| Day | Deliverable | Check |
| --- | --- | --- |
| 89 | `edits` migration; EDL domain type with range validation | Property tests on overlapping/invalid ranges |
| 90 | `POST /recordings/{id}/edits`; `RenderEdit` job concatenates kept ranges | Rendered MP4 duration equals kept ranges |
| 91 | Edit versioning: new renditions, HLS rebuild, `ready → processing → ready` | Old version replaced only after success |
| 92 | Waveform peaks job (JSON) | Peaks render in under 200 ms |
| 93 | Editor UI: timeline with waveform and thumbnails | Timeline matches video length |
| 94 | Trim handles with instant preview | Preview starts/ends at handles |
| 95 | Cut segments; preview skips cuts | Preview matches final render |
| 96 | Title/description in editor; undo/redo; revert to original | Revert restores original take |
| 97 | Buffer | **Demo:** trim + two cuts, re-share same link |

### M12 — Transcription and search (Days 98–105)

| Day | Deliverable | Check |
| --- | --- | --- |
| 98 | whisper.cpp in worker image; `Transcriber` trait + adapter | Transcribes a fixture offline |
| 99 | `Transcribe` job on `AudioReady` (16 kHz extract); `transcripts`, `transcript_segments` | Segments stored with timings |
| 100 | VTT/SRT caption renditions; `GET /recordings/{id}/transcript` | Captions load in the player |
| 101 | Transcribe priority and concurrency separate from FFmpeg; benchmark | Transcription ≤ 2× duration |
| 102 | Transcript panel with click-to-seek and live highlight | Clicking a line seeks exactly |
| 103 | Caption editing; regenerated VTT | Edited word appears in captions |
| 104 | Library search across titles and transcripts with jump-to-time results | Search hit opens at the spoken moment |
| 105 | Buffer | **Demo:** find a recording by a phrase said in it |

### M13 — Workspaces and library (Days 106–115)

| Day | Deliverable | Check |
| --- | --- | --- |
| 106 | Multiple workspaces per user; create workspace; switcher | Data separated per workspace |
| 107 | Invites: create, email, accept | Invitee joins with the right role |
| 108 | Final permission matrix enforced; change-role endpoint | Matrix tests for every role × action |
| 109 | Members UI: list, invite, change role, remove | Removed member loses access at once |
| 110 | `workspace` visibility; shared workspace library | Members see shared recordings only |
| 111 | `folders` migration; create/rename/move (max depth 5) | Depth limit enforced |
| 112 | Folder tree UI with drag-to-move | Moves persist |
| 113 | Filters and sort; bulk move/trash/visibility | Bulk action on 100 items succeeds |
| 114 | Trash view with 30-day restore | Restored link works again |
| 115 | Tenant-isolation harness extended | **Demo:** a 5-person team shares a library |

### M14 — Sharing controls (Days 116–121)

| Day | Deliverable | Check |
| --- | --- | --- |
| 116 | Password-protected links + unlock cookie | Wrong password rate-limited |
| 117 | Link expiry, revoke/regenerate slug, viewer download toggle | Expired link shows expiry page |
| 118 | Invite-only links by email | Non-invitee gets `404` |
| 119 | Open Graph tags on watch pages with poster/preview | Rich preview in Slack and WhatsApp |
| 120 | oEmbed endpoint + embed code | Embed plays on an external page |
| 121 | Buffer | **Demo:** protected, expiring, embeddable link |

### M15 — Engagement and analytics (Days 122–130)

| Day | Deliverable | Check |
| --- | --- | --- |
| 122 | `comments` migration + endpoints (timestamped, threaded) | Threads ordered by time |
| 123 | Comment UI with timeline markers | Clicking a marker seeks |
| 124 | @mentions of workspace members + `CommentCreated` events | Mentioned user notified |
| 125 | Emoji reactions at timestamps | Reactions show on timeline |
| 126 | View start + heartbeat; unique-view dedupe; `view_segments` | Counts match server logs within 2% |
| 127 | Aggregation job: views, unique viewers, average watch time | Stats update within 5 min |
| 128 | Owner analytics panel per recording | Numbers match aggregation |
| 129 | Abuse reports on public recordings + takedown flow | Taken-down link returns 410 page |
| 130 | Buffer | **Demo:** team review with comments and stats |

### M16 — Account security and privacy (Days 131–136)

| Day | Deliverable | Check |
| --- | --- | --- |
| 131 | Google OAuth sign-in | New and existing users both work |
| 132 | Microsoft and GitHub OAuth; account linking | Linked accounts share one user |
| 133 | TOTP enrolment, recovery codes, login challenge | Recovery code works once |
| 134 | Active sessions list + remote logout | Remote logout kills session in ≤ 15 min |
| 135 | Data export and account deletion with 30-day purge | Export downloadable; purge removes storage |
| 136 | Buffer | **Demo:** secured account, export, delete |

### M17 — TZS billing and mobile money (Days 137–145)

| Day | Deliverable | Check |
| --- | --- | --- |
| 137 | `plans`, `subscriptions`, `usage_records` migrations; entitlements from plan; TZS `Money` | Upgrading lifts limits at once |
| 138 | `PaymentProvider` trait; M-Pesa sandbox adapter (payment push) | Sandbox prompt reaches test phone |
| 139 | M-Pesa callback: verification, idempotent `payment_events` | Duplicate callback ignored |
| 140 | Airtel Money adapter + callback | Sandbox payment completes |
| 141 | Mixx by Yas and HaloPesa adapters (as each operator approves) | Each approved operator passes sandbox |
| 142 | Checkout UI: plan, operator, phone, pending/confirmed states | Pending state resolves on callback |
| 143 | Usage metering (storage, minutes), seats, proration | Mid-cycle seat add prorated |
| 144 | TZS receipts/invoices; daily reconciliation job | Reconciliation flags a mismatch |
| 145 | Buffer | **Demo:** upgrade a workspace with sandbox M-Pesa |

### M18 — Notifications, CDN, observability, release (Days 146–150)

| Day | Deliverable | Check |
| --- | --- | --- |
| 146 | In-app notification centre + per-user preferences | Muted event sends nothing |
| 147 | Emails: comment, mention, first view, invite (templates) | All templates render in Mailpit |
| 148 | CDN in front of MinIO and SPA with token auth | Origin hit ratio > 90% in test |
| 149 | Prometheus metrics, Grafana dashboards, OTel traces, alert rules; MinIO encryption at rest | Alerts fire in a drill |
| 150 | k6 load test, WCAG 2.2 AA fixes, dark/light theme | **Demo:** V1 exit criteria (main tab §12) ticked |

## Phase 3 — V2 (Days 151–215)

Starts from the V1 release. Ends with differentiators and scale: advanced editing, AI, integrations and public API, enterprise controls, growth billing, GPU workers and Swahili.

### M19 — Advanced editing (Days 151–161)

| Day | Deliverable | Check |
| --- | --- | --- |
| 151 | EDL supports multiple sources; `RenderEdit` stitches takes and recordings | Stitched output has correct total duration |
| 152 | Stitch UI: add recordings to the timeline, reorder | Reordered sections render in order |
| 153 | Text overlay model (text, position, time range) rendered by FFmpeg | Overlay appears at the right time |
| 154 | Text overlay editor UI | Drag-positioned text matches render |
| 155 | Blur/pixelate regions with time ranges | Redacted area unreadable in output |
| 156 | Redaction UI with preview | Preview matches render |
| 157 | Zoom/pan keyframes rendered via crop/scale | Zoom animation smooth at 30 fps |
| 158 | Zoom/pan editor UI | Keyframes editable and previewed |
| 159 | Re-record a segment and splice it in | Replacement plays seamlessly |
| 160 | Extension records a cursor track; smoothed cursor rendered | Jittery path renders smooth |
| 161 | Buffer | **Demo:** stitched, annotated, redacted, zoomed video |

### M20 — AI features (Days 162–171)

| Day | Deliverable | Check |
| --- | --- | --- |
| 162 | `Summarizer` trait, provider ADR (self-hosted vs API), prompt templates | ADR accepted; adapter passes contract tests |
| 163 | Summary and auto-title from transcript | Summary saved and shown on watch page |
| 164 | Auto-chapters from transcript into player chapters | Chapters align with topic changes |
| 165 | Filler-word and long-pause detection from word timings | Detected fillers listed with timestamps |
| 166 | One-click filler removal as an EDL | Output has no detected fillers |
| 167 | Transcript-based cuts: deleting words produces EDL ranges | Deleted words gone from render |
| 168 | Transcript editor UI | Edits preview before render |
| 169 | Caption translation with language picker | Second-language captions load |
| 170 | Q&A over a recording with timestamped answers | Answer links seek to source moment |
| 171 | Buffer | **Demo:** raw take becomes titled, chaptered, cleaned, bilingual |

### M21 — Capture and viewer insights (Days 172–179)

| Day | Deliverable | Check |
| --- | --- | --- |
| 172 | Camera background blur/replace via segmentation | Background replaced at ≥ 24 fps |
| 173 | Document PiP floating controls; `preferCurrentTab` | Controls float over other windows |
| 174 | On-screen drawing while recording | Drawings appear in output |
| 175 | Separate stored mic/system tracks; remix in editor | Rebalanced mix renders |
| 176 | Viewer list with watch percentage | Per-viewer % matches heartbeats |
| 177 | End-screen call-to-action; anonymous viewer name for comments | CTA clicks tracked |
| 178 | Retention (drop-off) curve per recording | Curve matches view segments |
| 179 | Buffer | **Demo:** polished recording with CTA and viewer insights |

### M22 — Public API and integrations (Days 180–190)

| Day | Deliverable | Check |
| --- | --- | --- |
| 180 | Scoped API keys (bearer) + management UI | Key without scope gets `403` |
| 181 | Public REST API (recordings, links, transcripts) + published docs | External script lists recordings |
| 182 | Webhook endpoints with signed deliveries; `DeliverWebhook` retries | Signature verifies; failed delivery retries |
| 183 | Webhook delivery log + replay | Replay redelivers once |
| 184 | Slack app: link unfurl with preview | Link unfurls with poster in Slack |
| 185 | Slack: share to channel and notifications | Comment posts to channel |
| 186 | GitHub: unfurl in issues/PRs, link recording to PR | PR shows recording card |
| 187 | Jira: attach recording to an issue | Issue shows playable link |
| 188 | Notion embed; Google Drive export | Recording exported to Drive |
| 189 | Zapier/Make: triggers (ready, comment), actions (create link) | Sample zap runs end to end |
| 190 | Buffer | **Demo:** recording flows into Slack, GitHub and Jira |

### M23 — Enterprise controls (Days 191–200)

| Day | Deliverable | Check |
| --- | --- | --- |
| 191 | OIDC SSO | Login via test IdP |
| 192 | SAML SSO | Login via SAML test IdP |
| 193 | SCIM provisioning | Deprovisioned user loses access |
| 194 | Team spaces inside a workspace | Space members only see their space |
| 195 | Admin policies: default visibility, disable public links, allowed domains | Policy blocks a public link |
| 196 | Audit log (view, share, delete, role change) + UI + export | Every sensitive action logged |
| 197 | Domain-restricted embeds | Embed refused on other domains |
| 198 | Viewer email watermark option | Watermark visible on playback |
| 199 | Retention policies; tags; duplicate recording | Policy deletes on schedule |
| 200 | Buffer | **Demo:** enterprise workspace with SSO, SCIM, audit |

### M24 — Growth billing and engagement (Days 201–206)

| Day | Deliverable | Check |
| --- | --- | --- |
| 201 | Trials and coupons | Coupon applies correct TZS discount |
| 202 | Annual billing; dunning (retries, grace period, notices) | Failed renewal enters grace, then downgrades |
| 203 | Daily/weekly digest emails | Digest lists correct view counts |
| 204 | Web push notifications | Push arrives on opt-in browser |
| 205 | Workspace analytics dashboard + CSV export | CSV totals match dashboard |
| 206 | Buffer | **Demo:** trial → paid annual with digests |

### M25 — Scale and localisation (Days 207–215)

| Day | Deliverable | Check |
| --- | --- | --- |
| 207 | GPU worker image (hardware encode + GPU whisper), capability-based job routing | GPU host processes 3× faster |
| 208 | Worker autoscaling by queue age | Queue drains under burst test |
| 209 | Multi-node erasure-coded MinIO migration | Node loss causes no data loss |
| 210 | Postgres read replica; analytics reads routed; `views` partitioning | Primary load drops under analytics |
| 211 | Per-tenant cost metrics (storage, egress, compute) | Cost per workspace on dashboard |
| 212 | NSFW moderation on public recordings | Flagged video held for review |
| 213 | Swahili translation of all UI strings | Full app usable in Swahili |
| 214 | Offline record-now/sync-later; scheduled recordings | Offline recording syncs on reconnect |
| 215 | Load test, security review, SOC 2 gap assessment | **Demo:** V2 release |

## Phase 4 — Future: Camtasia parity (Days 216–335)

Starts from the V2 release. Ends with every capability in the main tab's *Future versions* table delivered. Each milestone opens with an ADR because these are expensive to reverse.

### M26 — Multi-track timeline editor (Days 216–235)

| Day | Deliverable | Check |
| --- | --- | --- |
| 216 | ADR: timeline model and render architecture (server FFmpeg graph compiler + browser WebCodecs preview) | ADR accepted |
| 217 | Project model: tracks, clips, positions; `projects`, `project_versions` | Project saves and reloads |
| 218 | Timeline → FFmpeg `filter_complex` compiler with golden tests | Golden projects render identically |
| 219 | `RenderProject` job outputs a new recording rendition | Rendered project plays and shares |
| 220 | Browser preview engine (WebCodecs decode + canvas composite) | Two-track preview in real time |
| 221 | Timeline UI: tracks, clips, playhead, zoom | Scrubbing matches preview |
| 222 | Split, trim clip edges, delete | Edits reflected in render |
| 223 | Ripple move/delete, magnetic tracks | No gaps after ripple delete |
| 224 | Video layering with position/scale | Layered output matches preview |
| 225 | Audio tracks: volume envelopes, mute/solo | Envelope audible in render |
| 226 | Group/ungroup clips | Group moves as one |
| 227 | Clip speed | 2× clip halves duration, pitch preserved |
| 228 | Extend/freeze frame | Frozen frame holds for set time |
| 229 | Sync tracks by audio | Offset camera track auto-aligns |
| 230 | Proxy media for preview | 4K source previews smoothly |
| 231 | Undo/redo stack; autosaved versions | Restore any saved version |
| 232 | Media bin: import video, images, audio | Imported media usable on timeline |
| 233 | Batch export | Three projects export in one run |
| 234 | Performance pass on a 60-minute multi-track project | Preview stays interactive |
| 235 | Buffer | **Demo:** multi-track project from three recordings |

### M27 — Annotations and visual effects (Days 236–253)

| Day | Deliverable | Check |
| --- | --- | --- |
| 236 | Annotation layer with keyframed properties in both engines | Preview and render match |
| 237 | Callouts with styles | Callout styles render correctly |
| 238 | Arrows (incl. curved), lines, shapes | Curved arrow renders smoothly |
| 239 | Sketch-motion (draw-on) annotations | Draw-on animates over set duration |
| 240 | Highlight and spotlight boxes | Spotlight dims outside region |
| 241 | Freeze-region effect | Notification hidden while video plays |
| 242 | Device frame library | Recording framed in laptop/phone/browser |
| 243 | Green screen / chroma key | Green background removed cleanly |
| 244 | Masks, borders, drop shadows, corner rounding, reflections | Each effect toggles independently |
| 245 | Colour adjustment and LUT filters | LUT applies consistently |
| 246 | Transition engine + first 20 transitions | Transitions render between clips |
| 247 | Transition library to 150+ | All transitions pass render tests |
| 248 | Behaviors (text/object motion presets) | Behavior previews and renders |
| 249 | Lottie import and recolouring | Recoloured Lottie matches brand |
| 250 | Motion-graphic intro/outro templates | Template fills with project title |
| 251 | Themes: brand colours, fonts, logos | Theme change restyles all annotations |
| 252 | Templates and shared workspace asset library | Team member reuses a template |
| 253 | Buffer | **Demo:** fully branded, annotated tutorial |

### M28 — Guided polish and cursor effects (Days 254–265)

| Day | Deliverable | Check |
| --- | --- | --- |
| 254 | Canvas presets before recording (vertical, square) | Vertical recording exports 1080×1920 |
| 255 | Looks: size presets per platform | One click resizes for TikTok/YouTube |
| 256 | Layouts: screen-, camera-, balanced-focus; re-editable | Layout change needs no re-record |
| 257 | Backgrounds library (50+ static and animated) | Background applies behind content |
| 258 | Colour-grading filters in looks | Filter visible in preview and render |
| 259 | Editable cursor track for all recordings | Cursor can be hidden after recording |
| 260 | Automatic zoom/pan from cursor and click data | Zooms follow clicks sensibly |
| 261 | Cursor highlight, spotlight, magnifier | Effects render on cursor path |
| 262 | Click rings, click sounds, cursor scale, path editing | Edited path renders |
| 263 | Cursor isolation, lens, gradient, negative | Each effect renders |
| 264 | Cursor motion blur and kinetic cursor | Motion blur visible on fast moves |
| 265 | Buffer | **Demo:** one-click polished social video |

### M29 — Audio and captions (Days 266–275)

| Day | Deliverable | Check |
| --- | --- | --- |
| 266 | AI noise removal effect | Background noise reduced measurably |
| 267 | Compression, fades, pitch | Effects audible and reversible |
| 268 | Record narration over the timeline | Narration lands on its own track |
| 269 | Royalty-free music and SFX library (licensing ADR) | Track added with licence metadata |
| 270 | Music ducking under voice | Music dips during speech |
| 271 | Dynamic word-by-word captions with style presets | Words highlight in sync |
| 272 | Dynamic caption editor (timing, words) | Edited timing renders |
| 273 | VTT/SRT caption import | Imported captions align |
| 274 | Burned-in caption export | Captions visible in exported MP4 |
| 275 | Buffer | **Demo:** clean audio, music, animated captions |

### M30 — Generative AI (Days 276–290)

| Day | Deliverable | Check |
| --- | --- | --- |
| 276 | ADR: TTS, dubbing and avatar providers; licensing | ADR accepted |
| 277 | TTS voiceover from a script with voice picker | Voiceover generated and playable |
| 278 | Voiceover aligned to timeline | Narration matches on-screen steps |
| 279 | Multi-language voiceover | Swahili and English voiceovers |
| 280 | Dubbing pipeline: transcribe → translate → TTS → align | Dubbed audio stays in sync |
| 281 | Dubbing review UI | Reviewer fixes a line and re-dubs |
| 282 | AI script generation from a prompt | Script matches requested length |
| 283 | Script → narration → timeline assembly | Draft video assembled automatically |
| 284 | AI avatar presenter render | Avatar speaks the script |
| 285 | Avatar layouts on the timeline | Avatar positioned beside screen |
| 286 | Generative screencast: screenshots + prompt → storyboard | Storyboard lists steps per screenshot |
| 287 | Storyboard → cursor moves, zooms, transitions | Cursor path animates between steps |
| 288 | Storyboard → narrated video render | Full how-to video with no recording |
| 289 | Generative screencast refinements | Editing a step re-renders only that step |
| 290 | Buffer | **Demo:** training video from screenshots only |

### M31 — Interactivity and e-learning (Days 291–300)

| Day | Deliverable | Check |
| --- | --- | --- |
| 291 | Quiz model (multiple choice, true/false, short answer) on the timeline | Quiz saved at a timestamp |
| 292 | Quiz authoring UI | Author builds a 3-question quiz |
| 293 | Quiz playback (required or skippable) | Required quiz blocks progress |
| 294 | Quiz results + CSV reporting | CSV lists each learner's answers |
| 295 | Interactive hotspots: link, jump to time, pause | Hotspot jumps to target time |
| 296 | Hotspot authoring UI | Hotspot placed and previewed |
| 297 | SCORM 1.2 package export | Package loads in Moodle with tracking |
| 298 | SCORM 2004 export + LMS compatibility tests | Completion reported in two LMSs |
| 299 | Interactive player table of contents | TOC navigates chapters |
| 300 | Buffer | **Demo:** tracked lesson in an LMS |

### M32 — Export and publishing (Days 301–308)

| Day | Deliverable | Check |
| --- | --- | --- |
| 301 | GIF export | GIF under size target |
| 302 | WebM with transparency | Alpha preserved over a background |
| 303 | Audio-only and frame-as-image export | M4A and PNG exported |
| 304 | Export preset system | Saved preset reused |
| 305 | YouTube publishing (OAuth, upload, metadata) | Video appears on YouTube channel |
| 306 | Vimeo and Google Drive publishing | Uploads succeed |
| 307 | Social publishing with vertical presets | Vertical video posted |
| 308 | Buffer | **Demo:** one project published to four destinations |

### M33 — Real-time collaboration (Days 309–315)

| Day | Deliverable | Check |
| --- | --- | --- |
| 309 | ADR: CRDT-based project sync over WebSocket | ADR accepted |
| 310 | CRDT project sync server | Two editors converge |
| 311 | Presence and live cursors in the editor | Collaborator cursor visible |
| 312 | Per-scene access permissions | Restricted scene read-only |
| 313 | Scene-level comments in the editor | Comment anchored to scene |
| 314 | Offline edit merge and conflict tests | Offline edits merge cleanly |
| 315 | Buffer | **Demo:** two people edit one project live |

### M34 — Recording expansion and live (Days 316–335)

| Day | Deliverable | Check |
| --- | --- | --- |
| 316 | Audio-only recording mode | Podcast-style recording shares |
| 317 | Zoom cloud recording import | Zoom recording lands in library |
| 318 | PowerPoint add-in recording slides with timings | Slide timings become chapters |
| 319 | ADR: native desktop recorder (Rust-based) | ADR accepted |
| 320 | Desktop recorder on macOS with full system audio | System audio captured on macOS |
| 321 | Desktop recorder on Windows | Recording uploads via ingest |
| 322 | Desktop recorder on Linux | Recording uploads via ingest |
| 323 | Desktop cursor track + ingest integration | Cursor effects work on desktop recordings |
| 324 | Installers, auto-update, code signing | Signed installer updates itself |
| 325 | ADR: mobile recording apps | ADR accepted |
| 326 | iOS screen recording (broadcast extension) + upload | iPhone recording plays on Sintade |
| 327 | Android screen recording + upload | Android recording plays on Sintade |
| 328 | App store releases | Both apps published |
| 329 | ADR: live streaming (WebRTC ingest → low-latency HLS) | ADR accepted |
| 330 | Live ingest server | Stream ingested from browser |
| 331 | Live player + chat | Viewers watch with < 5 s delay |
| 332 | Live stream archived as a recording | Archive plays after stream ends |
| 333 | Live scaling test | 500 concurrent live viewers |
| 334 | Parity audit against the *Future versions* table | Every row delivered or logged |
| 335 | Release | **Demo:** Camtasia-parity release |
