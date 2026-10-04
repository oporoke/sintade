import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  DestroyRef,
  OnInit,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { PlaybackData, WatchApi, WatchData, WatchHttpError } from '../../core/watch-api.service';
import { formatDuration } from '../recorder/format';
import { PLAYBACK_SPEEDS, keyAction, seekTarget } from './player-keys';

/** How often a recording that is still processing is checked (docs/design.md §10 Watch). */
export const STATUS_POLL_MS = 1000;

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
            [src]="page.playback.url"
            [attr.poster]="page.playback.poster_url"
            (loadeddata)="onFirstFrame()"
            (loadedmetadata)="onMetadata()"
            (pause)="onPause()"
          ></video>
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
            <span class="meta" i18n>Keys: ← → seek 5 s, space play/pause, F fullscreen</span>
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

  protected readonly speeds = PLAYBACK_SPEEDS;
  protected readonly view = signal<View>({ kind: 'loading' });
  protected readonly speed = signal<number>(1);
  /** Milliseconds from opening the page to the first decoded frame (`loadeddata`). */
  protected readonly firstFrameMs = signal<number | null>(null);
  protected readonly downloadError = signal(false);

  private readonly video = viewChild<ElementRef<HTMLVideoElement>>('video');
  private readonly player = viewChild<ElementRef<HTMLElement>>('player');
  private startedAt = 0;
  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  /** The MP4's grant, once it exists while the preview is still on screen. */
  private upgrade: PlaybackData | null = null;
  /** Where the video was when it was switched to the MP4. */
  private resume: { at: number; play: boolean } | null = null;

  protected current() {
    return this.view();
  }

  constructor() {
    this.destroyRef.onDestroy(() => this.stopPolling());
  }

  ngOnInit(): void {
    void this.load();
  }

  protected duration(ms: number): string {
    return formatDuration(ms);
  }

  protected async load(): Promise<void> {
    this.stopPolling();
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
      } else if (watch.state === 'processing') {
        await this.whileProcessing(slug, watch);
      } else {
        await this.becameReady(slug, watch);
      }
    } catch (error) {
      const status = error instanceof WatchHttpError ? error.status : 0;
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

  private schedulePoll(): void {
    this.stopPolling();
    this.pollTimer = setTimeout(() => void this.refresh(), STATUS_POLL_MS);
  }

  private stopPolling(): void {
    if (this.pollTimer !== null) {
      clearTimeout(this.pollTimer);
      this.pollTimer = null;
    }
  }

  protected onPause(): void {
    this.applyUpgradeIfIdle();
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
    if (video && this.resume) {
      video.currentTime = this.resume.at;
      this.resume = null;
    }
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
    }
  }
}

/** Whether this browser can play a video of `contentType` (a WebM preview in Safari, say, can't). */
function canPlay(contentType: string): boolean {
  const probe = document.createElement('video');
  return typeof probe.canPlayType === 'function' && probe.canPlayType(contentType) !== '';
}
