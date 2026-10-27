import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  OnInit,
  afterNextRender,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { EditorApi, EditorHttpError, EditorSource } from '../../core/editor-api.service';
import { formatClock } from '../chapters/chapters-text';
import { SpriteCue, parseSpriteVtt } from '../watch/sprite-preview';
import { fractionToMs, msToFraction, thumbnailCount, thumbnailStrip } from './timeline';
import { Waveform, drawWaveform } from './waveform';

/** How tall the thumbnail strip and the waveform are, in CSS pixels. */
export const THUMB_HEIGHT_PX = 54;
export const WAVEFORM_HEIGHT_PX = 48;

/** How often the peaks are asked for again while they are being made, and how many times. */
export const WAVEFORM_RETRY_MS = 2000;
export const WAVEFORM_RETRIES = 30;

/** The width used before the timeline has been laid out (and where there is no layout). */
const FALLBACK_WIDTH_PX = 800;

/** A step of the arrow keys on the timeline. */
const KEY_STEP_MS = 1000;

type View =
  | { kind: 'loading' }
  | { kind: 'not-found' }
  | { kind: 'processing' }
  | { kind: 'error' }
  | { kind: 'ready'; source: EditorSource };

/**
 * The editor (`/recordings/:id/edit`, ADR-0040): the original recording above a timeline that
 * spans it exactly, with its thumbnails, its waveform and a playhead. Clicking the timeline seeks.
 * Everything is in the original's time, whichever version viewers get.
 */
@Component({
  selector: 'app-editor-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink],
  styles: `
    :host {
      display: block;
      max-width: 960px;
      margin: 0 auto;
      padding: var(--space-4);
    }
    video {
      width: 100%;
      max-height: 60vh;
      background: #000;
      display: block;
    }
    .timeline {
      position: relative;
      margin-top: var(--space-3);
      cursor: pointer;
      user-select: none;
      touch-action: none;
      border-radius: var(--radius-sm);
      overflow: hidden;
      background: var(--color-surface-raised);
    }
    .timeline:focus-visible {
      outline: 2px solid var(--color-primary);
      outline-offset: 2px;
    }
    .thumbs {
      display: flex;
      height: ${THUMB_HEIGHT_PX}px;
    }
    .thumb {
      flex: 1 1 0;
      overflow: hidden;
      position: relative;
    }
    .thumb > div {
      position: absolute;
      left: 50%;
      top: 0;
      transform-origin: top center;
      background-repeat: no-repeat;
    }
    canvas {
      display: block;
      width: 100%;
      height: ${WAVEFORM_HEIGHT_PX}px;
    }
    .playhead {
      position: absolute;
      top: 0;
      bottom: 0;
      width: 2px;
      margin-left: -1px;
      background: var(--color-danger);
      pointer-events: none;
    }
    .times {
      display: flex;
      justify-content: space-between;
      color: var(--color-text-muted);
      font-variant-numeric: tabular-nums;
      margin: var(--space-1) 0 0;
    }
  `,
  template: `
    <p><a routerLink="/library" i18n>Back to your recordings</a></p>
    <h1 i18n>Edit recording</h1>
    @switch (view().kind) {
      @case ('loading') {
        <p data-testid="editor-loading" i18n>Loading…</p>
      }
      @case ('not-found') {
        <p role="alert" data-testid="editor-not-found" i18n>
          This recording doesn't exist, or you can't edit it.
        </p>
      }
      @case ('processing') {
        <p data-testid="editor-processing" i18n>
          This recording is still being processed. Try again in a moment.
        </p>
      }
      @case ('error') {
        <div role="alert" data-testid="editor-error">
          <p i18n>We couldn't load the editor.</p>
          <button type="button" (click)="load()" i18n>Try again</button>
        </div>
      }
      @case ('ready') {
        <video
          #video
          data-testid="editor-video"
          controls
          preload="metadata"
          playsinline
          [src]="videoUrl()"
          (loadedmetadata)="onTime()"
          (timeupdate)="onTime()"
          (seeked)="onTime()"
        ></video>
        <div
          #timeline
          class="timeline"
          data-testid="editor-timeline"
          role="slider"
          tabindex="0"
          aria-label="Timeline"
          i18n-aria-label
          aria-valuemin="0"
          [attr.aria-valuemax]="durationMs()"
          [attr.aria-valuenow]="currentMs()"
          [attr.aria-valuetext]="clock(currentMs())"
          [attr.data-duration-ms]="durationMs()"
          (pointerdown)="onPointer($event)"
          (pointermove)="onPointerMove($event)"
          (keydown)="onKey($event)"
        >
          @if (strip().length > 0) {
            <div class="thumbs" data-testid="editor-thumbs">
              @for (thumb of strip(); track $index) {
                <div
                  class="thumb"
                  data-testid="editor-thumb"
                  [attr.data-start-ms]="round(thumb.startMs)"
                  [attr.data-end-ms]="round(thumb.endMs)"
                >
                  <div
                    [style.width.px]="thumb.cue.width"
                    [style.height.px]="thumb.cue.height"
                    [style.background-image]="sheet(thumb.cue)"
                    [style.background-position]="-thumb.cue.x + 'px ' + -thumb.cue.y + 'px'"
                    [style.transform]="thumbTransform(thumb.cue)"
                  ></div>
                </div>
              }
            </div>
          }
          <canvas
            #waveform
            data-testid="editor-waveform"
            [attr.data-state]="waveformState()"
            [attr.height]="waveformHeight"
          ></canvas>
          <div
            class="playhead"
            data-testid="editor-playhead"
            [style.left.%]="playheadPercent()"
          ></div>
        </div>
        <p class="times">
          <span data-testid="editor-time">{{ clock(currentMs()) }}</span>
          @if (waveformState() === 'pending') {
            <span data-testid="editor-waveform-pending" i18n>Making the waveform…</span>
          }
          <span data-testid="editor-duration">{{ clock(durationMs()) }}</span>
        </p>
      }
    }
  `,
})
export class EditorPage implements OnInit {
  private readonly api = inject(EditorApi);
  private readonly route = inject(ActivatedRoute);
  private readonly destroyRef = inject(DestroyRef);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  private readonly video = viewChild<ElementRef<HTMLVideoElement>>('video');
  private readonly timeline = viewChild<ElementRef<HTMLElement>>('timeline');
  private readonly canvas = viewChild<ElementRef<HTMLCanvasElement>>('waveform');

  protected readonly view = signal<View>({ kind: 'loading' });
  protected readonly currentMs = signal(0);
  protected readonly waveform = signal<Waveform | null>(null);
  protected readonly waveformState = signal<'pending' | 'ready' | 'unavailable'>('pending');
  protected readonly width = signal(FALLBACK_WIDTH_PX);
  protected readonly waveformHeight = WAVEFORM_HEIGHT_PX;

  private recordingId = '';
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private dragging = false;

  private readonly source = computed(() => {
    const view = this.view();
    return view.kind === 'ready' ? view.source : null;
  });
  protected readonly videoUrl = computed(() => this.source()?.video_url ?? '');
  /** The timeline's length: the original's, as the API measured it. */
  protected readonly durationMs = computed(() => this.source()?.duration_ms ?? 0);
  private readonly cues = computed<SpriteCue[]>(() => {
    const vtt = this.source()?.sprite_vtt;
    return vtt ? parseSpriteVtt(vtt) : [];
  });
  protected readonly strip = computed(() =>
    thumbnailStrip(
      this.cues(),
      this.durationMs(),
      thumbnailCount(this.width(), THUMB_HEIGHT_PX, this.cues()[0]),
    ),
  );
  protected readonly playheadPercent = computed(
    () => msToFraction(this.currentMs(), this.durationMs()) * 100,
  );

  constructor() {
    // Redraw the peaks whenever they arrive or the timeline changes width.
    effect(() => {
      const canvas = this.canvas()?.nativeElement;
      const waveform = this.waveform();
      const width = this.width();
      if (!canvas) {
        return;
      }
      canvas.width = Math.max(1, Math.round(width));
      canvas.height = WAVEFORM_HEIGHT_PX;
      const ctx = canvas.getContext('2d');
      if (ctx && waveform) {
        drawWaveform(ctx, waveform, accent(this.host.nativeElement));
      }
    });
    afterNextRender(() => {
      if (typeof ResizeObserver === 'undefined') {
        return;
      }
      const observer = new ResizeObserver(() => this.measure());
      observer.observe(this.host.nativeElement);
      this.destroyRef.onDestroy(() => observer.disconnect());
    });
    effect(() => {
      // The timeline appears once the source is loaded: measure it then.
      if (this.timeline()) {
        this.measure();
      }
    });
    this.destroyRef.onDestroy(() => this.stopRetry());
  }

  ngOnInit(): void {
    this.recordingId = this.route.snapshot.paramMap.get('id') ?? '';
    void this.load();
  }

  async load(): Promise<void> {
    this.view.set({ kind: 'loading' });
    try {
      const source = await this.api.source(this.recordingId);
      this.view.set({ kind: 'ready', source });
      void this.loadWaveform(0);
    } catch (error) {
      const status = error instanceof EditorHttpError ? error.status : 0;
      this.view.set({
        kind: status === 404 ? 'not-found' : status === 409 ? 'processing' : 'error',
      });
    }
  }

  private async loadWaveform(attempt: number): Promise<void> {
    this.stopRetry();
    try {
      const waveform = await this.api.waveform(this.recordingId);
      if (waveform) {
        this.waveform.set(waveform);
        this.waveformState.set('ready');
        return;
      }
    } catch {
      this.waveformState.set('unavailable');
      return;
    }
    if (attempt + 1 >= WAVEFORM_RETRIES) {
      this.waveformState.set('unavailable');
      return;
    }
    this.retryTimer = setTimeout(() => void this.loadWaveform(attempt + 1), WAVEFORM_RETRY_MS);
  }

  private stopRetry(): void {
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
  }

  private measure(): void {
    const width = this.timeline()?.nativeElement.clientWidth ?? 0;
    this.width.set(width > 0 ? width : FALLBACK_WIDTH_PX);
  }

  protected onTime(): void {
    const video = this.video()?.nativeElement;
    if (video) {
      this.currentMs.set(Math.min(this.durationMs(), Math.round(video.currentTime * 1000)));
    }
  }

  protected onPointer(event: PointerEvent): void {
    const target = event.currentTarget as HTMLElement;
    this.dragging = true;
    try {
      // Keeps the drag on the timeline when the pointer leaves it.
      target.setPointerCapture(event.pointerId);
    } catch {
      // Not an active pointer (or no capture support): clicking still seeks.
    }
    const release = () => {
      this.dragging = false;
      target.removeEventListener('pointerup', release);
      target.removeEventListener('pointercancel', release);
    };
    target.addEventListener('pointerup', release);
    target.addEventListener('pointercancel', release);
    this.seekToPointer(event, target);
  }

  protected onPointerMove(event: PointerEvent): void {
    if (this.dragging) {
      this.seekToPointer(event, event.currentTarget as HTMLElement);
    }
  }

  private seekToPointer(event: PointerEvent, target: HTMLElement): void {
    const box = target.getBoundingClientRect();
    const fraction = box.width > 0 ? (event.clientX - box.left) / box.width : 0;
    this.seek(fractionToMs(fraction, this.durationMs()));
  }

  protected onKey(event: KeyboardEvent): void {
    const steps: Record<string, number> = {
      ArrowLeft: this.currentMs() - KEY_STEP_MS,
      ArrowRight: this.currentMs() + KEY_STEP_MS,
      Home: 0,
      End: this.durationMs(),
    };
    if (event.key in steps) {
      event.preventDefault();
      this.seek(fractionToMs(msToFraction(steps[event.key], this.durationMs()), this.durationMs()));
    }
  }

  /** Puts the video, and the playhead, at `ms` of the original. */
  seek(ms: number): void {
    this.currentMs.set(ms);
    const video = this.video()?.nativeElement;
    if (video) {
      video.currentTime = ms / 1000;
    }
  }

  protected sheet(cue: SpriteCue): string {
    return `url("${cue.url}")`;
  }

  protected thumbTransform(cue: SpriteCue): string {
    return `translateX(-50%) scale(${THUMB_HEIGHT_PX / cue.height})`;
  }

  protected clock(ms: number): string {
    return formatClock(ms);
  }

  protected round(value: number): number {
    return Math.round(value);
  }
}

/** The waveform's colour: the theme's primary, or the text colour where there is none. */
function accent(element: HTMLElement): string {
  if (typeof getComputedStyle === 'undefined') {
    return '#888';
  }
  const style = getComputedStyle(element);
  return (
    style.getPropertyValue('--color-primary').trim() ||
    style.getPropertyValue('--color-text-muted').trim() ||
    '#888'
  );
}
