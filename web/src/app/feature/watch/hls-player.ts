import type HlsType from 'hls.js';

/** What the page needs to know about a started player. */
export interface QualityLevel {
  /** The hls.js level index. */
  index: number;
  /** The rung's height, e.g. 720. */
  height: number;
}

export type PlayerMode = 'hls' | 'native-hls' | 'mp4';

export interface PlayerEvents {
  /** The ladder's rungs, lowest first, once the master playlist is read. */
  levels(levels: QualityLevel[]): void;
  /** The rung now being shown (changes by itself in Auto). */
  switched(height: number): void;
  /** Adaptive playback gave up and the MP4 took over. */
  fellBack(): void;
}

export interface PlayerSource {
  /** Same-origin master playlist, once the ladder exists. */
  hlsUrl: string | null | undefined;
  /** The signed MP4 (or preview) every browser can play. */
  url: string;
}

/**
 * Plays a recording on a `<video>`: the adaptive (HLS) ladder where the browser can use it —
 * natively on Safari, through hls.js elsewhere — and the signed MP4 otherwise, or if adaptive
 * playback fails for good (docs/design.md §10 Watch). Switching rung never flushes the buffer, so
 * the picture doesn't stall while a new quality is fetched.
 */
export class VideoPlayer {
  private hls: HlsType | null = null;
  private destroyed = false;
  private retried = false;
  private recoveredMedia = false;
  private mode: PlayerMode = 'mp4';

  private constructor(
    private readonly video: HTMLVideoElement,
    private readonly source: PlayerSource,
    private readonly events: PlayerEvents,
  ) {}

  /** Starts playing `source` on `video` and returns the player that controls it. */
  static async start(
    video: HTMLVideoElement,
    source: PlayerSource,
    events: PlayerEvents,
  ): Promise<VideoPlayer> {
    const player = new VideoPlayer(video, source, events);
    await player.begin();
    return player;
  }

  get playing(): PlayerMode {
    return this.mode;
  }

  /** `-1` is Auto; otherwise a level index from `levels`. */
  setLevel(index: number): void {
    if (this.hls) {
      // `nextLevel` switches at the next fragment boundary and keeps what is buffered.
      this.hls.nextLevel = index;
    }
  }

  destroy(): void {
    this.destroyed = true;
    this.hls?.destroy();
    this.hls = null;
  }

  /**
   * Safari plays HLS better than hls.js can (hardware decode, AirPlay). Chromium now answers
   * `maybe` to the same question but offers no way to pick a rung, so it goes through hls.js.
   */
  private nativeHls(): boolean {
    return (
      /apple/i.test(navigator.vendor) &&
      this.video.canPlayType('application/vnd.apple.mpegurl') !== ''
    );
  }

  private async begin(): Promise<void> {
    const hlsUrl = this.source.hlsUrl;
    if (!hlsUrl) {
      return this.playMp4();
    }
    if (this.nativeHls()) {
      this.mode = 'native-hls';
      this.video.src = hlsUrl;
      return;
    }
    try {
      const { default: Hls } = await import('hls.js');
      if (this.destroyed) {
        return;
      }
      if (!Hls.isSupported()) {
        return this.playMp4();
      }
      this.mode = 'hls';
      this.attach(Hls, hlsUrl);
    } catch {
      this.playMp4();
    }
  }

  private attach(Hls: typeof HlsType, hlsUrl: string): void {
    const hls = new Hls({ startLevel: -1, capLevelToPlayerSize: false });
    this.hls = hls;
    hls.on(Hls.Events.MANIFEST_PARSED, (_event, data) => {
      this.events.levels(
        data.levels
          .map((level, index) => ({ index, height: level.height }))
          .sort((a, b) => a.height - b.height),
      );
    });
    hls.on(Hls.Events.LEVEL_SWITCHED, (_event, data) => {
      const level = hls.levels[data.level];
      if (level) {
        this.events.switched(level.height);
      }
    });
    hls.on(Hls.Events.FRAG_LOADED, () => {
      // Playback is healthy again: a later expiry may re-read the playlists once more.
      this.retried = false;
    });
    hls.on(Hls.Events.ERROR, (_event, data) => {
      if (!data.fatal) {
        return;
      }
      const at = this.video.currentTime;
      if (data.type === Hls.ErrorTypes.NETWORK_ERROR && !this.retried) {
        // Segment URLs expire with the playlist token (15 minutes): read the master again for a
        // fresh token and fresh segment URLs.
        this.retried = true;
        hls.loadSource(hlsUrl);
        hls.startLoad(at);
      } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR && !this.recoveredMedia) {
        this.recoveredMedia = true;
        hls.recoverMediaError();
      } else {
        this.fallBack(at);
      }
    });
    hls.loadSource(hlsUrl);
    hls.attachMedia(this.video);
  }

  private fallBack(at: number): void {
    const wasPlaying = !this.video.paused;
    this.hls?.destroy();
    this.hls = null;
    this.playMp4();
    this.video.addEventListener(
      'loadedmetadata',
      () => {
        this.video.currentTime = at;
        if (wasPlaying) {
          void this.video.play().catch(() => undefined);
        }
      },
      { once: true },
    );
    this.events.fellBack();
  }

  private playMp4(): void {
    this.mode = 'mp4';
    this.video.src = this.source.url;
  }
}
