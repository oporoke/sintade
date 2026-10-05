# ADR-0021: Popup sources, the microphone permission page, and copying the link

## Status

Accepted — 2026-10-05 (Day 65, M7).

## Context

The popup must take a signed-in user from the toolbar button to a copied link in three clicks
(icon, Record, Stop), offer the sources that make sense for a tab, and explain failures. Three
platform facts shape it: the toolbar popup closes when the browser shows a permission prompt;
an offscreen document can't show one; and the clipboard needs a user gesture (or the
`clipboardWrite` permission, which §20 doesn't list).

## Decision

1. **Sources.** Four checkboxes, saved in `chrome.storage.local` and read when a recording starts:
   *Tab audio* (on), *Microphone* (off), *Click highlights* (on), *Keystrokes* (off, never
   passwords: ADR-0020). Tab audio off still plays the tab's sound to the user (capturing mutes a
   tab) but leaves it out of the recording. The microphone and the tab's sound are mixed by the
   shared `AudioMixer`.
2. **Microphone permission** is asked for once, on a page of the extension (`mic.html`) the popup
   opens when the box is ticked without permission; the page saves the choice when the browser
   allows it. Afterwards the offscreen document opens the microphone without a prompt. A
   microphone that stops working fails the recording up front with "The microphone isn't
   available" and a Try again button, rather than recording without it.
3. **Copying the link.** On Stop the popup hands the clipboard a `ClipboardItem` whose text is a
   promise, resolved with the link when the recording is done: the gesture is the Stop click, the
   content arrives later (supported in Chrome, Edge and Safari). If the popup was closed meanwhile,
   or the limit stopped the recording, it tries on reopening and otherwise shows a Copy link
   button. No `clipboardWrite` permission.
4. **Unrecordable tabs** (browser pages, files, extension stores, tabs the extension isn't allowed
   to see) are named in the popup and Record is disabled, instead of failing after the click.
5. **Errors** map to a way out: not signed in → Sign in; plan limit → Open my library; microphone,
   capture, connection → Try again; upload failed after recording → both.
6. **Click budget.** Toolbar icon (1), Record (2), Stop (3): no countdown, confirmation or
   settings step in between. Defaults make that path work for a signed-in user.
7. **Testing.** `--use-fake-ui-for-media-stream` (answers media prompts) makes Chrome look for a
   fake capture device and breaks tab capture, so it is used only for the one test that needs a
   granted microphone; the popup tests that record use a fake microphone device without it.
   The popup accepts `?tabId=` so tests can aim it at a tab (the toolbar never sets it).

## Consequences

- A signed-in user can record any web page in three clicks and paste the link as soon as the
  upload finishes.
- The popup holds no recording state of its own: it reads the state from the service worker and
  reacts to its events, so closing and reopening it is safe.
