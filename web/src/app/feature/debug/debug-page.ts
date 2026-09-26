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

import { MicDevice, toCaptureError } from '../../capture';
import { CapabilityService } from '../../core/capability.service';
import { SOURCE_MANAGER } from '../../core/source-manager.token';

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
 * the capability matrix, plus a source preview (Day 21) that exercises `SourceManager` against
 * the real browser: pick a screen, open a mic, see both live.
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

    <button type="button" (click)="stopAll()" data-testid="source-stop-all">Stop all</button>
    @if (error()) {
      <p role="alert" data-testid="source-error">{{ error() }}</p>
    }
  `,
})
export class DebugPage {
  private readonly capabilityService = inject(CapabilityService);
  private readonly sources = inject(SOURCE_MANAGER);
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
    this.destroyRef.onDestroy(() => this.sources.stopAll());
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

  stopAll(): void {
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
