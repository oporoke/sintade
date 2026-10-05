# ADR-0026: Discard, restart, shortcuts and the plan-limit warning

## Status

Accepted — 2026-10-05 (Day 76, M9).

## Decision

1. **Discard** stops the take, cancels its uploader (`Uploader.cancel()`: nothing more is queued;
   a chunk already in flight may finish), deletes the take from the device and moves the server
   recording to the trash (`DELETE /recordings/{id}`, the existing route). Trashed recordings do
   not count against the plan's recording limit, so a restart-happy user is not locked out; no new
   route (and no new tenant-isolation row) is needed. A discard asks for confirmation in the page.
2. **Restart** is discard followed by the normal start (new 3-2-1, new take, same shared screen).
3. **Shortcuts** are plain letters while a take is running — `P` pause/resume, `S` stop, `M` mute
   the microphone, `R` restart, `D` discard (both ask first; `Esc` cancels) — ignored with
   Ctrl/Meta/Alt, when typing in a form field, or outside a take. They work when the Sintade tab
   is focused; while recording another window the tab is not, so the on-page buttons remain the
   way (a global shortcut or a Document PiP control window is V2, §3 "Document PiP control
   window").
4. **Plan-limit warning**: from 30 s before the automatic stop (limit minus the 1 s margin) the
   page shows "stops by itself in N s" (5 s steps above 10 s, then every second) in a polite live
   region. The stop itself was Day 41/45 behaviour and is unchanged.

## Consequences

A discarded take's chunks already uploaded stay in storage until the trash purge (30 days). The
shortcut letters are not configurable and not localized (`TODO: Verify` with translated UIs).
