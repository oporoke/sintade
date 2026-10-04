# ADR-0013: Long takes use a faster x264 preset; the worker runs per-kind job slots

## Status

Accepted — 2026-10-04 (Day 50, M5).

## Context

§10 targets `ProcessTake` at ≤ 0.5x the recording length at 1080p on 4 vCPU with `veryfast`
(§11). Day 50 measured a 30-minute 1080p30 VP9/Opus take (`docs/fixtures/generate-long.sh`,
a deliberately busy `testsrc2` picture) through the MP4 step with `taskset -c 0-3`:

| x264 preset | Wall clock | × recording | Peak RSS |
| --- | --- | --- | --- |
| `veryfast` | 14:58 | 0.50 | 453 MB |
| `superfast` | 10:05 | 0.34 | 407 MB |

`veryfast` met the plan's 15-minute check by two seconds, with no room for a busier worker or
a slower disk. Separately, the worker ran one job at a time and ignored `WORKER_CONCURRENCY`
(deferred since Day 6), so a long transcode blocked emails and sweeps; but two transcodes at
once just slow each other, since FFmpeg already uses every core.

## Decision

1. **Takes longer than 10 minutes are encoded with `-preset superfast`**; everything else keeps
   `veryfast` (§10). The free plan stops at 10 minutes (ADR-0011), so the faster preset only
   applies to paid-plan takes, where files get larger and slightly less efficient rather than
   processing getting late. The length is the recording's (`expected_ms`, or the probed
   duration when the caller doesn't know it).
2. **The worker runs `WORKER_CONCURRENCY` job slots** (default 2), each polling for work, and
   a per-kind limit caps how many run `ProcessTake` at once: `WORKER_PROCESS_TAKE_CONCURRENCY`
   (default 1, never above the slot count). A slot reserves a permit before claiming and
   claims only kinds it may run (`JobQueue::claim_next_of`), so a full kind is left in the
   queue for another worker, never claimed and held.

## Consequences

- A 30-minute take finishes in about a third of its length on 4 vCPU; email and sweep jobs run
  while it transcodes.
- Output size grows for long takes (the busy test picture doubled in size; real screen
  content is far lighter). Revisit with real long recordings, and at M8 with the HLS ladder.
- On a bigger worker, raise `WORKER_CONCURRENCY` and `WORKER_PROCESS_TAKE_CONCURRENCY` together.
- Limits are per worker process; several workers multiply them.
