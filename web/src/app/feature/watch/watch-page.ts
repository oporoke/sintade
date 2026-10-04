import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  OnInit,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { PlaybackData, WatchApi, WatchData, WatchHttpError } from '../../core/watch-api.service';
import { formatDuration } from '../recorder/format';
import { PLAYBACK_SPEEDS, keyAction, seekTarget } from './player-keys';

type View =
  | { kind: 'loading' }
  | { kind: 'not-found' }
  | { kind: 'error' }
  | { kind: 'login' }
  | { kind: 'processing'; watch: WatchData }
  | { kind: 'failed'; watch: WatchData }
  | { kind: 'ready'; watch: WatchData; playback: PlaybackData };

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
          This recording is still being processed.
        </p>
      }
      @if (page.kind === 'failed') {
        <p data-testid="watch-failed" role="alert" i18n>This recording couldn't be processed.</p>
      }
      @if (page.kind === 'ready') {
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
            [src]="page.playback.mp4_url"
            [attr.poster]="page.playback.poster_url"
            (loadeddata)="onFirstFrame()"
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

  protected readonly speeds = PLAYBACK_SPEEDS;
  protected readonly view = signal<View>({ kind: 'loading' });
  protected readonly speed = signal<number>(1);
  /** Milliseconds from opening the page to the first decoded frame (`loadeddata`). */
  protected readonly firstFrameMs = signal<number | null>(null);
  protected readonly downloadError = signal(false);

  private readonly video = viewChild<ElementRef<HTMLVideoElement>>('video');
  private readonly player = viewChild<ElementRef<HTMLElement>>('player');
  private startedAt = 0;

  protected current() {
    return this.view();
  }

  ngOnInit(): void {
    void this.load();
  }

  protected duration(ms: number): string {
    return formatDuration(ms);
  }

  protected async load(): Promise<void> {
    const slug = this.route.snapshot.paramMap.get('slug') ?? '';
    this.startedAt = performance.now();
    this.firstFrameMs.set(null);
    this.view.set({ kind: 'loading' });
    try {
      const watch = await this.api.watch(slug);
      if (watch.requirement === 'login') {
        this.view.set({ kind: 'login' });
      } else if (watch.state === 'processing') {
        this.view.set({ kind: 'processing', watch });
      } else if (watch.state === 'failed') {
        this.view.set({ kind: 'failed', watch });
      } else {
        const playback = await this.api.playback(slug);
        this.view.set({ kind: 'ready', watch, playback });
      }
    } catch (error) {
      const status = error instanceof WatchHttpError ? error.status : 0;
      this.view.set({ kind: status === 404 || status === 401 ? 'not-found' : 'error' });
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
      const grant = await this.api.download(this.route.snapshot.paramMap.get('slug') ?? '');
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
