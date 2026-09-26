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
  CaptureError,
  MicDevice,
  TakeMeta,
  TakeSession,
  assembleTake,
  toCaptureError,
} from '../../capture';
import { CapabilityService } from '../../core/capability.service';
import { AUDIO_MIXER, CHUNK_STORE, SOURCE_MANAGER, START_TAKE } from '../../core/capture.tokens';
import { Countdown } from './countdown';
import { formatClock, formatDuration } from './format';
import { meterValue } from './meter';
import { micPreference } from './mic-preference';

interface LiveSource {
  label: string;
  detail: string;
}

type Phase = 'setup' | 'countdown' | 'recording' | 'paused' | 'saving' | 'done';

const METER_INTERVAL_MS = 50;
const TIMER_INTERVAL_MS = 250;
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
 * alone. Takes are saved on this device; uploading arrives on Days 37–39.
 */
@Component({
  selector: 'app-recorder-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Countdown],
  template: `
    <h1 i18n>New recording</h1>

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

      <section aria-labelledby="recorder-mic-heading">
        <h2 id="recorder-mic-heading" i18n>Microphone</h2>
        <label>
          <span i18n>Microphone</span>
          <select (change)="onMicChange($event)" data-testid="recorder-mic-select">
            <option value="" [selected]="!selectedMic()" i18n>No microphone</option>
            @for (mic of mics(); track mic.deviceId) {
              <option
                [value]="mic.deviceId || anyMic"
                [selected]="(mic.deviceId || anyMic) === selectedMic()"
              >
                {{ mic.label }}
              </option>
            }
          </select>
        </label>
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
      </section>
    }

    @if (result(); as take) {
      <section aria-labelledby="recorder-done-heading" data-testid="recorder-done">
        <h2 #doneHeading id="recorder-done-heading" tabindex="-1" i18n>Recording saved</h2>
        <p data-testid="recorder-done-summary" [attr.data-duration-ms]="take.durationMs">
          {{ savedSummary(take) }}
        </p>
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

    @if (error()) {
      <p role="alert" data-testid="recorder-error">{{ error() }}</p>
    }
  `,
})
export class RecorderPage {
  private readonly capabilityService = inject(CapabilityService);
  private readonly sources = inject(SOURCE_MANAGER);
  private readonly mixer = inject(AUDIO_MIXER);
  private readonly chunkStore = inject(CHUNK_STORE);
  private readonly startTake = inject(START_TAKE);
  private readonly destroyRef = inject(DestroyRef);
  private readonly injector = inject(Injector);
  private meterSubscription: Subscription | null = null;
  private micsSubscription: Subscription | null = null;
  private session: TakeSession | null = null;
  private sessionSubscriptions: Subscription[] = [];
  private destroyed = false;

  protected readonly countdown = viewChild.required<Countdown>('countdownRef');
  private readonly pauseButton = viewChild<ElementRef<HTMLButtonElement>>('pauseButton');
  private readonly doneHeading = viewChild<ElementRef<HTMLElement>>('doneHeading');

  protected readonly systemAudioSupport = this.capabilityService.systemAudio;
  protected readonly systemAudio = signal(false);
  protected readonly display = signal<MediaStream | null>(null);
  protected readonly displayInfo = signal<LiveSource | null>(null);
  protected readonly mics = signal<MicDevice[]>([]);
  protected readonly selectedMic = signal<string | null>(micPreference.load());
  protected readonly micInfo = signal<LiveSource | null>(null);
  protected readonly micLevel = signal(0);
  protected readonly error = signal<string | null>(null);
  protected readonly phase = signal<Phase>('setup');
  protected readonly elapsedMs = signal(0);
  protected readonly result = signal<TakeMeta | null>(null);
  protected readonly downloadUrl = signal<string | null>(null);
  protected readonly anyMic = ANY_MIC;

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
    return $localize`${formatDuration(take.durationMs)}:duration: saved on this device. Uploading arrives soon.`;
  }

  protected downloadName(): string {
    const take = this.result();
    const extension = take?.mimeType.includes('mp4') ? 'mp4' : 'webm';
    return `sintade-${take?.takeId ?? 'recording'}.${extension}`;
  }

  onSystemAudioChange(event: Event): void {
    this.systemAudio.set((event.target as HTMLInputElement).checked);
  }

  async chooseScreen(): Promise<void> {
    this.error.set(null);
    try {
      const stream = await this.sources.pickDisplay({
        systemAudio: this.systemAudio() && this.systemAudioSupport().supported,
      });
      this.display.set(stream);
      this.displayInfo.set(describeDisplay(stream));
    } catch (error) {
      this.showError(error);
    }
  }

  async onMicChange(event: Event): Promise<void> {
    const deviceId = (event.target as HTMLSelectElement).value || null;
    this.selectedMic.set(deviceId);
    micPreference.save(deviceId);
    this.error.set(null);
    if (!deviceId) {
      this.stopMeter();
      this.sources.stopMic();
      this.micInfo.set(null);
      return;
    }
    try {
      const stream = await this.sources.openMic(deviceId === ANY_MIC ? undefined : deviceId);
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
      this.showError(error);
    }
  }

  /** 3-2-1 (Esc skips), then the take starts and focus moves to Pause. */
  async start(): Promise<void> {
    const display = this.display();
    if (this.phase() !== 'setup' || !display) {
      return;
    }
    this.error.set(null);
    this.phase.set('countdown');
    const outcome = await this.countdown().run();
    if (outcome === 'cancelled') {
      this.phase.set('setup');
      return;
    }
    try {
      const session = await this.startTake({
        display,
        mic: this.sources.currentMic,
        mixer: this.mixer,
        store: await this.chunkStore,
        takeId: crypto.randomUUID(),
      });
      this.session = session;
      this.phase.set('recording');
      this.sessionSubscriptions = [
        session.elapsed$(TIMER_INTERVAL_MS).subscribe((ms) => this.elapsedMs.set(ms)),
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
          this.showError(error);
        },
      );
      this.focusAfterRender(() => this.pauseButton()?.nativeElement);
    } catch (error) {
      this.phase.set('setup');
      this.showError(error);
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
    await this.session?.stop().catch((error: unknown) => this.showError(error));
  }

  newRecording(): void {
    this.releaseDownload();
    this.result.set(null);
    this.elapsedMs.set(0);
    this.phase.set('setup');
  }

  private async finish(take: TakeMeta): Promise<void> {
    if (this.destroyed) {
      return; // Left the page mid-take: it's saved; there's no view to update.
    }
    this.sessionSubscriptions.forEach((subscription) => subscription.unsubscribe());
    this.sessionSubscriptions = [];
    this.session = null;
    this.elapsedMs.set(take.durationMs);
    this.result.set(take);
    this.phase.set('done');
    this.focusAfterRender(() => this.doneHeading()?.nativeElement);
    try {
      const { blob } = await assembleTake(await this.chunkStore, take.takeId);
      this.releaseDownload();
      this.downloadUrl.set(URL.createObjectURL(blob));
    } catch {
      // The take is safe on the device either way; only the convenience download is missing.
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
      .subscribe((levels) => this.micLevel.update((shown) => meterValue(shown, levels.mic)));
  }

  private stopMeter(): void {
    this.meterSubscription?.unsubscribe();
    this.meterSubscription = null;
    this.micLevel.set(0);
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

  private showError(error: unknown): void {
    this.error.set(errorMessage(toCaptureError(error)));
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

/** Plain-language messages per failure; per-browser recovery instructions come on Day 30. */
function errorMessage(error: CaptureError): string {
  switch (error.kind) {
    case 'permission-denied':
      return $localize`Permission was denied or the picker was closed.`;
    case 'no-device':
      return $localize`That device isn't available. Check it's connected.`;
    case 'device-busy':
      return $localize`That device is in use by another application.`;
    case 'not-supported':
      return $localize`This browser can't record here.`;
    case 'aborted':
      return $localize`The shared screen is no longer available. Choose it again.`;
    default:
      return $localize`Something went wrong starting capture. Please try again.`;
  }
}
