import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  Injector,
  afterNextRender,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Subscription } from 'rxjs';

import {
  MicDevice,
  TakeMeta,
  AudioLevels,
  AudioProcessing,
  AudioSourceName,
  BubbleLayout,
  BubbleShape,
  DEFAULT_AUDIO_PROCESSING,
  DEFAULT_QUALITY,
  FRAME_RATES,
  FrameRate,
  QualityPreset,
  RESOLUTIONS,
  Resolution,
  clampQuality,
  TakeSession,
  UploadHttpError,
  UploadProgress,
  Uploader,
  assembleTake,
  selectMimeType,
  toCaptureError,
} from '../../capture';
import { currentBrowser } from '../../core/browser';
import { SHARE_API, shareUrl } from '../../core/share-api.service';
import { ShareDialog } from '../share/share-dialog';
import { CapabilityService } from '../../core/capability.service';
import { CreateRecordingResponse } from '../../core/ingest-api.service';
import {
  AUDIO_MIXER,
  CHUNK_STORE,
  PLAN_LIMITS,
  RECORDINGS_API,
  SOURCE_MANAGER,
  START_TAKE,
} from '../../core/capture.tokens';
import { CaptureProblem, CaptureSource, captureHelp } from './capture-help';
import { Countdown } from './countdown';
import { formatClock, formatDuration } from './format';
import { meterValue } from './meter';
import { micPreference } from './mic-preference';

interface LiveSource {
  label: string;
  detail: string;
}

type Phase = 'setup' | 'countdown' | 'recording' | 'paused' | 'saving' | 'uploading' | 'done';

interface UploadedTake {
  recordingId: string;
  /** From pressing Stop (or the take ending) to the server accepting finalize. */
  stopToFinalizeMs: number;
}

const METER_INTERVAL_MS = 50;
const TIMER_INTERVAL_MS = 250;
/** How far inside the plan's take limit the recorder stops (timer ticks every 250 ms). */
const LIMIT_MARGIN_MS = 1_000;
/**
 * Option value for a microphone listed before permission. Firefox then reports devices with an
 * empty `deviceId`, so the only thing we can ask for is "the browser's microphone".
 */
const ANY_MIC = 'any';

/**
 * The recorder: choose what to share, system audio and microphone (Day 27), check the mic's
 * level and count 3-2-1 (Day 28), then record with pause, resume, stop and a pause-excluding
 * timer (Day 29). Every control is a native button with a visible label, and focus follows the
 * take (to Pause when recording starts, to the result when it ends), so it works by keyboard
 * alone. While it records, every stored chunk is uploaded (Day 39): the recording is created on
 * the server during the countdown, and on stop the page drains the upload queue, finalizes and
 * clears the device's copy. If the server can't be reached at the start, the take is kept on
 * this device instead, as before.
 */
@Component({
  selector: 'app-recorder-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Countdown, ShareDialog],
  styles: `
    .bubble-pad {
      position: relative;
      width: 240px;
      max-width: 100%;
      border: 1px solid currentColor;
      touch-action: none;
      cursor: crosshair;
    }
    .bubble-dot {
      position: absolute;
      transform: translate(-50%, -50%);
      border-radius: 50%;
      background: currentColor;
      opacity: 0.5;
      pointer-events: none;
    }
    .bubble-dot.rounded {
      border-radius: 18%;
    }
  `,
  template: `
    <h1 i18n>New recording</h1>
    @if (viewOnly) {
      <section
        role="note"
        data-testid="recorder-mobile-notice"
        aria-labelledby="recorder-mobile-title"
      >
        <h2 id="recorder-mobile-title" i18n>Recording needs a computer</h2>
        <p i18n>
          Phones and tablets can't record the screen in a browser. Open Sintade in Chrome, Edge,
          Firefox or Safari on a computer to record. You can still watch and share recordings here.
        </p>
      </section>
    } @else {
      <fieldset [disabled]="phase() !== 'setup'" data-testid="recorder-setup">
        <legend i18n>Setup</legend>
        <section aria-labelledby="recorder-screen-heading">
          <h2 id="recorder-screen-heading" i18n>Screen</h2>
          <button type="button" (click)="chooseScreen()" data-testid="recorder-choose-screen" i18n>
            Choose screen, window or tab
          </button>
          @if (display(); as display) {
            <video
              [srcObject]="display"
              autoplay
              muted
              playsinline
              width="480"
              data-testid="recorder-screen-preview"
              i18n-aria-label
              aria-label="Preview of what you are sharing"
            ></video>
            @if (displayInfo(); as info) {
              <p data-testid="recorder-screen-info">{{ info.label }} — {{ info.detail }}</p>
            }
          }

          <p>
            <label>
              <input
                type="checkbox"
                [checked]="systemAudio()"
                [disabled]="!systemAudioSupport().supported"
                (change)="onSystemAudioChange($event)"
                aria-describedby="recorder-system-audio-hint"
                data-testid="recorder-system-audio"
              />
              <span i18n>Include system audio</span>
            </label>
          </p>
          <p id="recorder-system-audio-hint" data-testid="recorder-system-audio-hint">
            {{ systemAudioHint() }}
          </p>
        </section>

        <section aria-labelledby="recorder-quality-heading">
          <h2 id="recorder-quality-heading" i18n>Quality</h2>
          <label>
            <span i18n>Resolution</span>
            <select (change)="onResolutionChange($event)" data-testid="recorder-resolution">
              @for (option of resolutionOptions(); track option.height) {
                <option
                  [value]="option.height"
                  [disabled]="!option.allowed"
                  [selected]="option.height === quality().height"
                >
                  {{ option.label }}{{ option.allowed ? '' : upgradeSuffix }}
                </option>
              }
            </select>
          </label>
          <label>
            <span i18n>Frame rate</span>
            <select (change)="onFrameRateChange($event)" data-testid="recorder-fps">
              @for (fps of frameRates; track fps) {
                <option [value]="fps" [selected]="fps === quality().fps">{{ fps }} fps</option>
              }
            </select>
          </label>
        </section>

        <section aria-labelledby="recorder-camera-heading">
          <h2 id="recorder-camera-heading" i18n>Camera</h2>
          <label>
            <input
              type="checkbox"
              [checked]="camera() !== null"
              (change)="onCameraChange($event)"
              data-testid="recorder-camera"
            />
            <span i18n>Show my camera as a bubble</span>
          </label>
          @if (camera(); as camera) {
            <video
              [srcObject]="camera"
              autoplay
              muted
              playsinline
              width="120"
              data-testid="recorder-camera-preview"
              i18n-aria-label
              aria-label="Preview of your camera"
            ></video>
          }
        </section>

        <section aria-labelledby="recorder-mic-heading">
          <h2 id="recorder-mic-heading" i18n>Microphone</h2>
          <label>
            <span i18n>Microphone</span>
            <select (change)="onMicChange($event)" data-testid="recorder-mic-select">
              <option value="" [selected]="!selectedMic()" i18n>No microphone</option>
              @for (option of micOptions(); track option.value) {
                <option [value]="option.value" [selected]="option.value === selectedMic()">
                  {{ option.label }}
                </option>
              }
            </select>
          </label>
          <fieldset data-testid="recorder-audio-processing">
            <legend i18n>Microphone processing</legend>
            <label>
              <input
                type="checkbox"
                [checked]="audioProcessing().noiseSuppression"
                (change)="onProcessingChange('noiseSuppression', $event)"
                data-testid="recorder-noise-suppression"
              />
              <span i18n>Noise suppression</span>
            </label>
            <label>
              <input
                type="checkbox"
                [checked]="audioProcessing().echoCancellation"
                (change)="onProcessingChange('echoCancellation', $event)"
                data-testid="recorder-echo-cancellation"
              />
              <span i18n>Echo cancellation</span>
            </label>
            <label>
              <input
                type="checkbox"
                [checked]="audioProcessing().autoGainControl"
                (change)="onProcessingChange('autoGainControl', $event)"
                data-testid="recorder-auto-gain"
              />
              <span i18n>Automatic gain</span>
            </label>
          </fieldset>
        </section>
      </fieldset>

      @if (micInfo(); as mic) {
        <p data-testid="recorder-mic-info">{{ mic.label }} — {{ mic.detail }}</p>
        <p>
          <label for="recorder-mic-meter" i18n>Level</label>
          <meter
            id="recorder-mic-meter"
            min="0"
            max="1"
            low="0.02"
            high="0.6"
            [value]="micLevel()"
            data-testid="recorder-mic-meter"
          ></meter>
          <span data-testid="recorder-mic-level" hidden>{{ micLevel().toFixed(3) }}</span>
        </p>
        @if (phase() === 'setup') {
          <p i18n>Say something: the bar should move.</p>
        }
      }

      @if (soundRows().length > 0) {
        <fieldset data-testid="recorder-sound">
          <legend i18n>Sound</legend>
          @for (row of soundRows(); track row.name) {
            <p>
              <span>{{ row.label }}</span>
              <label>
                <input
                  type="checkbox"
                  [checked]="row.muted"
                  (change)="onMute(row.name, $event)"
                  [attr.data-testid]="'recorder-mute-' + row.name"
                />
                <span i18n>Mute</span>
              </label>
              <label>
                <span i18n>Volume</span>
                <input
                  type="range"
                  min="0"
                  max="2"
                  step="0.05"
                  [value]="row.volume"
                  (input)="onVolume(row.name, $event)"
                  [attr.data-testid]="'recorder-volume-' + row.name"
                />
              </label>
              <meter
                min="0"
                max="1"
                low="0.02"
                high="0.6"
                [value]="row.level"
                [attr.aria-label]="row.label"
                [attr.data-testid]="'recorder-level-' + row.name"
              ></meter>
            </p>
          }
        </fieldset>
      }

      @if (phase() === 'setup' || phase() === 'countdown') {
        <button
          type="button"
          (click)="start()"
          [disabled]="!display() || phase() === 'countdown'"
          data-testid="recorder-start"
          i18n
        >
          Start recording
        </button>
        @if (!display()) {
          <p i18n>Choose what to share first.</p>
        }
      }
      @if (limitNotice(); as notice) {
        <p role="status" data-testid="recorder-limit-notice">{{ notice }}</p>
      }
      <app-countdown #countdownRef />

      @if (isRecording()) {
        <section aria-label="Recording controls" i18n-aria-label data-testid="recorder-controls">
          <p role="status" data-testid="recorder-status">
            {{ phase() === 'paused' ? pausedLabel : recordingLabel }}
          </p>
          <p
            role="timer"
            aria-label="Recording time"
            i18n-aria-label
            data-testid="recorder-timer"
            [attr.data-elapsed-ms]="elapsedMs()"
          >
            {{ clock() }}
          </p>
          <button
            #pauseButton
            type="button"
            (click)="togglePause()"
            [disabled]="phase() === 'saving'"
            data-testid="recorder-pause"
          >
            {{ phase() === 'paused' ? resumeLabel : pauseLabel }}
          </button>
          <button
            type="button"
            (click)="stop()"
            [disabled]="phase() === 'saving'"
            data-testid="recorder-stop"
            i18n
          >
            Stop recording
          </button>
          @if (phase() === 'saving') {
            <p role="status" i18n>Saving…</p>
          }
          @if (bubble(); as layout) {
            <fieldset data-testid="recorder-bubble-controls">
              <legend i18n>Camera bubble</legend>
              <div
                class="bubble-pad"
                data-testid="recorder-bubble-pad"
                [style.aspect-ratio]="padAspect()"
                (pointerdown)="onPadPointer($event)"
                (pointermove)="onPadPointer($event)"
              >
                <span
                  class="bubble-dot"
                  data-testid="recorder-bubble-dot"
                  [class.rounded]="layout.shape === 'rounded'"
                  [style.left.%]="layout.cx * 100"
                  [style.top.%]="layout.cy * 100"
                  [style.height.%]="layout.size * 100"
                  [style.aspect-ratio]="padAspectOne()"
                ></span>
              </div>
              <label>
                <span i18n>Size</span>
                <input
                  type="range"
                  min="0.1"
                  max="0.6"
                  step="0.01"
                  [value]="layout.size"
                  (input)="onBubbleSize($event)"
                  data-testid="recorder-bubble-size"
                />
              </label>
              <label>
                <span i18n>Shape</span>
                <select (change)="onBubbleShape($event)" data-testid="recorder-bubble-shape">
                  <option value="circle" [selected]="layout.shape === 'circle'" i18n>Circle</option>
                  <option value="rounded" [selected]="layout.shape === 'rounded'" i18n>
                    Rounded square
                  </option>
                </select>
              </label>
              <label>
                <input
                  type="checkbox"
                  [checked]="layout.cameraOnly"
                  (change)="onCameraOnly($event)"
                  data-testid="recorder-camera-only"
                />
                <span i18n>Camera only (hide the screen)</span>
              </label>
            </fieldset>
          }
        </section>
      }

      @if (phase() === 'uploading') {
        <section aria-labelledby="recorder-uploading-heading" data-testid="recorder-uploading">
          <h2 id="recorder-uploading-heading" i18n>Uploading</h2>
          @if (upload(); as progress) {
            <progress
              [max]="progress.queued || 1"
              [value]="progress.uploaded"
              aria-labelledby="recorder-uploading-heading"
            ></progress>
            <p
              role="status"
              data-testid="recorder-upload-status"
              [attr.data-state]="progress.state"
            >
              {{ uploadStatusText(progress) }}
            </p>
          }
        </section>
      }

      @if (phase() === 'done' && result(); as take) {
        <section aria-labelledby="recorder-done-heading" data-testid="recorder-done">
          <h2 #doneHeading id="recorder-done-heading" tabindex="-1">
            {{ uploaded() ? uploadedHeading : savedHeading }}
          </h2>
          <p
            data-testid="recorder-done-summary"
            [attr.data-duration-ms]="take.durationMs"
            [attr.data-chunk-count]="take.chunkCount"
            [attr.data-take-id]="take.takeId"
            [attr.data-uploaded]="uploaded() ? 'true' : 'false'"
            [attr.data-recording-id]="uploaded()?.recordingId"
            [attr.data-stop-to-finalize-ms]="uploaded()?.stopToFinalizeMs"
          >
            {{ savedSummary(take) }}
          </p>
          @if (uploadNotice(); as notice) {
            <p data-testid="recorder-upload-notice">{{ notice }}</p>
          }
          @if (uploaded(); as done) {
            @if (shareLink(); as link) {
              <p
                data-testid="recorder-share-link"
                [attr.data-copied]="link.copied ? 'true' : 'false'"
              >
                @if (link.copied) {
                  <span i18n>Link copied:</span>
                } @else {
                  <span i18n>Your link:</span>
                }
                <a [href]="link.url" target="_blank" rel="noopener">{{ link.url }}</a>
                @if (!link.copied) {
                  <button
                    type="button"
                    (click)="copyShareLink()"
                    data-testid="recorder-copy-link"
                    i18n
                  >
                    Copy link
                  </button>
                }
              </p>
            } @else if (shareError()) {
              <p data-testid="recorder-share-error">{{ shareError() }}</p>
            }
            <button
              type="button"
              (click)="shareDialog().open(done.recordingId)"
              data-testid="recorder-share"
              i18n
            >
              Share…
            </button>
            <app-share-dialog #shareDialogRef [recordingId]="done.recordingId" />
          }
          @if (downloadUrl(); as url) {
            <a [href]="url" [download]="downloadName()" data-testid="recorder-download" i18n>
              Save a copy
            </a>
          }
          <button type="button" (click)="newRecording()" data-testid="recorder-new" i18n>
            New recording
          </button>
        </section>
      }

      @if (problem(); as problem) {
        <section role="alert" data-testid="recorder-error" aria-labelledby="recorder-problem-title">
          <h2 id="recorder-problem-title" data-testid="recorder-problem-title">
            {{ problem.title }}
          </h2>
          <ol data-testid="recorder-help-steps">
            @for (step of problem.steps; track step) {
              <li>{{ step }}</li>
            }
          </ol>
          <button type="button" (click)="retry()" data-testid="recorder-retry" i18n>
            Try again
          </button>
        </section>
      }
    }
  `,
})
export class RecorderPage {
  private readonly capabilityService = inject(CapabilityService);
  private readonly sources = inject(SOURCE_MANAGER);
  private readonly mixer = inject(AUDIO_MIXER);
  private readonly planLimits = inject(PLAN_LIMITS);
  private readonly chunkStore = inject(CHUNK_STORE);
  private readonly startTake = inject(START_TAKE);
  private readonly recordingsApi = inject(RECORDINGS_API);
  private readonly shareApi = inject(SHARE_API);
  private uploader: Uploader | null = null;
  /** `performance.now()` when Stop was pressed. */
  private stopPressedAt: number | null = null;
  private readonly destroyRef = inject(DestroyRef);
  private readonly injector = inject(Injector);
  private meterSubscription: Subscription | null = null;
  private micsSubscription: Subscription | null = null;
  private session: TakeSession | null = null;
  private sessionSubscriptions: Subscription[] = [];
  private destroyed = false;

  protected readonly countdown = viewChild.required<Countdown>('countdownRef');
  protected readonly shareDialog = viewChild.required<ShareDialog>('shareDialogRef');
  private readonly pauseButton = viewChild<ElementRef<HTMLButtonElement>>('pauseButton');
  private readonly doneHeading = viewChild<ElementRef<HTMLElement>>('doneHeading');

  protected readonly systemAudioSupport = this.capabilityService.systemAudio;
  protected readonly systemAudio = signal(false);
  protected readonly camera = signal<MediaStream | null>(null);
  private readonly requestedQuality = signal<QualityPreset>(DEFAULT_QUALITY);
  /** What is actually recorded: the request, kept within the plan (a free user never gets 4K). */
  protected readonly quality = computed(() =>
    clampQuality(this.requestedQuality(), this.planLimits().maxResolution),
  );
  protected readonly resolutionOptions = computed(() =>
    RESOLUTIONS.map((height) => ({
      height,
      allowed: height <= this.planLimits().maxResolution,
      label: height === 2160 ? '4K' : `${height}p`,
    })),
  );
  protected readonly frameRates = FRAME_RATES;
  protected readonly upgradeSuffix = $localize` — not on your plan`;
  protected readonly audioProcessing = signal<AudioProcessing>(DEFAULT_AUDIO_PROCESSING);
  protected readonly bubble = signal<BubbleLayout | null>(null);
  protected readonly display = signal<MediaStream | null>(null);
  protected readonly displayInfo = signal<LiveSource | null>(null);
  protected readonly mics = signal<MicDevice[]>([]);
  protected readonly selectedMic = signal<string | null>(micPreference.load());
  protected readonly micInfo = signal<LiveSource | null>(null);
  protected readonly micLevel = signal(0);
  protected readonly displayLevel = signal(0);
  private readonly soundSettings = signal({
    mic: { volume: 1, muted: false },
    display: { volume: 1, muted: false },
  });
  /** One row per source that has audio: the mic once open, system audio once shared. */
  protected readonly soundRows = computed(() => {
    const settings = this.soundSettings();
    const rows: {
      name: AudioSourceName;
      label: string;
      volume: number;
      muted: boolean;
      level: number;
    }[] = [];
    if (this.micInfo() || (this.isRecording() && this.sources.currentMic)) {
      rows.push({
        name: 'mic',
        label: $localize`Microphone`,
        ...settings.mic,
        level: this.micLevel(),
      });
    }
    if ((this.display()?.getAudioTracks().length ?? 0) > 0) {
      rows.push({
        name: 'display',
        label: $localize`System audio`,
        ...settings.display,
        level: this.displayLevel(),
      });
    }
    return rows;
  });
  protected readonly problem = signal<CaptureProblem | null>(null);
  private readonly browser = currentBrowser();
  /** Mobile browsers can't record (README §4.1); nor can anything without screen capture. */
  protected readonly viewOnly =
    this.browser.mobile || !this.capabilityService.capabilities().getDisplayMedia;
  protected readonly phase = signal<Phase>('setup');
  protected readonly elapsedMs = signal(0);
  protected readonly result = signal<TakeMeta | null>(null);
  protected readonly downloadUrl = signal<string | null>(null);
  protected readonly upload = signal<UploadProgress | null>(null);
  protected readonly uploaded = signal<UploadedTake | null>(null);
  /** The link created when the recording was uploaded, and whether it reached the clipboard. */
  protected readonly shareLink = signal<{ url: string; copied: boolean } | null>(null);
  protected readonly shareError = signal<string | null>(null);
  protected readonly uploadNotice = signal<string | null>(null);
  /** The plan's limits: recording refused (50 reached) or the take stopped at its length cap. */
  protected readonly limitNotice = signal<string | null>(null);
  protected readonly uploadedHeading = $localize`Recording uploaded`;
  protected readonly savedHeading = $localize`Recording saved`;
  protected readonly anyMic = ANY_MIC;
  /**
   * The selector's options. The remembered mic stays choosable even when the browser hides
   * device ids until permission is granted again (Firefox after a reload).
   */
  protected readonly micOptions = computed(() => {
    const options = this.mics().map((mic) => ({
      value: mic.deviceId || ANY_MIC,
      label: mic.label,
    }));
    const remembered = this.selectedMic();
    if (remembered && !options.some((option) => option.value === remembered)) {
      options.unshift({ value: remembered, label: $localize`Last used microphone` });
    }
    return options;
  });

  protected readonly isRecording = computed(() =>
    ['recording', 'paused', 'saving'].includes(this.phase()),
  );
  protected readonly clock = computed(() => formatClock(this.elapsedMs()));
  protected readonly recordingLabel = $localize`Recording`;
  protected readonly pausedLabel = $localize`Paused`;
  protected readonly pauseLabel = $localize`Pause`;
  protected readonly resumeLabel = $localize`Resume`;

  constructor() {
    this.sources.displayEnded$.pipe(takeUntilDestroyed()).subscribe(() => {
      this.display.set(null);
      this.displayInfo.set(null);
    });
    this.destroyRef.onDestroy(() => {
      this.destroyed = true;
      // Leaving mid-take still saves it (and the journal would recover it anyway).
      void this.session?.stop().catch(() => undefined);
      this.sessionSubscriptions.forEach((subscription) => subscription.unsubscribe());
      this.releaseDownload();
      this.stopMeter();
      this.sources.stopAll();
    });
    // Before permission browsers hide device names, so this first list may be generic; it's
    // refreshed with real names once a microphone has been opened.
    this.sources.listMics().then(
      (mics) => this.mics.set(mics),
      () => undefined,
    );
  }

  protected systemAudioHint(): string {
    const support = this.systemAudioSupport();
    if (!support.supported) {
      return support.reason;
    }
    return support.note ?? $localize`Records the audio of the screen or tab you share.`;
  }

  protected savedSummary(take: TakeMeta): string {
    return this.uploaded()
      ? $localize`${formatDuration(take.durationMs)}:duration: uploaded. Processing has started.`
      : $localize`${formatDuration(take.durationMs)}:duration: saved on this device.`;
  }

  protected uploadStatusText(progress: UploadProgress): string {
    const counts = $localize`${progress.uploaded}:uploaded: of ${progress.queued}:queued: parts uploaded`;
    switch (progress.state) {
      case 'offline':
        return $localize`${counts}:counts:. You're offline: uploading continues when the connection is back. The recording is safe on this device.`;
      case 'retrying':
        return $localize`${counts}:counts:. The connection is struggling; retrying.`;
      case 'finalizing':
        return $localize`All parts uploaded. Finishing…`;
      default:
        return counts;
    }
  }

  protected downloadName(): string {
    const take = this.result();
    const extension = take?.mimeType.includes('mp4') ? 'mp4' : 'webm';
    return `sintade-${take?.takeId ?? 'recording'}.${extension}`;
  }

  onSystemAudioChange(event: Event): void {
    this.systemAudio.set((event.target as HTMLInputElement).checked);
  }

  onResolutionChange(event: Event): void {
    const height = Number((event.target as HTMLSelectElement).value) as Resolution;
    this.requestedQuality.update((q) => ({ ...q, height }));
    void this.sources.applyQuality(this.quality());
  }

  onFrameRateChange(event: Event): void {
    const fps = Number((event.target as HTMLSelectElement).value) as FrameRate;
    this.requestedQuality.update((q) => ({ ...q, fps }));
    void this.sources.applyQuality(this.quality());
  }

  /** The browser applies these when the mic opens, so an open mic is reopened with the new set. */
  async onProcessingChange(name: keyof AudioProcessing, event: Event): Promise<void> {
    const checked = (event.target as HTMLInputElement).checked;
    this.audioProcessing.update((all) => ({ ...all, [name]: checked }));
    const mic = this.selectedMic();
    if (mic && this.micInfo()) {
      await this.openSelectedMic(mic);
    }
  }

  async onCameraChange(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    this.problem.set(null);
    if (!input.checked) {
      this.sources.stopCamera();
      this.camera.set(null);
      return;
    }
    try {
      this.camera.set(await this.sources.openCamera());
    } catch (error) {
      input.checked = false;
      this.showError(error, 'camera');
    }
  }

  protected padAspect(): string {
    const settings = this.display()?.getVideoTracks()[0]?.getSettings();
    return `${settings?.width ?? 16} / ${settings?.height ?? 9}`;
  }

  /** The dot's width follows from its height (a bubble is square in output pixels). */
  protected padAspectOne(): string {
    return '1 / 1';
  }

  /** Dragging on the pad places the bubble's centre under the pointer, live. */
  onPadPointer(event: PointerEvent): void {
    if (event.type === 'pointermove' && event.buttons !== 1) {
      return;
    }
    const pad = event.currentTarget as HTMLElement;
    const box = pad.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) {
      return;
    }
    this.updateBubble({
      cx: (event.clientX - box.left) / box.width,
      cy: (event.clientY - box.top) / box.height,
    });
  }

  onBubbleSize(event: Event): void {
    this.updateBubble({ size: Number((event.target as HTMLInputElement).value) });
  }

  onBubbleShape(event: Event): void {
    this.updateBubble({ shape: (event.target as HTMLSelectElement).value as BubbleShape });
  }

  onCameraOnly(event: Event): void {
    this.updateBubble({ cameraOnly: (event.target as HTMLInputElement).checked });
  }

  private updateBubble(change: Partial<BubbleLayout>): void {
    this.session?.setBubble(change);
    this.bubble.set(this.session?.bubble ?? null);
  }

  onVolume(name: AudioSourceName, event: Event): void {
    const volume = Number((event.target as HTMLInputElement).value);
    this.mixer.setVolume(name, volume);
    this.soundSettings.update((all) => ({ ...all, [name]: { ...all[name], volume } }));
  }

  onMute(name: AudioSourceName, event: Event): void {
    const muted = (event.target as HTMLInputElement).checked;
    this.mixer.setMuted(name, muted);
    this.soundSettings.update((all) => ({ ...all, [name]: { ...all[name], muted } }));
  }

  async chooseScreen(): Promise<void> {
    this.problem.set(null);
    try {
      const stream = await this.sources.pickDisplay({
        systemAudio: this.systemAudio() && this.systemAudioSupport().supported,
        frameRate: this.quality().fps,
        height: this.quality().height,
      });
      this.display.set(stream);
      this.displayInfo.set(describeDisplay(stream));
    } catch (error) {
      this.showError(error, 'screen');
    }
  }

  async onMicChange(event: Event): Promise<void> {
    const deviceId = (event.target as HTMLSelectElement).value || null;
    this.selectedMic.set(deviceId);
    micPreference.save(deviceId);
    this.problem.set(null);
    if (!deviceId) {
      this.stopMeter();
      this.sources.stopMic();
      this.micInfo.set(null);
      return;
    }
    await this.openSelectedMic(deviceId);
  }

  /** Opens the chosen mic (also Try again after a mic problem) and starts the device check. */
  private async openSelectedMic(deviceId: string): Promise<void> {
    try {
      const stream = await this.sources.openMic(
        deviceId === ANY_MIC ? undefined : deviceId,
        this.audioProcessing(),
      );
      const [track] = stream.getAudioTracks();
      // Pin (and remember) the real device, which is known now that permission was granted.
      const actual = track?.getSettings().deviceId;
      if (actual && actual !== deviceId) {
        this.selectedMic.set(actual);
        micPreference.save(actual);
      }
      this.micInfo.set({
        label: track?.label || $localize`Microphone`,
        detail: $localize`working`,
      });
      this.watchMics();
      await this.startMeter(stream);
    } catch (error) {
      this.stopMeter();
      this.micInfo.set(null);
      this.showError(error, 'mic');
    }
  }

  /** 3-2-1 (Esc skips), then the take starts and focus moves to Pause. */
  async start(): Promise<void> {
    const display = this.display();
    if (this.phase() !== 'setup' || !display) {
      return;
    }
    this.problem.set(null);
    this.uploaded.set(null);
    this.uploadNotice.set(null);
    this.upload.set(null);
    this.stopPressedAt = null;
    this.phase.set('countdown');
    const mic = this.sources.currentMic;
    const hasMic = (mic?.getAudioTracks().length ?? 0) > 0;
    const hasSystemAudio = display.getAudioTracks().length > 0;
    // Chosen now so the server knows the format; the mix has audio exactly when a source does.
    const mimeType = selectMimeType(undefined, { audio: hasMic || hasSystemAudio });
    // Created while the countdown runs (§10 Record steps 5–6). Unreachable server: record anyway;
    // the plan's limit reached (402): don't.
    const created: Promise<{ recording: CreateRecordingResponse | null; limitReached: boolean }> =
      mimeType
        ? this.recordingsApi
            .createRecording({
              mime_type: mimeType,
              has_system_audio: hasSystemAudio,
              has_mic: hasMic,
              has_camera: this.camera() !== null,
            })
            .then(
              (recording) => ({ recording, limitReached: false }),
              (error: unknown) => ({
                recording: null,
                limitReached: error instanceof UploadHttpError && error.status === 402,
              }),
            )
        : Promise.resolve({ recording: null, limitReached: false });
    const outcome = await this.countdown().run();
    if (outcome === 'cancelled') {
      this.phase.set('setup');
      return;
    }
    try {
      const { recording, limitReached } = await created;
      if (limitReached) {
        this.phase.set('setup');
        this.limitNotice.set(
          $localize`You've reached your plan's limit on recordings. Delete one to record another.`,
        );
        return;
      }
      this.limitNotice.set(null);
      const maxDurationMs = recording?.max_duration_ms ?? null;
      const store = await this.chunkStore;
      const takeId = recording?.take_id ?? crypto.randomUUID();
      const session = await this.startTake({
        display,
        mic,
        mixer: this.mixer,
        camera: this.sources.currentCamera,
        quality: this.quality(),
        store,
        takeId,
        ...(recording ? { serverTakeId: recording.take_id } : {}),
        ...(mimeType ? { mimeType } : {}),
      });
      this.session = session;
      this.bubble.set(session.bubble);
      this.watchLevelsWhileRecording();
      this.phase.set('recording');
      this.uploader = recording ? new Uploader({ api: this.recordingsApi, store, takeId }) : null;
      const uploader = this.uploader;
      if (!uploader) {
        this.uploadNotice.set(
          $localize`Couldn't reach Sintade when recording started, so this recording is kept on this device. It will be offered for upload the next time you open Sintade.`,
        );
      }
      this.sessionSubscriptions = [
        ...(uploader
          ? [
              session.stored$.subscribe({
                next: (idx) => uploader.enqueue(idx),
                error: () => undefined,
              }),
              uploader.progress$.subscribe((progress) => this.upload.set(progress)),
            ]
          : []),
        session.elapsed$(TIMER_INTERVAL_MS).subscribe((ms) => {
          this.elapsedMs.set(ms);
          // Stop just inside the plan's limit, so finalize accepts the take.
          if (
            maxDurationMs !== null &&
            ms >= maxDurationMs - LIMIT_MARGIN_MS &&
            !this.limitNotice()
          ) {
            this.limitNotice.set(
              $localize`Your plan allows recordings of up to ${formatDuration(maxDurationMs)}:limit:, so this one stopped there.`,
            );
            void this.stop();
          }
        }),
        session.state$.subscribe((state) => {
          if (state === 'paused') this.phase.set('paused');
          else if (state === 'recording') this.phase.set('recording');
          else if (state === 'stopping') this.phase.set('saving');
        }),
      ];
      // Ends by Stop or by the browser's "Stop sharing" alike (US-11).
      session.ended.then(
        (take) => this.finish(take),
        (error: unknown) => {
          this.phase.set('setup');
          this.showError(error, 'screen');
        },
      );
      this.focusAfterRender(() => this.pauseButton()?.nativeElement);
    } catch (error) {
      this.phase.set('setup');
      this.showError(error, 'screen');
    }
  }

  togglePause(): void {
    if (this.phase() === 'recording') {
      this.session?.pause();
    } else if (this.phase() === 'paused') {
      this.session?.resume();
    }
  }

  async stop(): Promise<void> {
    this.stopPressedAt ??= performance.now();
    await this.session?.stop().catch((error: unknown) => this.showError(error, 'screen'));
  }

  newRecording(): void {
    this.releaseDownload();
    this.result.set(null);
    this.uploaded.set(null);
    this.shareLink.set(null);
    this.shareError.set(null);
    this.uploadNotice.set(null);
    this.limitNotice.set(null);
    this.upload.set(null);
    this.elapsedMs.set(0);
    this.phase.set('setup');
  }

  private async finish(take: TakeMeta): Promise<void> {
    if (this.destroyed) {
      return; // Left the page mid-take: it's saved; there's no view to update.
    }
    const stoppedAt = this.stopPressedAt ?? performance.now();
    this.session = null;
    this.bubble.set(null);
    this.elapsedMs.set(take.durationMs);
    this.result.set(take);
    const uploader = this.uploader;
    this.uploader = null;
    if (uploader && take.chunkCount > 0) {
      this.phase.set('uploading');
      try {
        const { recording_id } = await uploader.finalize(take.chunkCount, take.durationMs);
        this.uploaded.set({
          recordingId: recording_id,
          stopToFinalizeMs: Math.round(performance.now() - stoppedAt),
        });
        void this.shareAndCopy(recording_id);
      } catch {
        this.uploadNotice.set(
          $localize`The upload couldn't be completed. The recording is kept on this device and will be offered for upload the next time you open Sintade.`,
        );
      }
    }
    this.sessionSubscriptions.forEach((subscription) => subscription.unsubscribe());
    this.sessionSubscriptions = [];
    if (this.destroyed) {
      return;
    }
    this.phase.set('done');
    this.focusAfterRender(() => this.doneHeading()?.nativeElement);
    if (this.uploaded()) {
      return; // The device's copy is gone; the server has the recording.
    }
    try {
      const { blob } = await assembleTake(await this.chunkStore, take.takeId);
      this.releaseDownload();
      this.downloadUrl.set(URL.createObjectURL(blob));
    } catch {
      // The take is safe on the device either way; only the convenience download is missing.
    }
  }

  /** The link is ready as soon as the recording is: create it and put it on the clipboard. */
  private async shareAndCopy(recordingId: string): Promise<void> {
    try {
      const link = await this.shareApi.create(recordingId, { visibility: 'link' });
      const url = shareUrl(link.slug);
      this.shareLink.set({ url, copied: await writeClipboard(url) });
    } catch {
      this.shareError.set(
        $localize`The recording is uploaded, but we couldn't create its link. Use Share… to try again.`,
      );
    }
  }

  protected async copyShareLink(): Promise<void> {
    const link = this.shareLink();
    if (link && (await writeClipboard(link.url))) {
      this.shareLink.set({ ...link, copied: true });
    }
  }

  private focusAfterRender(target: () => HTMLElement | undefined): void {
    if (this.destroyed) {
      return;
    }
    afterNextRender(() => target()?.focus(), { injector: this.injector });
  }

  private releaseDownload(): void {
    const url = this.downloadUrl();
    if (url) {
      URL.revokeObjectURL(url);
      this.downloadUrl.set(null);
    }
  }

  /** Device check: meters the open mic (the take's own mix keeps feeding it while recording). */
  private async startMeter(mic: MediaStream): Promise<void> {
    this.stopMeter();
    await this.mixer.mix({ mic });
    this.micLevel.set(0);
    this.meterSubscription = this.mixer
      .levels$(METER_INTERVAL_MS)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((levels) => this.showLevels(levels));
  }

  private showLevels(levels: AudioLevels): void {
    this.micLevel.update((shown) => meterValue(shown, levels.mic));
    this.displayLevel.update((shown) => meterValue(shown, levels.display));
  }

  /** While recording, meters stay live even when no mic was open for the device check. */
  private watchLevelsWhileRecording(): void {
    if (this.meterSubscription) {
      return;
    }
    this.meterSubscription = this.mixer
      .levels$(METER_INTERVAL_MS)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((levels) => this.showLevels(levels));
  }

  private stopMeter(): void {
    this.meterSubscription?.unsubscribe();
    this.meterSubscription = null;
    this.micLevel.set(0);
    this.displayLevel.set(0);
    void this.mixer.close();
  }

  private watchMics(): void {
    if (this.micsSubscription) {
      return;
    }
    this.micsSubscription = this.sources.mics$
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({ next: (mics) => this.mics.set(mics), error: () => undefined });
  }

  /** Try again: re-asks for whichever source failed. */
  async retry(): Promise<void> {
    const problem = this.problem();
    if (problem?.source === 'mic') {
      const deviceId = this.selectedMic();
      if (deviceId) {
        this.problem.set(null);
        await this.openSelectedMic(deviceId);
      }
    } else if (this.phase() === 'setup') {
      await this.chooseScreen();
    }
  }

  private showError(error: unknown, source: CaptureSource): void {
    this.problem.set(captureHelp(toCaptureError(error), source, this.browser));
  }
}

function describeDisplay(stream: MediaStream): LiveSource {
  const [video] = stream.getVideoTracks();
  const settings = video?.getSettings() ?? {};
  const audio =
    stream.getAudioTracks().length > 0 ? $localize`with system audio` : $localize`no system audio`;
  return {
    label: video?.label || $localize`Screen`,
    detail: `${settings.width ?? '?'}×${settings.height ?? '?'}, ${audio}`,
  };
}

/** Browsers may refuse the clipboard outside a user gesture (Safari): report, don't throw. */
async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
