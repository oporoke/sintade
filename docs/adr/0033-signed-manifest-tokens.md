# ADR-0033: A signed token per master playlist

## Status

Accepted — 2026-10-06 (Day 86, M10). Builds on ADR-0030; implements docs/design.md §16 "signed
URLs ≤ 15 minutes" for the HLS ladder. No new dependency (`hmac`, `sha2` and `base64` are already
used by `identity`).

## Context

ADR-0030 serves playlists through the API and signs each segment for 15 minutes. That left two
gaps: a rung playlist could be fetched directly by anyone who held a link the API admits (no
proof that a session had begun at the master), and segment URLs were valid for a full 15 minutes
from whichever playlist fetch minted them, so a URL could be refreshed indefinitely by refetching
one playlist.

## Decision

1. **The master playlist carries a token** (`?t=`) on every rung it names. The token is
   `base64url(recording id ‖ expiry) "." base64url(HMAC-SHA256)`, bound to one recording, valid
   15 minutes from the master fetch. The master itself needs no token: it is gated by the same
   link/visibility decision as `/playback` and is where a session starts.
2. **A rung playlist requires a genuine, unexpired token for its recording**, else `403` (after
   the link check, so a hidden link is still `404` to everyone). Sessions are therefore always
   rooted in a master fetch the link admitted.
3. **Segments are signed only until the token expires** (never longer than 15 minutes, at least
   1 s). Refetching the rung playlist cannot extend them; only a new master fetch does. Once the
   token is gone every segment URL is gone, and the bare object URL (the hotlink) is refused by
   the private bucket.
4. **The key is derived, not the session secret**: `HMAC(session_secret, "sintade-key:" +
   purpose)` with purpose `hls-manifest-v1` (`IdentityService::derive_key`).
5. **The player starts a session again** when playback fails with a network error: it re-reads the
   master (fresh token, fresh segment URLs), once per recovery; after any fragment loads, the
   next expiry may do so again.

## Consequences

- A token is a bearer credential like a presigned URL: someone who is handed a live master URL
  with its token can play for up to 15 minutes. Binding to a viewer (cookie or address) is not
  done; a CDN with signed cookies (Day 148) is the planned answer for production.
- The unit under test for "URLs expire" is the real bucket (`delivery` tests), not a fake clock.
