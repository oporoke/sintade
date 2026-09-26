import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';

import { Subject, of } from 'rxjs';

import { AudioMixer, CaptureError, MicDevice, SourceManager } from '../../capture';
import { CapabilityService, CapabilityMatrix } from '../../core/capability.service';
import { AUDIO_MIXER, SOURCE_MANAGER } from '../../core/capture.tokens';
import { DebugPage } from './debug-page';

describe('DebugPage', () => {
  it('renders one row per capability with its detected value', () => {
    const fakeCapabilities: CapabilityMatrix = {
      getDisplayMedia: true,
      mediaRecorderWebm: false,
      opfs: true,
      systemAudio: false,
    };
    const capabilityServiceStub: Partial<CapabilityService> = {
      capabilities: signal(fakeCapabilities),
    };

    TestBed.configureTestingModule({
      imports: [DebugPage],
      providers: [
        { provide: CapabilityService, useValue: capabilityServiceStub },
        { provide: SOURCE_MANAGER, useValue: fakeSources().manager },
        { provide: AUDIO_MIXER, useValue: fakeMixer() },
      ],
    });

    const fixture = TestBed.createComponent(DebugPage);
    fixture.detectChanges();
    const element: HTMLElement = fixture.nativeElement;

    expect(
      element
        .querySelector('[data-testid="capability-value-getDisplayMedia"]')
        ?.textContent?.trim(),
    ).toBe('true');
    expect(
      element
        .querySelector('[data-testid="capability-value-mediaRecorderWebm"]')
        ?.textContent?.trim(),
    ).toBe('false');
    expect(element.querySelectorAll('[data-testid^="capability-row-"]').length).toBe(4);
  });

  describe('sources', () => {
    function setup(capabilities: Partial<CapabilityMatrix> = {}) {
      const sources = fakeSources();
      const mixer = fakeMixer();
      TestBed.configureTestingModule({
        imports: [DebugPage],
        providers: [
          {
            provide: CapabilityService,
            useValue: {
              capabilities: signal<CapabilityMatrix>({
                getDisplayMedia: true,
                mediaRecorderWebm: true,
                opfs: true,
                systemAudio: true,
                ...capabilities,
              }),
            },
          },
          { provide: SOURCE_MANAGER, useValue: sources.manager },
          { provide: AUDIO_MIXER, useValue: mixer },
        ],
      });
      const fixture = TestBed.createComponent(DebugPage);
      fixture.detectChanges();
      const element: HTMLElement = fixture.nativeElement;
      const click = async (testId: string) => {
        element.querySelector<HTMLElement>(`[data-testid="${testId}"]`)?.click();
        await fixture.whenStable();
        fixture.detectChanges();
      };
      const text = (testId: string) =>
        element.querySelector(`[data-testid="${testId}"]`)?.textContent?.trim();
      return { sources, mixer, fixture, element, click, text };
    }

    it('previews the picked screen with its track info', async () => {
      const { sources, element, click, text } = setup();
      await click('source-pick-screen');

      expect(sources.manager.pickDisplay).toHaveBeenCalledWith({ systemAudio: false });
      expect(element.querySelector('[data-testid="source-screen-preview"]')).not.toBeNull();
      expect(text('source-screen-info')).toContain('screen:0');
      expect(text('source-screen-info')).toContain('1920×1080');
    });

    it('opens the mic, shows it live, and then lists mics', async () => {
      const { sources, click, text, element } = setup();
      await click('source-open-mic');

      expect(sources.manager.openMic).toHaveBeenCalledWith(undefined);
      expect(text('source-mic-info')).toContain('Fake Mic');
      expect(text('source-mic-info')).toContain('live');
      expect(element.querySelectorAll('[data-testid="source-mic-select"] option').length).toBe(2);
    });

    it('clears the preview when the browser ends the share', async () => {
      const { sources, click, fixture, element } = setup();
      await click('source-pick-screen');
      sources.displayEnded.next();
      fixture.detectChanges();

      expect(element.querySelector('[data-testid="source-screen-preview"]')).toBeNull();
    });

    it('shows the normalised error kind when capture fails', async () => {
      const { sources, click, text } = setup();
      sources.manager.pickDisplay = vi
        .fn()
        .mockRejectedValue(new CaptureError('permission-denied', 'user said no'));
      await click('source-pick-screen');

      expect(text('source-error')).toBe('permission-denied: user said no');
    });

    it('mixes the open sources and shows level meters', async () => {
      const { sources, mixer, click, text } = setup();
      await click('source-pick-screen');
      await click('source-open-mic');
      await click('mix-start');

      expect(mixer.mix).toHaveBeenCalledWith({
        mic: sources.manager.currentMic,
        display: sources.manager.currentDisplay,
      });
      expect(text('mix-info')).toContain('Mixed Audio');
      expect(text('level-mic')).toBe('0.500');
      expect(text('level-display')).toBe('0.000');
      expect(text('level-mix')).toBe('0.500');
    });

    it('stop all tears down the mix', async () => {
      const { mixer, click, element } = setup();
      await click('source-open-mic');
      await click('mix-start');
      await click('source-stop-all');

      expect(mixer.close).toHaveBeenCalled();
      expect(element.querySelector('[data-testid="level-mix"]')).toBeNull();
    });

    it('disables the system-audio toggle where it is unsupported', () => {
      const { element } = setup({ systemAudio: false });
      const toggle = element.querySelector<HTMLInputElement>('[data-testid="source-system-audio"]');
      expect(toggle?.disabled).toBe(true);
    });
  });
});

function fakeTrack(kind: 'audio' | 'video', label: string, settings: MediaTrackSettings) {
  return {
    kind,
    label,
    readyState: 'live',
    getSettings: () => settings,
    stop: vi.fn(),
  } as unknown as MediaStreamTrack;
}

function fakeSources() {
  const video = fakeTrack('video', 'screen:0', { width: 1920, height: 1080, frameRate: 30 });
  const audio = fakeTrack('audio', 'Fake Mic', { deviceId: 'fake-mic' });
  const displayStream = {
    getVideoTracks: () => [video],
    getAudioTracks: () => [],
  } as unknown as MediaStream;
  const micStream = { getAudioTracks: () => [audio] } as unknown as MediaStream;
  const mics: MicDevice[] = [{ deviceId: 'fake-mic', label: 'Fake Mic' }];
  const displayEnded = new Subject<void>();
  const manager = {
    displayEnded$: displayEnded.asObservable(),
    mics$: of(mics),
    currentDisplay: displayStream,
    currentMic: micStream,
    pickDisplay: vi.fn().mockResolvedValue(displayStream),
    openMic: vi.fn().mockResolvedValue(micStream),
    stopAll: vi.fn(),
  } as unknown as SourceManager;
  return { manager, displayEnded };
}

function fakeMixer() {
  const mixed = fakeTrack('audio', 'Mixed Audio', {});
  return {
    mix: vi.fn().mockResolvedValue(mixed),
    levels$: () => of({ mic: 0.5, display: 0, mix: 0.5 }),
    close: vi.fn().mockResolvedValue(undefined),
  } as unknown as AudioMixer;
}
