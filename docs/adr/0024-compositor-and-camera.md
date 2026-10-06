# ADR-0024: The compositor — screen plus webcam bubble on a canvas

## Status

Accepted — 2026-10-05 (Day 71, M9).

## Context

§10 Record says: with a webcam, `getDisplayMedia` feeds a Compositor whose canvas `captureStream`
track is recorded; without one, the screen track goes straight to the recorder. V1 adds the
bubble (Days 71–72).

## Decision

1. **`capture/compositor.ts`, framework-free.** `Compositor` draws the screen, then the camera
   clipped to a circle or rounded square, on one canvas and exposes `canvas.captureStream(fps)`'s
   video track. The bubble is a `BubbleLayout` in fractions of the frame (centre, size as a
   fraction of height, shape), clamped inside the frame, so it survives any resolution and Day 72
   can move it with `setBubble`. The camera is cropped to its centre square (`coverSquare`).
2. **Only built when a camera is on.** `TakeSession.start({camera})` creates it; with no camera
   nothing changes (no extra CPU, the screen track is recorded as before). `SourceManager` gained
   `openCamera`/`stopCamera` (video only; the mic stays separate).
3. **The canvas is an `HTMLCanvasElement`, not an `OffscreenCanvas`.** `captureStream` exists only
   on the element; OffscreenCanvas has none. Moving the drawing off the main thread (worker +
   `MediaStreamTrackGenerator`) is a Day 78 option if profiling needs it; the plan's "OffscreenCanvas
   where available" is deferred to that day for this reason.
4. **A timer, not `requestAnimationFrame`**, drives the draw: the recorder tab is normally in the
   background while another window is recorded, and rAF stops there. The scheduler is injectable.
5. **The canvas track never ends by itself**, so `TakeSession` ends the take when the *screen* track
   ends ("Stop sharing") and stops the compositor once the take is stored.
6. Output size is the screen track's reported size (fallback 1280×720).

## Consequences

Browsers may throttle timers in background tabs; capture streams normally exempt the page, but
this is measured on Day 78 (`TODO: Verify` on Safari). Quality presets (Day 74) set the canvas
size and fps.

## Day 72 addendum — live bubble control and camera-only

`BubbleLayout` gained `cameraOnly`; `Compositor.setBubble` clamps (centre inside the frame, size
0.1–0.6 of the height). `TakeSession.setBubble`/`bubble` expose it. The recorder page shows a pad
with the screen's aspect ratio while recording: pointer down/drag places the bubble's centre
live; size slider, shape select and a "camera only" checkbox change it too. Camera-only keeps the
screen shared (it fixes the output size and the take still ends with "Stop sharing") but draws only
the camera, cropped to cover. Keyboard-only bubble placement is not in this day (`TODO: Verify`
accessibility with the Day 76 shortcuts).

## Day 75 addendum — region crop and cursor

The compositor also exists when only a crop is set (`camera` may be null; `TakeSession` builds it
for a camera *or* a region). A region is fractions of the shared screen; the canvas is the
region's size (rounded to even), fixed for the take, and the screen is drawn from the region only
so it fills the frame. The user drags the crop on the setup preview (not changeable while
recording: a moving canvas size would break the encoder). The pointer is hidden with the
`cursor: 'never'` display-capture constraint, which Chromium honours at pick time and others
ignore (`TODO: Verify` Safari/Firefox); the UI says it applies the next time a screen is chosen.
