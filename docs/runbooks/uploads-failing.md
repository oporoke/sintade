# Runbook: uploads failing

**Symptom:** the recorder shows "offline"/retrying for long; `uploads` errors in logs; creators
report "couldn't be uploaded". Recordings are never lost while the tab/device holds the chunks
(OPFS), so there is time, but not unlimited.

1. **Is it everyone or one user?** One user: their network, a corporate proxy blocking the
   storage host, or the plan limit (`402`). Everyone: continue.
2. **API:** `curl -s https://<origin>/readyz`. The presign/ack routes need the database.
3. **Rate limits:** `429` on `/api/v1/takes/*` means 600 requests a minute for that user were
   exceeded (a runaway client). The `Retry-After` header says when. Normal recording sends ~45
   requests a minute. Look for a loop: logs by `x-request-id`.
4. **Storage reachable from browsers?** A presigned PUT fails with a CORS/TLS error if
   `S3_PUBLIC_ENDPOINT` is wrong, the certificate expired, or the bucket's CORS no longer allows
   the app origin. Check from a laptop: `curl -sI <a presigned URL host>`; look at the browser
   console for `CORS`.
5. **Signature errors (`403` from storage):** clock skew on the API host (`timedatectl`), or the
   storage keys were rotated without restarting the API. Presigned URLs live 5 minutes.
6. **`409` on ack:** a chunk's bytes differ from the one acked earlier: a client bug or two tabs
   recording the same take; not an outage.
7. **Disk or quota on the storage host** (`507 Insufficient Storage`): `disk-full.md`.
