# Store listing — Chrome Web Store and Microsoft Edge Add-ons

Source of truth for every field the two stores ask for. Items marked **TODO: Verify** depend on
decisions not made yet (production address, company details, support contact).

## Basics

| Field | Value |
| --- | --- |
| Name | Sintade |
| Summary (≤ 132 chars; also the manifest `description`) | Record any tab and get a shareable link in seconds. |
| Category | Productivity (Chrome) · Productivity (Edge) |
| Language | English (more with the i18n work) |
| Version | from `extension/package.json` (first submission `0.1.0`) |
| Website | `https://<production origin>` — **TODO: Verify** |
| Support | `[support email]` — **TODO: Verify** |
| Privacy policy URL | `https://<production origin>/privacy` — the page exists (`web/src/app/feature/legal/privacy-page.ts`); its bracketed items and legal wording need review first. **TODO: Verify** |

## Detailed description

> Sintade records the browser tab you choose and gives you a link to the video as soon as you
> stop. No downloads, no waiting for an upload: the recording uploads while you talk.
>
> **Three clicks:** click the Sintade button, press Record, press Stop. The link is copied for you.
>
> • Records the picture and sound of one tab, at up to 1080p.
> • Optional microphone, mixed with the tab's sound.
> • Click highlights, so viewers see where you pointed. Optional keystroke labels for shortcuts;
>   nothing typed into a password, one-time-code or card field is ever shown.
> • Share with anyone who has the link, only people in your workspace, or only you. Turn a link off
>   at any time; allow or block downloads.
> • Watch right away: the video plays seconds after you stop, while it finishes processing.
>
> Needs a free Sintade account. Sintade is built in Tanzania and priced in shillings.
>
> The extension only ever records the tab you chose, after you press Record. It sends the
> recording to your Sintade account and nothing else, runs no analytics and loads no remote code.

## Single purpose (Chrome "Privacy practices")

Record the current browser tab and upload it to the user's Sintade account to get a shareable link.

## Permission justifications

| Permission | Justification |
| --- | --- |
| `activeTab` | Grants access to the tab the user is looking at when they click the extension button, which is the tab they want to record. No other tab is touched. |
| `tabCapture` | The core function: captures the picture and sound of that tab for the recording. |
| `offscreen` | A Manifest V3 service worker cannot run `MediaRecorder` or hold a media stream, so a hidden offscreen document records the tab and uploads it while recording. |
| `scripting` | Injects a small script into the tab being recorded (only after Record is pressed) to draw click highlights and optional keystroke labels so they appear in the video. It never runs otherwise. |
| `storage` | Saves the user's choices (tab audio, microphone, highlights, keystrokes) and the state of the current recording. |
| Host permission `https://<production origin>/*` | Lets the extension use the user's existing Sintade sign-in to create and upload recordings. It cannot read any other site. |

## Data usage disclosure (Chrome "Privacy practices")

| Category | Collected? | Why |
| --- | --- | --- |
| Personally identifiable information | Yes (account email, via the existing Sintade sign-in) | To identify whose account the recording belongs to. |
| Authentication information | The browser's existing Sintade session cookies are used; the extension never reads or stores them or a password. | Signing in. |
| Website content | Yes: the video and audio of the tab the user chooses to record | The product's single purpose. |
| User activity | Click positions and key labels are drawn onto the page being recorded, on the device; they are not sent anywhere except as part of the video. | Click highlights and keystroke labels. |
| Everything else (health, financial, location, web history, personal communications) | No | — |

Certifications to tick: data is **not sold** to third parties; **not used or transferred** for
purposes unrelated to the single purpose; **not used** for creditworthiness or lending.
Remote code: **No** (everything runs from the package; no `eval`, no remote scripts).

## Images (generated from the real extension: `docs/store/assets/`)

| Use | File | Size |
| --- | --- | --- |
| Store icon | `extension/dist/icons/icon-128.png` (packaged in the zip) | 128×128 |
| Screenshot 1 — Record in three clicks | `screenshot-1-record.png` | 1280×800 |
| Screenshot 2 — Uploads while you record | `screenshot-2-recording.png` | 1280×800 |
| Screenshot 3 — Link copied | `screenshot-3-link.png` | 1280×800 |
| Screenshot 4 — The video, seconds later | `screenshot-4-watch.png` | 1280×800 |
| Screenshot 5 — Your library | `screenshot-5-library.png` | 1280×800 |
| Small promo tile | `promo-small-440x280.png` | 440×280 |
| Marquee promo tile (Edge: optional) | `promo-marquee-1400x560.png` | 1400×560 |

Regenerate with `STORE_ASSETS=1 npx playwright test e2e/store-assets.spec.ts --project=chrome`
(needs the app running, as the other extension e2e tests). **Before submitting, regenerate them
against the production app with a real-looking account:** the committed ones show a local address
and a test email.

## Notes for certification (Edge) / reviewer instructions (Chrome)

> Create a free account at `https://<production origin>/signup`, verify the email, then open any
> web page (not a browser page), click the Sintade button, press Record, wait a few seconds, press
> Stop. The popup shows and copies a link that opens the video. A test account can be provided on
> request: `[reviewer account]` — **TODO: Verify**.
