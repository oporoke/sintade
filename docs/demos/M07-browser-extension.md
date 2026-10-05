# M7 — Browser extension demo (Days 61–66)

Date: 2026-10-05
Branch: `feat/day-066-store-submission` (on top of `main` @ `0764000` + the M7 branches)
Environment: **local** (Docker Compose Postgres/MinIO/Mailpit, `api` and `worker` binaries, the
HTTPS dev server), **Chrome 154** and **Microsoft Edge**, both headless under Playwright. There is
no staging VPS (M1 carry-over) and no store account, so the store submission itself was not made.

## Scope

`docs/design.md` §12, weeks 12–13: "MV3 extension for Chrome/Edge: launch from any tab, click
highlights, keystroke overlay, session handoff from web app; store listing."

The demo's headline, from the exit criterion: **the extension starts a recording from any tab with
click highlights visible in the output.**

Built in this milestone:

| Day | What |
| --- | --- |
| 61 | `extension/` project: manifest, service worker, popup, esbuild, loads unpacked in Chrome and Edge (ADR-0017) |
| 62 | The extension uses the browser's own Sintade session; the popup shows who is signed in (ADR-0018) |
| 63 | Tab recording: offscreen document + the shared capture engine + the same ingest; the link plays on Sintade (ADR-0019) |
| 64 | Click highlights and keystroke labels drawn into the recorded tab (ADR-0020) |
| 65 | Popup: sources, three-click recording, link copied, error states (ADR-0021) |
| 66 | Store package, listing, images, privacy-policy page, submission guide |

## How to try it yourself

```bash
just deps-up && just db-migrate
just worker   # terminal 1
just web      # terminal 2 -> https://localhost:4200 (accept the self-signed certificate once)
just api      # terminal 3
just ext-build
```

1. Chrome or Edge → `chrome://extensions` / `edge://extensions` → Developer mode → **Load unpacked**
   → `extension/dist`.
2. Sign in at `https://localhost:4200/login` (sign up first if needed).
3. Open any website. Click the Sintade button in the toolbar. **Expect:** "Signed in as you@…", the
   tab's title, four options (Tab audio ✔, Microphone, Click highlights ✔, Keystrokes).
4. Tick **Keystrokes** if you want them. Click **Record this tab**, click around the page, press a
   shortcut such as Ctrl+K, then **Stop**. **Expect:** "Finishing the upload…" then "Link copied" and
   the link.
5. Paste the link in a private window. **Expect:** the video plays within seconds, with an amber
   ring wherever you clicked and (if on) the key labels along the bottom. Nothing typed into a
   password field appears.

## Results (2026-10-05, automated)

`cd extension && npm run e2e` — 12 browser tests × Chrome and Edge, 68 unit tests.

| Check | Test | Chrome 154 | Edge |
| --- | --- | --- | --- |
| Loads unpacked; MV3; exactly 5 permissions; popup talks to the service worker | `load.spec.ts` | Pass | Pass |
| Popup is signed in as whoever is signed in to the web app; follows sign-out | `handoff.spec.ts` | Pass | Pass |
| State-changing call passes CSRF only with the extension header | `handoff.spec.ts` | Pass | Pass |
| Records a tab through the extension; the link plays on Sintade, showing that tab | `tab-recording.spec.ts` | Pass | Pass |
| Click rings and keystroke label visible in the output MP4; password typing not shown | `overlay.spec.ts` | Pass | Pass |
| Icon → Record → Stop in 3 clicks; timer; link copied | `popup.spec.ts` | Pass | Pass |
| Browser pages named as unrecordable; signed-out error with Sign in; mic blocked / not allowed / allowed | `popup.spec.ts` | Pass | Pass |
| The store zip loads unpacked with the store manifest | `package.spec.ts` | Pass | Pass |

Pixel checks on the finished video (Day 64): amber ring frames found at the click position, none
before the first click; the dark key label found at the bottom, up ~1.5 s only.

## Exit criteria and acceptance criteria

§12 MVP exit criterion: *"Extension installed from the Chrome Web Store starts a recording from any
tab with click highlights visible in the output"* — **not yet**: everything up to the store listing
works and is tested (installed unpacked), but the extension is not in either store. See
`docs/store/SUBMISSION.md`.

## Carry-over

- **Store submission** (Day 66's Check "submission accepted for review") needs the owner's Chrome
  Web Store and Microsoft Partner Center accounts, the production origin, company details and a
  legal review of the privacy policy. The zip, listing, permission justifications, images and the
  `/privacy` page are ready; the guide lists the owner's steps.
- Production origin for the host permission (`SINTADE_ORIGIN`, **TODO: Verify**) and for storage
  uploads if they use a different host.
- **Manual checks** automation can't do: the real toolbar click (grants `activeTab` and tab
  capture), a real microphone together with tab capture, tab audio, Firefox/Safari (not targets).
- Store screenshots regenerated against production (the committed ones show a local address).
- The extension doesn't record offline or resume an interrupted upload (the web recorder does).
- After a cross-origin navigation inside the recorded tab the overlay stops until the next
  recording (the `activeTab` grant is lost).
