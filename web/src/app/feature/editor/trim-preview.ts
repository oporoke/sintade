/**
 * Keeps a `<video>` inside the trim while it plays (ADR-0041): playback that starts outside the
 * kept stretch jumps to the start handle, and the frame that reaches the end handle pauses it and
 * puts it exactly there. Checked on every animation frame: `timeupdate` comes only every ~250 ms,
 * too late to stop at a handle, and a screen recording can go seconds without a new video frame
 * (its frame rate follows what changed on screen), so frame callbacks could be late too.
 */
import { Trim, previewAction } from './trim';

export class TrimPreview {
  private trim: Trim;
  private frame: number | null = null;
  private readonly onPlay = () => {
    this.apply('play');
    this.watch();
  };
  private readonly onStop = () => this.unwatch();

  constructor(
    readonly video: HTMLVideoElement,
    trim: Trim,
  ) {
    this.trim = trim;
    video.addEventListener('play', this.onPlay);
    video.addEventListener('pause', this.onStop);
    video.addEventListener('ended', this.onStop);
  }

  setTrim(trim: Trim): void {
    this.trim = trim;
  }

  /** Plays the kept stretch from the start handle. */
  async play(): Promise<void> {
    this.video.currentTime = this.trim.startMs / 1000;
    await this.video.play();
  }

  detach(): void {
    this.unwatch();
    this.video.removeEventListener('play', this.onPlay);
    this.video.removeEventListener('pause', this.onStop);
    this.video.removeEventListener('ended', this.onStop);
  }

  private apply(event: 'play' | 'frame'): void {
    const action = previewAction(event, this.video.currentTime * 1000, this.trim);
    if (action.kind === 'seek') {
      this.video.currentTime = action.ms / 1000;
    } else if (action.kind === 'stop') {
      this.video.pause();
      this.video.currentTime = this.trim.endMs / 1000;
    }
  }

  private watch(): void {
    this.unwatch();
    const tick = () => {
      this.frame = null;
      if (this.video.paused) {
        return;
      }
      this.apply('frame');
      if (!this.video.paused) {
        this.schedule(tick);
      }
    };
    this.schedule(tick);
  }

  private schedule(tick: () => void): void {
    this.frame = requestAnimationFrame(tick);
  }

  private unwatch(): void {
    if (this.frame === null) {
      return;
    }
    cancelAnimationFrame(this.frame);
    this.frame = null;
  }
}
