import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  DestroyRef,
  OnInit,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { Subscription } from 'rxjs';

import { STATUS_STREAM } from '../../core/status-stream.service';
import { PlaybackData, WatchApi, WatchData, WatchHttpError } from '../../core/watch-api.service';
import { formatDuration } from '../recorder/format';
import { QualityLevel, VideoPlayer } from './hls-player';
import { PLAYBACK_SPEEDS, keyAction, seekTarget, stepSpeed, stepVolume } from './player-keys';
import { loadResume, saveResume } from './resume';
import { SpriteCue, cueAt, parseSpriteVtt } from './sprite-preview';

/** How soon a recording that is still processing is checked again (docs/design.md §10 Watch). */
export const STATUS_POLL_MS = 1000;
/** …and the longest wait between checks. */
export const STATUS_POLL_MAX_MS = 8000;

/**
 * The wait before check number `attempt` (0 = the first re-check): quick at first, because a
 * recording is usually watchable within seconds, then slower, so one open tab can't spend the
 * 120-requests-a-minute public watch budget (ADR-0022) while a long recording is processed.
 */
export function pollDelay(attempt: number): number {
  return Math.min(STATUS_POLL_MS * 1.3 ** attempt, STATUS_POLL_MAX_MS);
}

type View =
  | { kind: 'loading' }
  | { kind: 'not-found' }
  | { kind: 'error' }
  | { kind: 'login' }
  | { kind: 'processing'; watch: WatchData }
  | { kind: 'failed'; watch: WatchData }
  | { kind: 'ready'; watch: WatchData; playback: PlaybackData; preview: boolean };

/**
 * The public watch page (`/s/:slug`, docs/design.md §10 Watch): poster, MP4 player with speed,
 * fullscreen and keyboard seek. No session needed; the API decides what this viewer may see
 * and answers `404` for everything else, so "no such link" and "not for you" look the same.
 */
@Component({
  selector: 'app-watch-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink],
  styles: `
    :host {
      display: block;
      max-width: 960px;
      margin: 0 auto;
      padding: var(--space-3);
    }
    .player {
      position: relative;
      background: #000;
      border-radius: var(--radius-md);
      overflow: hidden;
    }
    .player:focus-visible {
      outline: 2px solid var(--color-primary);
    }
    video {
      display: block;
      width: 100%;
      max-height: 80vh;
      background: #000;
    }
    .scrub {
      position: relative;
      height: 14px;
      background: var(--color-surface-raised);
      cursor: pointer;
      touch-action: none;
    }
    .scrub .fill {
      height: 100%;
      background: var(--color-primary);
      opacity: 0.6;
      pointer-events: none;
    }
    .scrub .tip {
      position: absolute;
      bottom: 18px;
      transform: translateX(-50%);
      pointer-events: none;
      background: #000;
      color: #fff;
      border-radius: var(--radius-md);
      overflow: hidden;
      font-size: 0.8rem;
      text-align: center;
    }
    .scrub .tip .tile {
      background-repeat: no-repeat;
    }
    .bar {
      display: flex;
      gap: var(--space-3);
      align-items: center;
      padding: var(--space-2) var(--space-3);
      background: var(--color-surface-raised);
    }
    .meta {
      color: var(--color-text-muted);
    }
  `,
  template: `
    @switch (view().kind) {
      @case ('loading') {
        <p data-testid="watch-loading" role="status" i18n>Loading…</p>
      }
      @case ('not-found') {
        <h1 data-testid="watch-not-found" i18n>This link doesn't work</h1>
        <p i18n>It may have been turned off, expired, or never existed.</p>
      }
      @case ('error') {
        <h1 data-testid="watch-error" i18n>Something went wrong</h1>
        <p i18n>We couldn't load this recording. Check your connection and try again.</p>
        <button type="button" (click)="load()" i18n>Try again</button>
      }
      @case ('login') {
        <h1 data-testid="watch-login" i18n>Sign in to watch</h1>
        <p i18n>This recording is shared with a workspace.</p>
        <p>
          <a routerLink="/login" data-testid="watch-login-link" i18n>Log in</a>
        </p>
      }
    }
    @if (current(); as page) {
      @if (page.kind === 'processing' || page.kind === 'failed' || page.kind === 'ready') {
        <h1 data-testid="watch-title">{{ page.watch.title }}</h1>
        @if (page.watch.duration_ms) {
          <p class="meta" data-testid="watch-duration">{{ duration(page.watch.duration_ms) }}</p>
        }
      }
      @if (page.kind === 'processing') {
        <p data-testid="watch-processing" role="status" i18n>
          This recording is still being processed. It will appear here as soon as it's ready.
        </p>
      }
      @if (page.kind === 'failed') {
        <p data-testid="watch-failed" role="alert" i18n>This recording couldn't be processed.</p>
      }
      @if (page.kind === 'ready') {
        @if (page.preview) {
          <p data-testid="watch-preview" role="status" i18n>
            Preview — the full-quality version is still being prepared. Seeking may be limited until
            then.
          </p>
        }
        <div
          #player
          class="player"
          tabindex="0"
          data-testid="watch-player"
          [attr.data-first-frame-ms]="firstFrameMs()"
          (keydown)="onKey($event)"
        >
          <video
            #video
            data-testid="watch-video"
            controls
            playsinline
            preload="auto"
            [attr.poster]="page.playback.poster_url"
            (loadeddata)="onFirstFrame()"
            (loadedmetadata)="onMetadata()"
            (pause)="onPause()"
            (ended)="onPause()"
            (timeupdate)="onTime()"
          ></video>
          <div
            class="scrub"
            data-testid="watch-scrub"
            role="presentation"
            (pointermove)="onScrubMove($event)"
            (pointerleave)="hoverAt.set(null)"
            (click)="onScrubClick($event)"
          >
            <div class="fill" [style.width.%]="progress() * 100"></div>
            @if (hover(); as tip) {
              <div class="tip" data-testid="watch-scrub-tip" [style.left.%]="tip.fraction * 100">
                @if (tip.cue; as cue) {
                  <div
                    class="tile"
                    data-testid="watch-scrub-tile"
                    [style.width.px]="cue.width"
                    [style.height.px]="cue.height"
                    [style.background-image]="'url(&quot;' + cue.url + '&quot;)'"
                    [style.background-position]="'-' + cue.x + 'px -' + cue.y + 'px'"
                  ></div>
                }
                <div data-testid="watch-scrub-time">{{ duration(tip.timeS * 1000) }}</div>
              </div>
            }
          </div>
          <div class="bar">
            <label>
              <span i18n>Speed</span>
              <select
                data-testid="watch-speed"
                [value]="speed()"
                (change)="setSpeed($any($event.target).value)"
              >
                @for (option of speeds; track option) {
                  <option [value]="option" [selected]="option === speed()">{{ option }}×</option>
                }
              </select>
            </label>
            @if (levels().length > 1) {
              <label>
                <span i18n>Quality</span>
                <select
                  data-testid="watch-quality"
                  [attr.data-current-height]="currentHeight()"
                  (change)="setQuality($any($event.target).value)"
                >
                  <option value="-1" [selected]="quality() === -1" i18n>
                    Auto
                    @if (quality() === -1 && currentHeight()) {
                      ({{ currentHeight() }}p)
                    }
                  </option>
                  @for (level of levels(); track level.index) {
                    <option [value]="level.index" [selected]="quality() === level.index">
                      {{ level.height }}p
                    </option>
                  }
                </select>
              </label>
            }
            <button type="button" data-testid="watch-fullscreen" (click)="toggleFullscreen()" i18n>
              Fullscreen
            </button>
            @if (page.watch.can_download) {
              <button type="button" data-testid="watch-download" (click)="download()" i18n>
                Download
              </button>
            }
            @if (downloadError()) {
              <span role="alert" data-testid="watch-download-error" i18n>
                The download couldn't start. Try again.
              </span>
            }
            <span class="meta" i18n
              >Keys: ← → seek 5 s, J L 10 s, 0–9 jump, &lt; &gt; speed, ↑ ↓ volume, space
              play/pause, F fullscreen, M mute</span
            >
          </div>
        </div>
      }
    }
  `,
})
export class WatchPage implements OnInit {
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(WatchApi);
  private readonly destroyRef = inject(DestroyRef);
  private readonly statusStream = inject(STATUS_STREAM);

  protected readonly speeds = PLAYBACK_SPEEDS;
  protected readonly view = signal<View>({ kind: 'loading' });
  protected readonly speed = signal<number>(1);
  /** Milliseconds from opening the page to the first decoded frame (`loadeddata`). */
  protected readonly firstFrameMs = signal<number | null>(null);
  protected readonly downloadError = signal(false);
  /** The ladder's rungs when the page plays adaptively; empty for a plain MP4. */
  protected readonly levels = signal<QualityLevel[]>([]);
  /** The chosen rung's index, or -1 for Auto. */
  protected readonly quality = signal(-1);
  /** The height of the rung on screen. */
  protected readonly currentHeight = signal<number | null>(null);

  private readonly video = viewChild<ElementRef<HTMLVideoElement>>('video');
  private readonly player = viewChild<ElementRef<HTMLElement>>('player');
  private startedAt = 0;
  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private pollAttempt = 0;
  /** The live status stream while the recording is being made ready. */
  private live: Subscription | null = null;
  /** The stream failed to open or was refused: ask now and then instead. */
  private liveBroken = false;
  /** The MP4's grant, once it exists while the preview is still on screen. */
  private upgrade: PlaybackData | null = null;
  /** Where the video was when it was switched to the MP4. */
  private resume: { at: number; play: boolean } | null = null;

  /** The sprite's thumbnails, once loaded. */
  private readonly cues = signal<SpriteCue[]>([]);
  protected readonly hoverAt = signal<{ fraction: number; timeS: number } | null>(null);
  /** The hover tooltip: where the pointer is and the thumbnail there (once the sprite loaded). */
  protected readonly hover = computed(() => {
    const at = this.hoverAt();
    return at ? { ...at, cue: cueAt(this.cues(), at.timeS) } : null;
  });
  protected readonly progress = signal(0);
  private lastSavedAt = 0;
  private resumed = false;
  private videoPlayer: VideoPlayer | null = null;
  /** The grant the video element is currently set up for. */
  private mounted: PlaybackData | null = null;

  protected current() {
    return this.view();
  }

  constructor() {
    this.destroyRef.onDestroy(() => {
      this.stopPolling();
      this.saveNow();
      this.videoPlayer?.destroy();
    });
    // Hands the video element its source whenever the page has a new grant to play.
    effect(() => {
      const view = this.view();
      const video = this.video()?.nativeElement;
      if (view.kind !== 'ready' || !video || this.mounted === view.playback) {
        return;
      }
      this.mount(video, view.playback);
    });
  }

  private mount(video: HTMLVideoElement, playback: PlaybackData): void {
    this.mounted = playback;
    this.videoPlayer?.destroy();
    this.videoPlayer = null;
    this.levels.set([]);
    this.quality.set(-1);
    this.currentHeight.set(null);
    this.cues.set([]);
    this.hoverAt.set(null);
    this.resumed = false;
    if (playback.sprite_url) {
      this.api
        .sprite(playback.sprite_url)
        .then((text) => {
          if (this.mounted === playback) {
            this.cues.set(parseSpriteVtt(text));
          }
        })
        .catch(() => undefined);
    }
    void VideoPlayer.start(
      video,
      { hlsUrl: playback.hls_url, url: playback.url },
      {
        levels: (levels) => this.levels.set(levels),
        switched: (height) => this.currentHeight.set(height),
        fellBack: () => {
          this.levels.set([]);
          this.currentHeight.set(null);
        },
      },
    ).then((player) => {
      if (this.mounted === playback) {
        this.videoPlayer = player;
      } else {
        player.destroy();
      }
    });
  }

  protected setQuality(raw: string): void {
    const index = Number(raw);
    if (!Number.isInteger(index)) {
      return;
    }
    this.quality.set(index);
    this.videoPlayer?.setLevel(index);
  }

  ngOnInit(): void {
    void this.load();
  }

  protected duration(ms: number): string {
    return formatDuration(ms);
  }

  protected async load(): Promise<void> {
    this.stopPolling();
    this.liveBroken = false;
    this.pollAttempt = 0;
    this.upgrade = null;
    this.startedAt = performance.now();
    this.firstFrameMs.set(null);
    this.view.set({ kind: 'loading' });
    await this.refresh();
  }

  private slug(): string {
    return this.route.snapshot.paramMap.get('slug') ?? '';
  }

  /** Reads the recording's state and picks what to show; keeps polling while it's processing. */
  private async refresh(): Promise<void> {
    const slug = this.slug();
    try {
      const watch = await this.api.watch(slug);
      if (watch.requirement === 'login') {
        this.view.set({ kind: 'login' });
      } else if (watch.state === 'failed') {
        this.view.set({ kind: 'failed', watch });
        this.stopPolling();
      } else if (watch.state === 'processing') {
        await this.whileProcessing(slug, watch);
      } else {
        await this.becameReady(slug, watch);
      }
    } catch (error) {
      const status = error instanceof WatchHttpError ? error.status : 0;
      if (status === 429 && this.view().kind !== 'loading') {
        // Told to slow down: keep what is on screen and ask again later.
        this.schedulePoll();
        return;
      }
      this.view.set({ kind: status === 404 || status === 401 ? 'not-found' : 'error' });
      this.stopPolling();
    }
  }

  /** Processing: play the original if there is one this browser can play, else wait. */
  private async whileProcessing(slug: string, watch: WatchData): Promise<void> {
    const current = this.view();
    if (current.kind === 'ready') {
      // Already previewing: just keep checking for the MP4.
      this.schedulePoll();
      return;
    }
    try {
      const playback = await this.api.playback(slug);
      if (playback.kind === 'preview' && canPlay(playback.content_type)) {
        this.view.set({ kind: 'ready', watch, playback, preview: true });
      } else {
        this.view.set({ kind: 'processing', watch });
      }
    } catch (error) {
      if (error instanceof WatchHttpError && (error.status === 404 || error.status === 401)) {
        throw error;
      }
      this.view.set({ kind: 'processing', watch });
    }
    this.schedulePoll();
  }

  private async becameReady(slug: string, watch: WatchData): Promise<void> {
    const playback = await this.api.playback(slug);
    const current = this.view();
    if (current.kind === 'ready' && current.preview) {
      // Swap from the preview to the MP4 without interrupting someone who is watching.
      this.upgrade = playback;
      this.view.set({ ...current, watch });
      this.applyUpgradeIfIdle();
    } else {
      this.view.set({ kind: 'ready', watch, playback, preview: false });
    }
    this.stopPolling();
  }

  /**
   * Waits for the recording to change: on the live status stream (docs/design.md §9), so the
   * page moves on the moment processing ends; or, if the stream can't be had, by asking again
   * after a growing delay.
   */
  private schedulePoll(): void {
    this.clearTimer();
    if (this.followLive()) {
      return;
    }
    this.pollTimer = setTimeout(() => void this.refresh(), pollDelay(this.pollAttempt));
    this.pollAttempt += 1;
  }

  private followLive(): boolean {
    if (this.liveBroken) {
      return false;
    }
    if (this.live) {
      return true;
    }
    let first = true;
    this.live = this.statusStream.follow(`/s/${encodeURIComponent(this.slug())}/events`).subscribe({
      next: (status) => {
        // The first status is what the page already knows, unless it moved on meanwhile.
        const known = first && status.state === 'processing';
        first = false;
        if (!known) {
          void this.refresh();
        }
      },
      error: () => {
        this.live = null;
        this.liveBroken = true;
        this.schedulePoll();
      },
      complete: () => {
        this.live = null;
      },
    });
    return true;
  }

  private clearTimer(): void {
    if (this.pollTimer !== null) {
      clearTimeout(this.pollTimer);
      this.pollTimer = null;
    }
  }

  private stopPolling(): void {
    this.clearTimer();
    this.live?.unsubscribe();
    this.live = null;
  }

  protected onPause(): void {
    this.saveNow();
    this.applyUpgradeIfIdle();
  }

  protected onTime(): void {
    const video = this.video()?.nativeElement;
    if (!video) {
      return;
    }
    if (Number.isFinite(video.duration) && video.duration > 0) {
      this.progress.set(Math.min(video.currentTime / video.duration, 1));
    }
    const now = performance.now();
    if (now - this.lastSavedAt > 2000) {
      this.saveNow();
    }
  }

  /** Remembers where this viewer is (not for the original preview, which can't seek well). */
  private saveNow(): void {
    const video = this.video()?.nativeElement;
    const current = this.view();
    if (!video || current.kind !== 'ready' || current.preview || !this.resumed) {
      return;
    }
    this.lastSavedAt = performance.now();
    saveResume(this.slug(), video.currentTime, video.duration);
  }

  protected onScrubMove(event: PointerEvent): void {
    const video = this.video()?.nativeElement;
    const bar = event.currentTarget as HTMLElement;
    if (!video || !Number.isFinite(video.duration) || video.duration <= 0) {
      return;
    }
    const box = bar.getBoundingClientRect();
    const fraction = Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1);
    const timeS = fraction * video.duration;
    this.hoverAt.set({ fraction, timeS });
  }

  protected onScrubClick(event: MouseEvent): void {
    const video = this.video()?.nativeElement;
    const bar = event.currentTarget as HTMLElement;
    if (!video || !Number.isFinite(video.duration) || video.duration <= 0) {
      return;
    }
    const box = bar.getBoundingClientRect();
    const fraction = Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1);
    video.currentTime = fraction * video.duration;
  }

  private applyUpgradeIfIdle(): void {
    const video = this.video()?.nativeElement;
    const upgrade = this.upgrade;
    const current = this.view();
    if (!upgrade || current.kind !== 'ready' || (video && !video.paused && !video.ended)) {
      return;
    }
    this.upgrade = null;
    this.resume = video && video.currentTime > 0 ? { at: video.currentTime, play: false } : null;
    this.view.set({ kind: 'ready', watch: current.watch, playback: upgrade, preview: false });
  }

  protected onMetadata(): void {
    const video = this.video()?.nativeElement;
    if (!video) {
      return;
    }
    if (this.resume) {
      video.currentTime = this.resume.at;
      this.resume = null;
    } else if (!this.resumed) {
      const current = this.view();
      const at =
        current.kind === 'ready' && !current.preview
          ? loadResume(this.slug(), video.duration)
          : null;
      if (at !== null) {
        video.currentTime = at;
      }
    }
    // Nothing is saved until the stored position has been applied (or ruled out).
    this.resumed = true;
  }

  protected onFirstFrame(): void {
    if (this.firstFrameMs() === null) {
      this.firstFrameMs.set(Math.round(performance.now() - this.startedAt));
    }
  }

  protected setSpeed(raw: string): void {
    const value = Number(raw);
    if (!PLAYBACK_SPEEDS.some((option) => option === value)) {
      return;
    }
    this.speed.set(value);
    const video = this.video()?.nativeElement;
    if (video) {
      video.playbackRate = value;
    }
  }

  /** Asks for a signed download URL and lets the browser save it (it arrives as an attachment). */
  protected async download(): Promise<void> {
    this.downloadError.set(false);
    try {
      const grant = await this.api.download(this.slug());
      const link = document.createElement('a');
      link.href = grant.url;
      link.download = grant.filename;
      link.rel = 'noopener';
      document.body.appendChild(link);
      link.click();
      link.remove();
    } catch {
      this.downloadError.set(true);
    }
  }

  protected async toggleFullscreen(): Promise<void> {
    const player = this.player()?.nativeElement;
    if (!player) {
      return;
    }
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await player.requestFullscreen?.();
    }
  }

  protected onKey(event: KeyboardEvent): void {
    // Keys typed into the speed menu or a button keep their own meaning.
    const target = event.target as HTMLElement | null;
    if (target && target.tagName !== 'DIV' && target.tagName !== 'VIDEO') {
      return;
    }
    const action = keyAction(event);
    const video = this.video()?.nativeElement;
    if (!action || !video) {
      return;
    }
    event.preventDefault();
    switch (action.kind) {
      case 'seek':
        video.currentTime = seekTarget(video.currentTime, action.deltaS, video.duration);
        break;
      case 'toggle-play':
        if (video.paused) {
          void video.play();
        } else {
          video.pause();
        }
        break;
      case 'fullscreen':
        void this.toggleFullscreen();
        break;
      case 'mute':
        video.muted = !video.muted;
        break;
      case 'seek-to':
        if (Number.isFinite(video.duration)) {
          video.currentTime = video.duration * action.fraction;
        }
        break;
      case 'seek-edge':
        video.currentTime = action.edge === 'start' ? 0 : video.duration || 0;
        break;
      case 'speed':
        this.setSpeed(String(stepSpeed(this.speed(), action.direction)));
        break;
      case 'volume':
        video.volume = stepVolume(video.volume, action.delta);
        video.muted = false;
        break;
    }
  }
}

/** Whether this browser can play a video of `contentType` (a WebM preview in Safari, say, can't). */
function canPlay(contentType: string): boolean {
  const probe = document.createElement('video');
  return typeof probe.canPlayType === 'function' && probe.canPlayType(contentType) !== '';
}
