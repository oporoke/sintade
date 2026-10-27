import { TrimPreview } from './trim-preview';

/** A stand-in `<video>`: a clock that moves only when the test says so. */
class FakeVideo extends EventTarget {
  currentTime = 0;
  paused = true;
  async play(): Promise<void> {
    this.paused = false;
    this.dispatchEvent(new Event('play'));
  }
  pause(): void {
    this.paused = true;
    this.dispatchEvent(new Event('pause'));
  }
}

describe('TrimPreview', () => {
  let frames: FrameRequestCallback[] = [];
  /** Plays `seconds` of video, then shows one animation frame. */
  const advance = (video: FakeVideo, seconds: number) => {
    if (!video.paused) {
      video.currentTime += seconds;
    }
    const due = frames;
    frames = [];
    due.forEach((frame) => frame(0));
  };

  beforeEach(() => {
    frames = [];
    vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => {
      frames.push(frame);
      return frames.length;
    });
    vi.stubGlobal('cancelAnimationFrame', () => undefined);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('plays from the start handle and stops exactly at the end handle', async () => {
    const video = new FakeVideo();
    video.currentTime = 11;
    const preview = new TrimPreview(video as unknown as HTMLVideoElement, {
      startMs: 3_000,
      endMs: 9_000,
    });
    await preview.play();
    expect(video.currentTime).toBe(3);
    for (let i = 0; i < 6 * 60 - 1; i++) {
      advance(video, 1 / 60);
    }
    expect(video.paused).toBe(false);
    // One more frame crosses the end handle (a little past it): paused, put back on it.
    advance(video, 1 / 30);
    expect(video.paused).toBe(true);
    expect(video.currentTime).toBe(9);
    expect(frames).toHaveLength(0);
    preview.detach();
  });

  it('keeps the native play button inside the trim, and follows a moved handle', async () => {
    const video = new FakeVideo();
    const preview = new TrimPreview(video as unknown as HTMLVideoElement, {
      startMs: 2_000,
      endMs: 4_000,
    });
    await video.play();
    expect(video.currentTime).toBe(2);
    preview.setTrim({ startMs: 2_000, endMs: 3_000 });
    advance(video, 1.01);
    expect(video.paused).toBe(true);
    expect(video.currentTime).toBe(3);
    // Playing again from the end handle starts over.
    await video.play();
    expect(video.currentTime).toBe(2);
    preview.detach();
    // Detached: the video is left alone.
    video.pause();
    video.currentTime = 10;
    await video.play();
    expect(video.currentTime).toBe(10);
  });
});
