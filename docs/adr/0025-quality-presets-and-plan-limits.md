# ADR-0025: Quality presets, microphone processing and plan limits in the recorder

## Status

Accepted — 2026-10-05 (Day 74, M9).

## Context

V1 lets the user pick 720p/1080p/4K at 30 or 60 fps and toggle noise suppression, echo
cancellation and automatic gain. §4 says other modules read *limits*, never plan names; the free
tier allows up to 1080p (`kernel::Entitlements::max_resolution`).

## Decision

1. **`capture/quality.ts`** holds the presets (`RESOLUTIONS` 720/1080/2160, `FRAME_RATES` 30/60),
   the bitrate table (5 Mbit/s at 1080p30; 60 fps ×1.6; 720p 3; 4K 16) and `clampQuality`.
   Framework-free; `TakeSession` takes a `quality` and sets `videoBitsPerSecond` and the
   compositor's fps from it.
2. **Capture is asked, not forced.** The height goes to `getDisplayMedia` as an `ideal`
   constraint and is re-applied with `applyConstraints` when the user changes it after picking; a
   small surface keeps its own size. Output size is whatever the browser delivers.
3. **The plan limit reaches the client in `GET /api/v1/me`** (`entitlements.max_resolution`,
   `max_duration_ms` of the current workspace, from `billing`), not through a new route, so no new
   tenant-isolation row is needed. The recorder disables options above the limit **and** clamps at
   use time, so a tampered UI value still records at the plan's height. Unknown limits default to
   the free tier. The server's processing step already caps the MP4 height by the same
   entitlement (Day 46), so a modified client cannot get a taller rendition.
4. **Microphone processing** is passed as `getUserMedia` audio constraints (all on by default, as
   browsers do) and applied by reopening the open mic when a toggle changes.

## Consequences

`max_duration_ms` is also now client-visible, ready for Day 76's auto-stop warning. 4K needs a paid
plan, which does not exist yet (KI-006: every workspace is on the free tier), so the 4K path is
unit-tested but cannot be exercised end to end until plans exist.
