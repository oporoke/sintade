import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  inject,
  signal,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Subscription } from 'rxjs';

import { AudioLevels, MicDevice, toCaptureError } from '../../capture';
import { CapabilityService } from '../../core/capability.service';
import { AUDIO_MIXER, SOURCE_MANAGER } from '../../core/capture.tokens';
import { ClipAnalysis, MixSelfTestResult, runMixSelfTest } from './mix-self-test';
import { RecorderSelfTestResult, runRecorderSelfTest } from './recorder-self-test';

interface CapabilityRow {
  name: string;
  supported: boolean;
}

interface TrackInfo {
  label: string;
  readyState: MediaStreamTrackState;
  detail: string;
}

/**
 * Developer diagnostics (dev-only, so plain strings rather than `$localize`, as on Day 9):
 * the capability matrix, a source preview (Day 21) that exercises `SourceManager` against the
 * real browser, live mixing with level meters, and device-free self-tests of the mixer (Day 22)
 * and the chunk recorder (Day 23).
 */
@Component({
  selector: 'app-debug-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <h1>Capability matrix</h1>
    <table data-testid="capability-matrix">
      <thead>
        <tr>
          <th>Capability</th>
          <th>Supported</th>
        </tr>
      </thead>
      <tbody>
        @for (row of rows(); track row.name) {
          <tr [attr.data-testid]="'capability-row-' + row.name">
            <td>{{ row.name }}</td>
            <td [attr.data-testid]="'capability-value-' + row.name">{{ row.supported }}</td>
          </tr>
        }
      </tbody>
    </table>

    <h2>Sources</h2>
    <section aria-label="Screen">
      <label>
        <input
          type="checkbox"
          [checked]="systemAudio()"
          [disabled]="!capabilities().systemAudio"
          (change)="onSystemAudioChange($event)"
          data-testid="source-system-audio"
        />
        System audio
        @if (!capabilities().systemAudio) {
          <small>(not supported in this browser)</small>
        }
      </label>
      <button type="button" (click)="pickScreen()" data-testid="source-pick-screen">
        Pick screen
      </button>
      @if (display(); as display) {
        <video
          [srcObject]="display"
          autoplay
          muted
          playsinline
          width="480"
          data-testid="source-screen-preview"
        ></video>
        <p data-testid="source-screen-info">
          {{ displayInfo()?.label }} — {{ displayInfo()?.readyState }} — {{ displayInfo()?.detail }}
        </p>
      }
    </section>

    <section aria-label="Microphone">
      <label>
        Microphone
        <select (change)="onMicChange($event)" data-testid="source-mic-select">
          <option value="">Browser default</option>
          @for (mic of mics(); track mic.deviceId) {
            <option [value]="mic.deviceId" [selected]="mic.deviceId === selectedMic()">
              {{ mic.label }}
            </option>
          }
        </select>
      </label>
      <button type="button" (click)="openMic()" data-testid="source-open-mic">Open mic</button>
      @if (micInfo(); as mic) {
        <p data-testid="source-mic-info">
          {{ mic.label }} — {{ mic.readyState }} — {{ mic.detail }}
        </p>
      }
    </section>

    <section aria-label="Mix">
      <button
        type="button"
        (click)="mixSources()"
        [disabled]="!display() && !micInfo()"
        data-testid="mix-start"
      >
        Mix sources
      </button>
      @if (mixInfo(); as mix) {
        <p data-testid="mix-info">{{ mix.label }} — {{ mix.readyState }}</p>
      }
      @if (levels(); as levels) {
        <dl>
          @for (name of levelNames; track name) {
            <dt>{{ name }}</dt>
            <dd>
              <meter min="0" max="1" [value]="levels[name]"></meter>
              <span [attr.data-testid]="'level-' + name">{{ levels[name].toFixed(3) }}</span>
            </dd>
          }
        </dl>
      }
    </section>

    <button type="button" (click)="stopAll()" data-testid="source-stop-all">Stop all</button>
    @if (error()) {
      <p role="alert" data-testid="source-error">{{ error() }}</p>
    }

    <h2>Mix self-test</h2>
    <p>
      Two synthetic tones stand in for mic (440 Hz) and display audio (1000 Hz): mixed by the real
      AudioMixer, recorded to a 2 s clip, decoded and analysed. 2500 Hz is a control.
    </p>
    <button
      type="button"
      (click)="runSelfTest()"
      [disabled]="selfTestRunning()"
      data-testid="selftest-run"
    >
      Run mix self-test
    </button>
    @if (selfTestRunning()) {
      <p data-testid="selftest-running">Recording…</p>
    }
    @if (selfTest(); as result) {
      <p data-testid="selftest-levels">
        mic {{ result.levels.mic.toFixed(3) }}, display {{ result.levels.display.toFixed(3) }}, mix
        {{ result.levels.mix.toFixed(3) }}
      </p>
      @if (clipAnalysis(result); as clip) {
        <p data-testid="selftest-clip">
          {{ clip.mimeType }}, {{ clip.bytes }} bytes, {{ clip.decodedSeconds.toFixed(2) }} s
        </p>
        <table data-testid="selftest-results">
          <tbody>
            @for (tone of clip.tones; track tone.frequencyHz) {
              <tr>
                <td>{{ tone.frequencyHz }} Hz ({{ tone.role }})</td>
                <td [attr.data-testid]="'selftest-tone-' + tone.frequencyHz">
                  {{ tone.present ? 'present' : 'absent' }}
                </td>
                <td>{{ tone.amplitude.toFixed(3) }}</td>
              </tr>
            }
          </tbody>
        </table>
      } @else {
        <p data-testid="selftest-clip-unavailable">No test clip: {{ clipUnavailable(result) }}</p>
      }
    }
    @if (selfTestError()) {
      <p role="alert" data-testid="selftest-error">{{ selfTestError() }}</p>
    }

    <h2>Recorder self-test</h2>
    <p>
      Records 6 s of an animated canvas plus a tone with ChunkRecorder (2 s slices), concatenates
      the chunks in order and plays the result to the end at 4×.
    </p>
    <button
      type="button"
      (click)="runRecorderTest()"
      [disabled]="recorderTestRunning()"
      data-testid="rec-selftest-run"
    >
      Run recorder self-test
    </button>
    @if (recorderTestRunning()) {
      <p data-testid="rec-selftest-running">Recording and replaying…</p>
    }
    @if (recorderTest(); as result) {
      <p data-testid="rec-selftest-chunks">
        {{ result.mimeType }}: {{ result.chunkSizes.length }} chunks ({{
          result.chunkSizes.join(', ')
        }}
        bytes) over {{ (result.recordedMs / 1000).toFixed(2) }} s
      </p>
      <p data-testid="rec-selftest-playback">
        {{ result.playback.ended ? 'played to the end' : 'did not finish' }}:
        {{ result.playback.videoWidth }}×{{ result.playback.videoHeight }},
        {{ result.playback.playedSeconds.toFixed(2) }} s
      </p>
      @if (recorderTestUrl(); as url) {
        <a [href]="url" download="recorder-self-test.webm" data-testid="rec-selftest-download">
          Download concatenated file
        </a>
      }
    }
    @if (recorderTestError()) {
      <p role="alert" data-testid="rec-selftest-error">{{ recorderTestError() }}</p>
    }
  `,
})
export class DebugPage {
  private readonly capabilityService = inject(CapabilityService);
  private readonly sources = inject(SOURCE_MANAGER);
  private readonly mixer = inject(AUDIO_MIXER);
  private levelsSubscription: Subscription | null = null;
  private readonly destroyRef = inject(DestroyRef);
  private micsSubscription: Subscription | null = null;

  protected readonly capabilities = this.capabilityService.capabilities;
  protected readonly systemAudio = signal(false);
  protected readonly display = signal<MediaStream | null>(null);
  protected readonly displayInfo = signal<TrackInfo | null>(null);
  protected readonly mics = signal<MicDevice[]>([]);
  protected readonly selectedMic = signal<string | undefined>(undefined);
  protected readonly micInfo = signal<TrackInfo | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly mixInfo = signal<TrackInfo | null>(null);
  protected readonly levels = signal<AudioLevels | null>(null);
  protected readonly levelNames = ['mic', 'display', 'mix'] as const;
  protected readonly selfTest = signal<MixSelfTestResult | null>(null);
  protected readonly selfTestRunning = signal(false);
  protected readonly selfTestError = signal<string | null>(null);
  protected readonly recorderTest = signal<RecorderSelfTestResult | null>(null);
  protected readonly recorderTestUrl = signal<string | null>(null);
  protected readonly recorderTestRunning = signal(false);
  protected readonly recorderTestError = signal<string | null>(null);

  readonly rows = computed<CapabilityRow[]>(() => {
    const capabilities = this.capabilities();
    return [
      { name: 'getDisplayMedia', supported: capabilities.getDisplayMedia },
      { name: 'mediaRecorderWebm', supported: capabilities.mediaRecorderWebm },
      { name: 'opfs', supported: capabilities.opfs },
      { name: 'systemAudio', supported: capabilities.systemAudio },
    ];
  });

  constructor() {
    this.sources.displayEnded$.pipe(takeUntilDestroyed()).subscribe(() => {
      this.display.set(null);
      this.displayInfo.set(null);
    });
    this.destroyRef.onDestroy(() => {
      this.releaseRecorderTestUrl();
      this.stopMix();
      this.sources.stopAll();
    });
  }

  onSystemAudioChange(event: Event): void {
    this.systemAudio.set((event.target as HTMLInputElement).checked);
  }

  onMicChange(event: Event): void {
    this.selectedMic.set((event.target as HTMLSelectElement).value || undefined);
  }

  async pickScreen(): Promise<void> {
    this.error.set(null);
    try {
      const stream = await this.sources.pickDisplay({ systemAudio: this.systemAudio() });
      this.display.set(stream);
      this.displayInfo.set(describe(stream.getVideoTracks()[0], stream.getAudioTracks().length));
    } catch (error) {
      this.showError(error);
    }
  }

  async openMic(): Promise<void> {
    this.error.set(null);
    try {
      const stream = await this.sources.openMic(this.selectedMic());
      this.micInfo.set(describe(stream.getAudioTracks()[0]));
      this.watchMics();
    } catch (error) {
      this.showError(error);
    }
  }

  /** Mixes whatever is open now (display audio and/or mic) and starts the level meters. */
  async mixSources(): Promise<void> {
    this.error.set(null);
    try {
      const track = await this.mixer.mix({
        mic: this.sources.currentMic,
        display: this.sources.currentDisplay,
      });
      this.mixInfo.set(
        track ? describe(track) : { label: 'no audio inputs', readyState: 'ended', detail: '' },
      );
      this.levelsSubscription?.unsubscribe();
      this.levelsSubscription = this.mixer
        .levels$()
        .pipe(takeUntilDestroyed(this.destroyRef))
        .subscribe((levels) => this.levels.set(levels));
    } catch (error) {
      this.showError(error);
    }
  }

  async runSelfTest(): Promise<void> {
    this.selfTestRunning.set(true);
    this.selfTest.set(null);
    this.selfTestError.set(null);
    try {
      this.selfTest.set(await runMixSelfTest());
    } catch (error) {
      this.selfTestError.set(error instanceof Error ? error.message : String(error));
    } finally {
      this.selfTestRunning.set(false);
    }
  }

  protected clipAnalysis(result: MixSelfTestResult): ClipAnalysis | null {
    return 'tones' in result.clip ? result.clip : null;
  }

  protected clipUnavailable(result: MixSelfTestResult): string {
    return 'unavailable' in result.clip ? result.clip.unavailable : '';
  }

  async runRecorderTest(): Promise<void> {
    this.recorderTestRunning.set(true);
    this.recorderTest.set(null);
    this.recorderTestError.set(null);
    this.releaseRecorderTestUrl();
    try {
      const result = await runRecorderSelfTest();
      this.recorderTest.set(result);
      this.recorderTestUrl.set(URL.createObjectURL(result.file));
    } catch (error) {
      this.recorderTestError.set(error instanceof Error ? error.message : String(error));
    } finally {
      this.recorderTestRunning.set(false);
    }
  }

  stopAll(): void {
    this.stopMix();
    this.sources.stopAll();
    this.display.set(null);
    this.displayInfo.set(null);
    this.micInfo.set(null);
  }

  /** Mic labels are only real after permission, so start listing once a mic has opened. */
  private watchMics(): void {
    if (this.micsSubscription) {
      return;
    }
    this.micsSubscription = this.sources.mics$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe({
      next: (mics) => this.mics.set(mics),
      error: (error: unknown) => this.showError(error),
    });
  }

  private releaseRecorderTestUrl(): void {
    const url = this.recorderTestUrl();
    if (url) {
      URL.revokeObjectURL(url);
      this.recorderTestUrl.set(null);
    }
  }

  private stopMix(): void {
    this.levelsSubscription?.unsubscribe();
    this.levelsSubscription = null;
    this.levels.set(null);
    this.mixInfo.set(null);
    void this.mixer.close();
  }

  private showError(error: unknown): void {
    const captureError = toCaptureError(error);
    this.error.set(`${captureError.kind}: ${captureError.message}`);
  }
}

function describe(track: MediaStreamTrack | undefined, audioTracks?: number): TrackInfo | null {
  if (!track) {
    return null;
  }
  const settings = track.getSettings();
  const parts =
    track.kind === 'video'
      ? [`${settings.width ?? '?'}×${settings.height ?? '?'} @ ${settings.frameRate ?? '?'} fps`]
      : [`device ${settings.deviceId ?? '?'}`];
  if (audioTracks !== undefined) {
    parts.push(audioTracks > 0 ? 'with system audio' : 'no audio');
  }
  return { label: track.label, readyState: track.readyState, detail: parts.join(', ') };
}
