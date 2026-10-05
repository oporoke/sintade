# ADR-0018: The extension uses the browser's own session

## Status

Accepted — 2026-10-05 (Day 62, M7).

## Context

§2 and §5 say the extension has an "auth handoff to the web app session". The extension must call
the API as whoever is signed in to Sintade in that browser. Options were: (a) mint a token for
the extension (a new credential type, storage and revocation to design); (b) read the session
cookies with the `cookies` permission; (c) let the browser attach them.

## Decision

1. **(c).** The manifest holds a host permission for the app's origin
   (`SINTADE_ORIGIN`, build time; dev default `https://localhost:4200`). Requests from the service
   worker with `credentials: 'include'` carry the `HttpOnly` session and refresh cookies, as the
   web app's own requests do; Chrome and Edge both send them to an extension that holds the host
   permission (verified in both, `extension/e2e/handoff.spec.ts`). Nothing is copied or stored:
   sign out of the web app and the extension is signed out on its next call.
2. **CSRF.** The double-submit token needs the `sintade_csrf` cookie to be readable, which would
   take the `cookies` permission, an extra the store review would question (§20 lists four
   permissions). Instead the extension sends `X-Sintade-Client: extension/<version>`, and
   `verify_csrf` accepts that header in place of the double-submit pair. This is OWASP's
   custom-header defence: a cross-site page can't add a custom header without a CORS preflight,
   and the API's CORS policy allows only the app's origin and doesn't list this header, so the
   header proves the request came from an extension page. The double-submit check is unchanged for
   everything else.
3. **Expired access cookie.** The service worker does what the web app does on a `401`: one
   `POST /auth/refresh`, then retries; if that fails, the popup shows "Sign in".
4. **Sign in** opens the web app's login page in a tab; there is no password field in the
   extension.

## Consequences

- No new credential, no token storage, nothing to revoke: logging out everywhere (Day 19) logs
  the extension out too.
- Anyone able to run code in a *different* installed extension that also holds the host
  permission could send the header; extensions with that permission can already call the API with
  the user's cookies, so this adds no capability.
- The production origin is a build-time input (`TODO: Verify`, not decided); Day 66 sets it.
