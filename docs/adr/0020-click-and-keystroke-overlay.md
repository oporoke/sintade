# ADR-0020: Click highlights and keystrokes are drawn into the page being recorded

## Status

Accepted — 2026-10-05 (Day 64, M7).

## Context

§4.2 and §20 promise click highlights and a keystroke overlay "visible in the output" for tab
recordings. Tab capture records the tab's own picture, so anything drawn in the page is in the
video.

## Decision

1. **A content script draws the overlay in the recorded tab.** The service worker injects
   `content.js` with `chrome.scripting.executeScript` once the recording has started, and tells it
   what to show (`overlay-start`); it is removed (`overlay-stop`) when the recording ends. The
   script lives in a closed shadow root with `pointer-events: none` at the top z-index, so page
   CSS and scripts can't reach it and it never takes a click. Nothing is injected before the user
   records.
2. **Permissions.** Injection relies on `activeTab` (granted by the click on the extension that
   starts the recording) plus the new `scripting` permission; no host patterns. §20's list becomes
   `activeTab`, `scripting`, `tabCapture`, `offscreen`, `storage`. A tab that can't take a script
   (a browser page, the store) is recorded without the overlay. After a cross-origin navigation
   the grant is lost and the overlay stops until the next recording.
3. **Click highlights** (default on): an amber ring at every `pointerdown`, 700 ms.
4. **Keystrokes** (default **off**; the popup toggle arrives with Day 65): the key or shortcut
   (`Ctrl + K`, `Enter`, `⌘ + S`), up to four at once for 1.5 s. Because they can reveal what
   someone types, nothing typed into a password field, a one-time-code field or a card-number
   field is ever shown, whatever the setting; bare modifier presses and auto-repeat are ignored.
5. **Settings** are two booleans in `chrome.storage.local`, read when a recording starts.
6. **Testing.** A toolbar click can't be automated, so the e2e build (`SINTADE_DEV_KEY=1`) also
   grants a host permission for the test pages (`https://*.example/*`), as it grants the fixed
   key; production manifests have neither.

## Consequences

- The overlay is part of the pixels: it can't be turned off after recording, and it also covers
  what is under it (the keystroke label sits at the bottom centre).
- Pointer events are drawn at the page's coordinates, so they stay aligned with scrolling and
  zoom.
- Window or whole-screen recordings (the web recorder) get no overlay; only the extension's tab
  recordings do.
