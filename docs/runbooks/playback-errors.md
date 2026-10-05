# Runbook: playback errors (`403`/`404`, video won't start)

1. **`404` on `/s/<slug>`:** by design the answer for a revoked, expired, private-to-others or
   trashed link. Confirm which: `SELECT visibility, revoked_at, expires_at FROM share_links WHERE slug = '…';`
   and `SELECT state, trashed_at FROM recordings WHERE id = '…';`.
2. **`409` on `/playback`:** the recording is still `processing` and has no original yet (the page
   shows a notice and keeps checking). If it never changes, `worker-stuck.md`.
3. **Video element errors / `403` from storage:** signed URLs last 15 minutes. A tab left open
   longer needs a reload. Many at once: clock skew on the API host, or the storage keys changed.
4. **Plays on Chrome, not Safari:** Safari can't play the WebM preview; it waits for the MP4. If the
   MP4 itself fails, check its codec (`ffprobe` on the object: H.264/AAC, fast-start).
5. **Slow start:** first frame target 1.5 s. Check the storage host's latency and whether range
   requests work (`curl -r 0-1023 -I <signed URL>` → `206`).
6. **Download button does nothing:** `GET /s/<slug>/download` needs the link's `allow_download` or
   the owner; `403` means off, `409` means not ready.
