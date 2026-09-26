/**
 * Calls `handler` once each time `track` ends; returns an unsubscribe function.
 *
 * Registers through both `addEventListener` and the `onended` property. Under Playwright's
 * Firefox, `ended` events on a `MediaStreamTrack` reach the `onended` handler but not
 * `addEventListener` listeners (Chromium delivers to both), and a missed `ended` would leave a
 * recording running after the user clicked "Stop sharing". Any existing `onended` handler is
 * chained, not replaced, and restored on unsubscribe; the same event never fires `handler` twice.
 */
export function onTrackEnded(track: MediaStreamTrack, handler: () => void): () => void {
  let lastEvent: Event | null = null;
  const once = (event: Event) => {
    if (event === lastEvent) {
      return;
    }
    lastEvent = event;
    handler();
  };
  const previous = track.onended;
  const chained = function (this: MediaStreamTrack, event: Event) {
    previous?.call(this, event);
    once(event);
  };
  track.addEventListener('ended', once);
  track.onended = chained;
  return () => {
    track.removeEventListener('ended', once);
    if (track.onended === chained) {
      track.onended = previous;
    }
  };
}
