# Runbook: recordings stuck in `processing`

**Symptom:** a recording stays `processing` well past ~0.5× its length (plus queue wait); the
creator sees "still being processed"; the "ready" email doesn't come.

1. **Is the worker alive?** `docker compose logs --tail=100 worker` / `systemctl status`. A crash
   loop shows in the log. Restart it; jobs are re-claimed (locks expire after 30 s).
2. **What does the queue say?**
   ```sql
   SELECT kind, count(*) FILTER (WHERE done_at IS NULL AND dead_at IS NULL) AS waiting,
          count(*) FILTER (WHERE dead_at IS NOT NULL) AS dead,
          min(run_at) FILTER (WHERE done_at IS NULL AND dead_at IS NULL) AS oldest
   FROM jobs GROUP BY kind;
   ```
   Waiting with an old `oldest` and an idle worker → the job slots are full (`WORKER_CONCURRENCY`,
   `WORKER_PROCESS_TAKE_CONCURRENCY`, ADR-0013) or a job holds a lock: `SELECT id, kind, locked_by, locked_until, attempts, last_error FROM jobs WHERE locked_until > now();`
3. **A dead job** (`dead_at` set): read `last_error`. The recording is `failed` with a reason the
   creator sees; they can press retry, or you can: `POST /api/v1/recordings/{id}/retry` as them,
   or re-queue the job after fixing the cause.
4. **The take's own status:** `SELECT state, last_error, started_at FROM media_jobs WHERE recording_id = '…';`
5. **Disk:** `df -h $WORKER_SCRATCH_DIR` — a full scratch disk makes FFmpeg fail (`disk-full.md`).
   Leftover scratch directories older than 6 h are swept at worker start.
6. **FFmpeg:** a stalled transcode is killed after 120 s without progress or at 3× the recording
   length; the job retries (up to 5 times, exponential backoff).
7. Long recordings are slow by design: a 30-minute 1080p take takes ~10 minutes on 4 vCPU. Check
   the CPU, not the code, first.
