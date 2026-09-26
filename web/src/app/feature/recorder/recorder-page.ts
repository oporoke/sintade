import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Subscription } from 'rxjs';

import { CaptureError, MicDevice, toCaptureError } from '../../capture';
import { CapabilityService } from '../../core/capability.service';
import { AUDIO_MIXER, SOURCE_MANAGER } from '../../core/capture.tokens';
import { Countdown } from './countdown';
import { meterValue } from './meter';
import { micPreference } from './mic-preference';

interface LiveSource {
  label: string;
  detail: string;
}

const METER_INTERVAL_MS = 50;
/**
 * Option value for a microphone listed before permission. Firefox then reports devices with an
 * empty `deviceId`, so the only thing we can ask for is "the browser's microphone".
 */
const ANY_MIC = 'any';

/**
 * Recorder setup: choose what to share, whether to include system audio, and which microphone
 * (Day 27); check the mic's level and count 3-2-1 into the take (Day 28). Starting the actual
 * recording, pausing and stopping arrive with the control bar (Day 29).
 */
@Component({
  selector: 'app-recorder-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Countdown],
  template: `
    <h1 i18n>New recording</h1>

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
        <p i18n>Say something: the bar should move.</p>
      }
    </section>

    <section aria-label="Start" i18n-aria-label>
      <button
        type="button"
        (click)="start()"
        [disabled]="!display() || countdown().running"
        data-testid="recorder-start"
        i18n
      >
        Start recording
      </button>
      @if (!display()) {
        <p i18n>Choose what to share first.</p>
      }
      <app-countdown #countdownRef />
      @if (ready()) {
        <p role="status" data-testid="recorder-ready" i18n>
          Countdown complete. Recording starts here once the recording controls are in place.
        </p>
      }
    </section>

    @if (error()) {
      <p role="alert" data-testid="recorder-error">{{ error() }}</p>
    }
  `,
})
export class RecorderPage {
  private readonly capabilityService = inject(CapabilityService);
  private readonly sources = inject(SOURCE_MANAGER);
  private readonly mixer = inject(AUDIO_MIXER);
  private meterSubscription: Subscription | null = null;
  protected readonly countdown = viewChild.required<Countdown>('countdownRef');
  private readonly destroyRef = inject(DestroyRef);
  private micsSubscription: Subscription | null = null;

  protected readonly systemAudioSupport = this.capabilityService.systemAudio;
  protected readonly systemAudio = signal(false);
  protected readonly display = signal<MediaStream | null>(null);
  protected readonly displayInfo = signal<LiveSource | null>(null);
  protected readonly mics = signal<MicDevice[]>([]);
  protected readonly selectedMic = signal<string | null>(micPreference.load());
  protected readonly micInfo = signal<LiveSource | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly micLevel = signal(0);
  protected readonly ready = signal(false);
  protected readonly anyMic = ANY_MIC;

  constructor() {
    this.sources.displayEnded$.pipe(takeUntilDestroyed()).subscribe(() => {
      this.display.set(null);
      this.displayInfo.set(null);
    });
    this.destroyRef.onDestroy(() => {
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

  /** Runs the 3-2-1 (Esc skips). The control bar (Day 29) starts the take when it ends. */
  async start(): Promise<void> {
    this.ready.set(false);
    const outcome = await this.countdown().run();
    if (outcome !== 'cancelled') {
      this.ready.set(true);
    }
  }

  /** Device check: meters the open mic through a mic-only mix. */
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
    default:
      return $localize`Something went wrong starting capture. Please try again.`;
  }
}
