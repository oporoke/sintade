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
  untracked,
  viewChild,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { EditorApi, EditorHttpError, EditorSource } from '../../core/editor-api.service';
import { formatClock } from '../chapters/chapters-text';
import { formatDuration } from '../recorder/format';
import { SpriteCue, parseSpriteVtt } from '../watch/sprite-preview';
import { fractionToMs, msToFraction, thumbnailCount, thumbnailStrip } from './timeline';
import { Trim, fullTrim, isTrimmed, moveEnd, moveStart } from './trim';
import { TrimPreview } from './trim-preview';
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

/** A step of the arrow keys on a trim handle, and with Shift held. */
const HANDLE_STEP_MS = 100;
const HANDLE_BIG_STEP_MS = 1000;

type Handle = 'start' | 'end';

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
    .track {
      position: relative;
      margin-top: var(--space-3);
    }
    .timeline {
      position: relative;
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
    .cut {
      position: absolute;
      top: 0;
      bottom: 0;
      background: rgb(0 0 0 / 55%);
      pointer-events: none;
    }
    .handle {
      position: absolute;
      top: 0;
      bottom: 0;
      width: 12px;
      cursor: ew-resize;
      touch-action: none;
      background: var(--color-primary);
      border-radius: var(--radius-sm);
    }
    .handle.end {
      transform: translateX(-100%);
    }
    .handle:focus-visible {
      outline: 2px solid var(--color-text);
      outline-offset: 1px;
    }
    .trim-bar {
      display: flex;
      gap: var(--space-2);
      align-items: center;
      margin-top: var(--space-2);
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
        <div class="track">
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
          <div class="cut" [style.left.%]="0" [style.width.%]="startPercent()"></div>
          <div class="cut" [style.left.%]="endPercent()" [style.right.%]="0"></div>
          <div
            class="handle start"
            data-testid="editor-trim-start"
            role="slider"
            tabindex="0"
            aria-label="Start of what is kept"
            i18n-aria-label
            aria-valuemin="0"
            [attr.aria-valuemax]="trim().endMs"
            [attr.aria-valuenow]="trim().startMs"
            [attr.aria-valuetext]="clock(trim().startMs)"
            [style.left.%]="startPercent()"
            (pointerdown)="onHandle($event, 'start')"
            (pointermove)="onHandleMove($event, 'start')"
            (keydown)="onHandleKey($event, 'start')"
          ></div>
          <div
            class="handle end"
            data-testid="editor-trim-end"
            role="slider"
            tabindex="0"
            aria-label="End of what is kept"
            i18n-aria-label
            [attr.aria-valuemin]="trim().startMs"
            [attr.aria-valuemax]="durationMs()"
            [attr.aria-valuenow]="trim().endMs"
            [attr.aria-valuetext]="clock(trim().endMs)"
            [style.left.%]="endPercent()"
            (pointerdown)="onHandle($event, 'end')"
            (pointermove)="onHandleMove($event, 'end')"
            (keydown)="onHandleKey($event, 'end')"
          ></div>
        </div>
        <p class="times">
          <span data-testid="editor-time">{{ clock(currentMs()) }}</span>
          @if (waveformState() === 'pending') {
            <span data-testid="editor-waveform-pending" i18n>Making the waveform…</span>
          }
          <span data-testid="editor-duration">{{ clock(durationMs()) }}</span>
        </p>
        <div class="trim-bar">
          <button type="button" data-testid="editor-preview" (click)="preview()" i18n>
            Preview
          </button>
          <span data-testid="editor-kept" [attr.data-kept-ms]="keptMs()">
            @if (trimmed()) {
              <ng-container i18n
                >Keeps {{ clock(trim().startMs) }}–{{ clock(trim().endMs) }} ({{
                  length(keptMs())
                }})</ng-container
              >
            } @else {
              <ng-container i18n>Keeps the whole recording</ng-container>
            }
          </span>
          @if (trimmed()) {
            <button type="button" data-testid="editor-trim-reset" (click)="resetTrim()" i18n>
              Keep everything
            </button>
          }
        </div>
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
  private draggingHandle: Handle | null = null;
  private trimPreview: TrimPreview | null = null;

  /** What is kept: from the start handle to the end handle, in the original's time. */
  protected readonly trim = signal<Trim>(fullTrim(0));

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
  protected readonly startPercent = computed(
    () => msToFraction(this.trim().startMs, this.durationMs()) * 100,
  );
  protected readonly endPercent = computed(
    () => msToFraction(this.trim().endMs, this.durationMs()) * 100,
  );
  protected readonly keptMs = computed(() => this.trim().endMs - this.trim().startMs);
  protected readonly trimmed = computed(() => isTrimmed(this.trim(), this.durationMs()));
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
    // The preview follows the video element (there is one once the source is loaded) and the
    // handles.
    effect(() => {
      const video = this.video()?.nativeElement;
      if (video && this.trimPreview?.video !== video) {
        this.trimPreview?.detach();
        this.trimPreview = new TrimPreview(video, untracked(this.trim));
      }
    });
    effect(() => this.trimPreview?.setTrim(this.trim()));
    this.destroyRef.onDestroy(() => {
      this.stopRetry();
      this.trimPreview?.detach();
    });
  }

  ngOnInit(): void {
    this.recordingId = this.route.snapshot.paramMap.get('id') ?? '';
    void this.load();
  }

  async load(): Promise<void> {
    this.view.set({ kind: 'loading' });
    try {
      const source = await this.api.source(this.recordingId);
      this.trim.set(fullTrim(source.duration_ms));
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

  protected onHandle(event: PointerEvent, handle: Handle): void {
    // The handle sits over the timeline: grabbing it must not also seek the timeline.
    event.stopPropagation();
    event.preventDefault();
    const target = event.currentTarget as HTMLElement;
    this.draggingHandle = handle;
    this.video()?.nativeElement.pause();
    try {
      target.setPointerCapture(event.pointerId);
    } catch {
      // Not an active pointer (or no capture support): the press still moves the handle.
    }
    const release = () => {
      this.draggingHandle = null;
      target.removeEventListener('pointerup', release);
      target.removeEventListener('pointercancel', release);
    };
    target.addEventListener('pointerup', release);
    target.addEventListener('pointercancel', release);
    target.focus();
    // Grabbing a handle shows its frame.
    this.seek(handle === 'start' ? this.trim().startMs : this.trim().endMs);
  }

  protected onHandleMove(event: PointerEvent, handle: Handle): void {
    const box = this.timeline()?.nativeElement.getBoundingClientRect();
    if (this.draggingHandle !== handle || !box || box.width <= 0) {
      return;
    }
    this.moveHandle(
      handle,
      fractionToMs((event.clientX - box.left) / box.width, this.durationMs()),
    );
  }

  protected onHandleKey(event: KeyboardEvent, handle: Handle): void {
    const at = handle === 'start' ? this.trim().startMs : this.trim().endMs;
    const step = event.shiftKey ? HANDLE_BIG_STEP_MS : HANDLE_STEP_MS;
    const targets: Record<string, number> = {
      ArrowLeft: at - step,
      ArrowRight: at + step,
      Home: 0,
      End: this.durationMs(),
    };
    if (event.key in targets) {
      event.preventDefault();
      event.stopPropagation();
      this.moveHandle(handle, targets[event.key]);
    }
  }

  /** Moves a handle and shows the frame at it at once: the instant preview of the trim. */
  moveHandle(handle: Handle, ms: number): void {
    const duration = this.durationMs();
    const trim =
      handle === 'start'
        ? moveStart(this.trim(), ms, duration)
        : moveEnd(this.trim(), ms, duration);
    this.trim.set(trim);
    this.trimPreview?.setTrim(trim);
    this.seek(handle === 'start' ? trim.startMs : trim.endMs);
  }

  /** Plays what is kept, from the start handle to the end handle. */
  protected preview(): void {
    this.trimPreview?.setTrim(this.trim());
    void this.trimPreview?.play().catch(() => undefined);
  }

  protected resetTrim(): void {
    this.trim.set(fullTrim(this.durationMs()));
  }

  protected length(ms: number): string {
    return formatDuration(ms);
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
