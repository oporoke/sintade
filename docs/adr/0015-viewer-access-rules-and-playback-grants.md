# ADR-0015: Who can open a share link, and how playback is granted

## Status

Accepted — 2026-10-05 (Day 53, M6).

## Context

`GET /s/{slug}` and `/s/{slug}/playback` (§9) are reached by people with no workspace and often no
session. §10 Watch lists the requirements `none | login | password | workspace`, but not what a
viewer who fails them is told, and CLAUDE.md rule 5 says inaccessible private resources are `404`.

## Decision

1. `sharing::decide(link, viewer)` is a pure function over the link's visibility and the viewer
   (anonymous / signed in / workspace member / owner):

   | visibility | anonymous | signed in | member | owner |
   | --- | --- | --- | --- | --- |
   | `private` | `404` | `404` | `404` | allow |
   | `workspace` | login required | `404` | allow | allow |
   | `link`, `public` | allow | allow | allow | allow |

   Only a `workspace` link opened anonymously answers differently (`requirement: "login"` on the
   watch data, `401` on playback), since signing in can change the outcome; a private link never
   admits it exists. Everything else that is a "no" is `404`, identical to a slug that doesn't
   exist (same status and body).
2. The routes live under `/api/v1/s/{slug}[/playback]` with a new `Access::Viewer` class. The
   tenant-isolation harness checks each one is mounted and that a private link of workspace A is
   `404` to an anonymous visitor and to workspace B, but not to A's owner.
3. Only `processing`, `ready` and `failed` recordings are watchable; `recording`, `abandoned` and
   trashed ones read as missing. Playback is `409` until the MP4 exists (Day 59 adds the WebM
   fallback).
4. `delivery::DeliveryService::grant` signs the MP4 and poster with `ObjectStore::presign_get`,
   TTL 15 minutes (§16), reading rendition keys through `media::RenditionReader`. Responses carry
   `Cache-Control: no-store`.
5. Password-protected links and unlock cookies stay V1 (§9).

## Consequences

- The signed URL leaves the API's trust boundary: anyone who obtains it can watch for 15 minutes.
  Revoking a link stops new grants at once; grants already issued run out.
- No per-slug rate limit yet: 71-bit slugs make guessing infeasible, and the API-wide request
  limits apply. Revisit at M8 (hardening).
