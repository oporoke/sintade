# ADR-0019: Tab recording runs in an offscreen document on the shared capture engine

## Status

Accepted — 2026-10-05 (Day 63, M7).

## Context

A Manifest V3 service worker can't hold a `MediaStream` or run `MediaRecorder`, and it is stopped
after seconds of inactivity. Recording a tab needs `chrome.tabCapture` (a stream id minted for the
tab) and a document that lives for the whole recording.

## Decision

1. **Flow.** The popup's Record click (which grants `activeTab`) sends `start-recording` to the
   service worker. The service worker calls `chrome.tabCapture.getMediaStreamId`, creates the
   offscreen document (reason `USER_MEDIA`, `offscreen.html`) and sends it the stream id and the
   API origin. The offscreen document turns the id into a `MediaStream` with `getUserMedia`
   (`chromeMediaSource: 'tab'`, up to 1080p30), plays the tab's audio back to the user (capturing
   mutes a tab), and hands the stream to a `RecordingController`.
2. **The controller reuses the web recorder's pieces unchanged:** the server creates the recording
   and its take (`POST /recordings`, with the extension's session, ADR-0018), `TakeSession`
   records 2 s chunks into the device's chunk store (OPFS in the extension's origin) and the
   `Uploader` sends each one as it is stored (presign → PUT → ack), then finalizes with the chunk
   count and duration. Afterwards it creates a `link`-visibility share link (the same default as
   the web recorder) and reports the URL. It stops itself just inside the plan's limit and when
   the tab closes (the take simply ends).
3. **The offscreen document is the source of truth for the state** (`idle`, `starting`,
   `recording`, `uploading`, `done`, `error`); it broadcasts every change. The service worker keeps
   the last state in `storage.session` so the popup shows a finished link after it was closed, and
   closes the offscreen document when a recording ends.
4. **Not signed in or offline:** the extension doesn't record offline (the web recorder does,
   with OPFS recovery). The server must accept the recording first; `401`, `402` (plan limit) and
   unreachable are reported in the popup. Chunks of a failed upload stay in the extension's
   store; offering to resume them is future work.
5. **Testing.** Chrome starts tab capture only after a toolbar click. Automated runs build with a
   fixed development key (`SINTADE_DEV_KEY=1`, `extension/dev-key.json`, public key only, never in
   a store build), giving the extension a known id, and start the browser with
   `--allowlisted-extension-id=<id>` and `--auto-accept-this-tab-capture`.

## Consequences

- The extension's recordings are indistinguishable from the web app's on the server and play
  through the same pipeline (preview, then MP4).
- Permissions stay at `activeTab`, `tabCapture`, `offscreen`, `storage` plus the app's origin.
- The offscreen document must reach storage (the presigned `PUT` host). In development that is
  the app's origin (ADR-0010); in production the storage host needs a host permission
  (`TODO: Verify` with the production origin, Day 66).
