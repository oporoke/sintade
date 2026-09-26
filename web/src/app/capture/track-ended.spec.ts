import { onTrackEnded } from './track-ended';

/** A track whose event delivery can mimic either engine. */
class FakeTrack extends EventTarget {
  onended: ((this: MediaStreamTrack, event: Event) => unknown) | null = null;
  /** Chromium: listeners and the handler property both see the event. */
  endEverywhere(): void {
    const event = new Event('ended');
    this.dispatchEvent(event);
    this.onended?.call(this as unknown as MediaStreamTrack, event);
  }
  /** Playwright's Firefox: only the `onended` property sees it. */
  endViaPropertyOnly(): void {
    this.onended?.call(this as unknown as MediaStreamTrack, new Event('ended'));
  }
}

const asTrack = (fake: FakeTrack) => fake as unknown as MediaStreamTrack;

describe('onTrackEnded', () => {
  it('fires once per event when both delivery paths see it', () => {
    const track = new FakeTrack();
    const handler = vi.fn();
    onTrackEnded(asTrack(track), handler);
    track.endEverywhere();
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('still fires when only the onended property is invoked', () => {
    const track = new FakeTrack();
    const handler = vi.fn();
    onTrackEnded(asTrack(track), handler);
    track.endViaPropertyOnly();
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('chains an existing onended handler and supports two subscribers', () => {
    const track = new FakeTrack();
    const existing = vi.fn();
    track.onended = existing;
    const first = vi.fn();
    const second = vi.fn();
    onTrackEnded(asTrack(track), first);
    onTrackEnded(asTrack(track), second);
    track.endViaPropertyOnly();
    expect([existing, first, second].map((fn) => fn.mock.calls.length)).toEqual([1, 1, 1]);
  });

  it('unsubscribing stops delivery and restores the previous handler', () => {
    const track = new FakeTrack();
    const existing = vi.fn();
    track.onended = existing;
    const handler = vi.fn();
    const unsubscribe = onTrackEnded(asTrack(track), handler);
    unsubscribe();
    track.endEverywhere();
    expect(handler).not.toHaveBeenCalled();
    expect(track.onended).toBe(existing);
  });
});
