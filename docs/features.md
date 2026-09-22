Tags: **[MVP]** = ship first, **[V1]** = needed to compete, **[V2]** = differentiators/scale.

## 1. Capture

**Screen sources**
- [MVP] Full screen, single window, or single browser tab via `getDisplayMedia`
- [MVP] Source picker (browser-native) with a preview before recording starts
- [V1] Multi-monitor awareness (the user picks which display)
- [V1] Region/area crop, done by capturing the full screen and cropping on a canvas or server-side
- [V2] Tab capture with `preferCurrentTab` / `selfBrowserSurface` controls

**Audio sources**
- [MVP] Microphone via `getUserMedia`, with a device selector
- [MVP] System/tab audio via `getDisplayMedia({ audio: true })`
- [MVP] Mixing mic and system audio into one track with the Web Audio API (`AudioContext` → `MediaStreamDestination`)
- [V1] Per-source volume sliders and mute toggles during recording
- [V1] Live input level meters (`AnalyserNode`)
- [V1] Noise suppression, echo cancellation, and auto gain (`getUserMedia` constraints)
- [V2] Separate audio tracks stored per source, for remixing in the editor

**Platform constraints to design around**
- System audio: Chrome/Edge on Windows supports full-screen audio. On macOS/Linux it's tab-only. Firefox and Safari don't support it. Detect this and tell the user.
- Safari: limited `getDisplayMedia`, MP4/H.264 only in `MediaRecorder`, no tab audio.
- Mobile browsers: screen capture isn't supported. Plan for view-only on mobile.

**Webcam**
- [V1] Camera overlay (picture-in-picture bubble), composited on a canvas
- [V1] Bubble shape (circle/rounded), size, and position, draggable during recording
- [V1] Camera-only and screen-only modes
- [V2] Background blur/replacement (MediaPipe / TF.js segmentation)
- [V2] Picture-in-picture floating control window via the Document PiP API

## 2. Recording controls
- [MVP] Start, stop, pause, and resume (`MediaRecorder.pause()`)
- [MVP] 3-2-1 countdown before start
- [MVP] Elapsed-time display
- [MVP] Handle the user clicking the browser's "Stop sharing" button (`track.onended`)
- [V1] Restart/discard the current take
- [V1] Keyboard shortcuts (start/stop/pause)
- [V1] Max-duration limit per plan
- [V1] Quality presets: 720p/1080p/4K, 30/60fps, bitrate
- [V1] Cursor show/hide (`cursor: "always" | "never"`)
- [V2] Drawing/annotation on screen while recording (canvas overlay)
- [V2] Click highlighting and keystroke display. These need a browser extension, since a web page can't see clicks outside itself.
- [V2] Scheduled or timed recordings

## 3. Reliability during recording (critical for trust)
- [MVP] Chunked recording (`MediaRecorder.start(timeslice)`, e.g. 1–5s chunks)
- [MVP] Local persistence of chunks in IndexedDB/OPFS so a crashed tab or closed browser doesn't lose the recording
- [MVP] Recovery flow on next visit: "You have an unfinished recording, upload or discard?"
- [V1] Streaming upload during recording, so chunks go up live and the video is ready almost immediately on stop
- [V1] Resumable uploads with retry and exponential backoff on network loss
- [V1] Disk-quota checks (`navigator.storage.estimate()`) with a warning
- [V1] Wake Lock API to prevent sleep during recording
- [V1] Warn before closing the tab mid-recording (`beforeunload`)

## 4. Upload and ingest (backend)
- [MVP] Multipart/chunked upload endpoint (Axum), or direct-to-object-storage presigned URLs (S3-compatible: MinIO, R2, Wasabi)
- [MVP] Upload session model: `recording_id`, chunk index, checksum, status
- [MVP] Chunk assembly and integrity verification (SHA-256 per chunk)
- [V1] tus protocol or an S3 multipart equivalent for true resumability
- [V1] Deduplication/idempotency on chunk retries
- [V1] Per-user and per-plan storage quotas enforced at ingest

## 5. Processing pipeline
- [MVP] Background job queue (Postgres-backed, e.g. `SKIP LOCKED`, or Redis)
- [MVP] Transcode WebM (VP8/VP9/Opus) to MP4 (H.264/AAC) with FFmpeg for universal playback
- [MVP] Fix WebM duration metadata. `MediaRecorder` WebM files lack duration and cues, which breaks seeking.
- [MVP] Thumbnail/poster generation
- [V1] HLS (or DASH) adaptive bitrate ladder: 360p/720p/1080p
- [V1] Animated preview (GIF/WebP) for link unfurls
- [V1] Sprite sheet for scrubber hover-previews
- [V1] Audio normalization (loudnorm)
- [V1] Processing status streamed to the client (SSE/WebSocket): uploading → processing → ready
- [V2] GPU transcoding workers, autoscaling worker pool
- [V2] Silence detection and auto-trim

## 6. Playback
- [MVP] Web player: play/pause, seek, volume, fullscreen
- [MVP] Playback speed (0.5x–2x)
- [V1] HLS playback (hls.js; native on Safari)
- [V1] Quality selector
- [V1] Captions/subtitles track (WebVTT)
- [V1] Keyboard shortcuts, remembered resume position
- [V1] Thumbnail-sprite hover preview on the scrub bar
- [V1] Chapters/markers
- [V2] Embeddable player (`<iframe>` + oEmbed endpoint)
- [V2] Custom branding on the player (logo, colors) per workspace

## 7. Editing (browser-based)
- [V1] Trim start and end
- [V1] Cut/remove middle segments
- [V1] Non-destructive edits: store an edit decision list (EDL) and render server-side with FFmpeg
- [V1] Title and description
- [V2] Split and merge clips, stitching multiple recordings
- [V2] Text overlays, blur/redact regions (sensitive data)
- [V2] Zoom/pan effects, cursor smoothing
- [V2] Transcript-based editing (delete words to cut video)
- [V2] Replace/re-record segments

## 8. Transcription and AI
- [V1] Auto-transcription (Whisper self-hosted or via API)
- [V1] Generated captions (VTT/SRT) with language selection
- [V1] Searchable transcript with click-to-seek
- [V2] AI summary, auto-title, auto-chapters
- [V2] Filler-word removal ("um", "uh")
- [V2] Translation of captions
- [V2] Q&A over video content

## 9. Sharing and access control
- [MVP] Shareable link (unguessable slug/UUID)
- [MVP] Visibility: private / anyone with link / public
- [V1] Workspace-only visibility
- [V1] Password-protected links
- [V1] Link expiry dates
- [V1] Share with specific emails (invite-only)
- [V1] Download toggle (allow/deny MP4 download)
- [V1] Revoke links, regenerate slug
- [V1] Open Graph/Twitter meta and oEmbed for rich previews in Slack, WhatsApp, etc.
- [V2] Signed, expiring CDN URLs for media segments (prevents hotlinking)
- [V2] Domain-restricted embeds
- [V2] Watermarking (visible or viewer-email overlay)

## 10. Viewer engagement
- [V1] Timestamped comments
- [V1] Emoji reactions at timestamps
- [V1] Threaded replies, @mentions
- [V1] View counts, including unique viewers
- [V2] Viewer list (who watched, how much), with a per-viewer watch-percentage heatmap
- [V2] Call-to-action button/link overlay at the end of the video
- [V2] Anonymous viewer name capture before commenting

## 11. Library and organization
- [MVP] "My recordings" list with thumbnails, duration, and date
- [MVP] Rename, delete
- [V1] Folders/collections
- [V1] Search by title, and by transcript text (Postgres full-text search / `pg_trgm`)
- [V1] Filters and sort (date, duration, views)
- [V1] Soft delete with a trash and 30-day restore
- [V1] Bulk actions: move, delete, change visibility
- [V2] Tags
- [V2] Duplicate/copy video
- [V2] Retention policies (auto-delete after N days)

## 12. Accounts and auth
- [MVP] Email and password signup/login (argon2 hashing)
- [MVP] Email verification, password reset
- [MVP] Sessions/JWT with refresh-token rotation
- [V1] OAuth login (Google, Microsoft, GitHub)
- [V1] Profile: name, avatar
- [V1] Two-factor authentication (TOTP)
- [V1] Active-sessions list with remote logout
- [V2] SSO (SAML/OIDC) for enterprise
- [V2] SCIM provisioning

## 13. Workspaces and teams
- [V1] Workspaces (multi-tenant): a user belongs to one or more
- [V1] Roles: owner, admin, member, viewer (RBAC)
- [V1] Invite by email or link
- [V1] Shared workspace library
- [V2] Team spaces/groups inside a workspace
- [V2] Admin controls: default visibility, disable public links, allowed domains
- [V2] Audit log (who viewed, shared, deleted what)

## 14. Billing and plans
- [V1] Free tier limits: recording length, video count, resolution
- [V1] Paid plans with seat-based pricing
- [V1] Payment integration. Stripe isn't available to Tanzanian merchants, so look at Flutterwave, Pesapal, DPO, Selcom, or mobile money for local customers, and Paddle/Lemon Squeezy as merchant of record for international.
- [V1] Upgrade/downgrade, proration, invoices
- [V1] Usage metering: storage GB, minutes recorded, bandwidth
- [V1] Limit enforcement at both the frontend and API layer
- [V2] Trials, coupons, annual billing
- [V2] Dunning (failed-payment retries and notices)

## 15. Notifications
- [V1] Email: video ready, new comment, new view (first view), mention
- [V1] In-app notification center
- [V1] Notification preferences per user
- [V2] Digest emails (daily/weekly views summary)
- [V2] Web push notifications

## 16. Integrations and API
- [V2] Public REST API with API keys and scopes
- [V2] Webhooks: `recording.ready`, `comment.created`, `view.created`
- [V2] Slack unfurls and posting
- [V2] Browser extension (Chrome/Edge): record from any tab, click highlights, quicker launch
- [V2] Integrations: Jira, GitHub, Notion, Google Drive export
- [V2] Zapier/Make connectors

## 17. Analytics
- [V1] Per-video views, unique viewers, average watch time
- [V2] Drop-off/retention curve per video
- [V2] Workspace-level dashboard (top videos, active creators)
- [V2] Export analytics (CSV)

## 18. Security and compliance
- [MVP] HTTPS everywhere, HSTS
- [MVP] Authorization checks on every media request (not just the page)
- [MVP] Rate limiting (tower-governor), upload size limits
- [MVP] Input validation, CSRF protection, secure cookies
- [V1] Encryption at rest (object storage server-side encryption) and in transit
- [V1] Malware/content-type validation on uploads (verify container/codec with ffprobe)
- [V1] Abuse reporting on public videos, takedown workflow
- [V1] GDPR-style data export and account deletion. Tanzania's PDPA 2022 applies to you locally.
- [V2] Content moderation (automated NSFW detection on public videos)
- [V2] Data residency options, SOC 2 readiness

## 19. Infrastructure and operations
- [MVP] Object storage (S3-compatible) separate from app servers
- [MVP] Postgres: users, workspaces, recordings, uploads, jobs, views, comments
- [MVP] Workers separate from the API (FFmpeg is CPU-heavy)
- [V1] CDN in front of media (Cloudflare, BunnyCDN)
- [V1] Structured logging and tracing (`tracing` crate), metrics (Prometheus), error tracking (Sentry)
- [V1] Health checks, graceful shutdown, job retries and dead-letter queue
- [V1] Storage lifecycle: delete raw chunks after successful transcode, cold tier for old videos
- [V1] Backups: Postgres PITR, object storage versioning
- [V2] Horizontal scaling of API and workers, multi-region storage
- [V2] Cost monitoring per tenant (storage and egress dominate costs)

## 20. UX, accessibility, and client
- [MVP] Permission onboarding: explain mic/screen/camera prompts and handle denials gracefully
- [MVP] Browser capability detection with clear unsupported messages
- [V1] Pre-recording device check (mic test, camera preview)
- [V1] Dark/light theme
- [V1] Responsive viewer pages that work on mobile
- [V1] Accessibility: keyboard navigation, ARIA, captions, contrast
- [V1] PWA install (desktop app feel, launch from the dock)
- [V2] i18n (English and Swahili first)
- [V2] Offline recording with sync later (builds on the IndexedDB chunk store)

## Minimal MVP cut
Screen + mic + system audio capture → chunked local persistence → streaming resumable upload → FFmpeg transcode to MP4 + thumbnail → private/link sharing → basic player → library list → email auth.

Everything else layers on top of that pipeline without rework, provided the chunk/upload/job architecture is right from day one.