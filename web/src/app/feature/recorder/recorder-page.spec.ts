import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Subject, of } from 'rxjs';

import { CaptureError, MicDevice, SourceManager } from '../../capture';
import { CapabilityService, SystemAudioSupport } from '../../core/capability.service';
import { SOURCE_MANAGER } from '../../core/capture.tokens';
import { RecorderPage } from './recorder-page';

function track(kind: 'audio' | 'video', label: string, settings: MediaTrackSettings = {}) {
  return { kind, label, getSettings: () => settings } as unknown as MediaStreamTrack;
}

function fakeSources() {
  const mics: MicDevice[] = [
    { deviceId: 'mic-a', label: 'Mic A' },
    { deviceId: 'mic-b', label: 'Mic B' },
  ];
  const display = {
    getVideoTracks: () => [track('video', 'screen:0', { width: 1920, height: 1080 })],
    getAudioTracks: () => [],
  } as unknown as MediaStream;
  const mic = { getAudioTracks: () => [track('audio', 'Mic B')] } as unknown as MediaStream;
  const manager = {
    displayEnded$: new Subject<void>().asObservable(),
    mics$: of(mics),
    listMics: vi.fn().mockResolvedValue(mics),
    pickDisplay: vi.fn().mockResolvedValue(display),
    openMic: vi.fn().mockResolvedValue(mic),
    stopMic: vi.fn(),
    stopAll: vi.fn(),
  };
  return manager as unknown as SourceManager & typeof manager;
}

async function setup(support: SystemAudioSupport, sources = fakeSources()) {
  TestBed.configureTestingModule({
    imports: [RecorderPage],
    providers: [
      { provide: CapabilityService, useValue: { systemAudio: signal(support) } },
      { provide: SOURCE_MANAGER, useValue: sources },
    ],
  });
  const fixture = TestBed.createComponent(RecorderPage);
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve, 0));
  fixture.detectChanges();
  const element: HTMLElement = fixture.nativeElement;
  const q = <T extends Element>(testId: string) =>
    element.querySelector<T>(`[data-testid="${testId}"]`);
  const settle = async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
    fixture.detectChanges();
  };
  return { fixture, element, sources, q, settle };
}

const SUPPORTED: SystemAudioSupport = { supported: true, note: null };

describe('RecorderPage', () => {
  beforeEach(() => localStorage.clear());

  it('disables system audio with the reason where it is unsupported', async () => {
    const { q } = await setup({
      supported: false,
      reason: "Firefox can't record system or tab audio.",
    });
    expect(q<HTMLInputElement>('recorder-system-audio')?.disabled).toBe(true);
    expect(q('recorder-system-audio-hint')?.textContent?.trim()).toBe(
      "Firefox can't record system or tab audio.",
    );
  });

  it('enables it with the platform note where only tab audio works', async () => {
    const { q } = await setup({ supported: true, note: 'Tab audio only here.' });
    expect(q<HTMLInputElement>('recorder-system-audio')?.disabled).toBe(false);
    expect(q('recorder-system-audio-hint')?.textContent?.trim()).toBe('Tab audio only here.');
  });

  it('asks for system audio only when ticked, and previews the chosen screen', async () => {
    const { q, sources, settle } = await setup(SUPPORTED);
    const toggle = q<HTMLInputElement>('recorder-system-audio');
    toggle?.click();
    q<HTMLButtonElement>('recorder-choose-screen')?.click();
    await settle();

    expect(sources.pickDisplay).toHaveBeenCalledWith({ systemAudio: true });
    expect(q('recorder-screen-preview')).not.toBeNull();
    expect(q('recorder-screen-info')?.textContent).toContain('1920×1080');
  });

  it('lists mics, opens the chosen one, and remembers it', async () => {
    const { q, sources, settle } = await setup(SUPPORTED);
    const select = q<HTMLSelectElement>('recorder-mic-select');
    expect([...(select?.options ?? [])].map((option) => option.text.trim())).toEqual([
      'No microphone',
      'Mic A',
      'Mic B',
    ]);

    if (select) {
      select.value = 'mic-b';
      select.dispatchEvent(new Event('change'));
    }
    await settle();

    expect(sources.openMic).toHaveBeenCalledWith('mic-b');
    expect(q('recorder-mic-info')?.textContent).toContain('Mic B');
    expect(localStorage.getItem('sintade.recorder.mic')).toBe('mic-b');
  });

  it('preselects the remembered mic on the next visit', async () => {
    localStorage.setItem('sintade.recorder.mic', 'mic-b');
    const { q } = await setup(SUPPORTED);
    expect(q<HTMLSelectElement>('recorder-mic-select')?.value).toBe('mic-b');
  });

  it('"No microphone" releases the mic and forgets the choice', async () => {
    localStorage.setItem('sintade.recorder.mic', 'mic-b');
    const { q, sources, settle } = await setup(SUPPORTED);
    const select = q<HTMLSelectElement>('recorder-mic-select');
    if (select) {
      select.value = '';
      select.dispatchEvent(new Event('change'));
    }
    await settle();
    expect(sources.stopMic).toHaveBeenCalled();
    expect(localStorage.getItem('sintade.recorder.mic')).toBeNull();
  });

  it('explains a denied or cancelled screen picker in plain language', async () => {
    const sources = fakeSources();
    sources.pickDisplay.mockRejectedValue(new CaptureError('permission-denied', 'NotAllowedError'));
    const { q, settle } = await setup(SUPPORTED, sources);
    q<HTMLButtonElement>('recorder-choose-screen')?.click();
    await settle();
    expect(q('recorder-error')?.textContent?.trim()).toBe(
      'Permission was denied or the picker was closed.',
    );
  });
});
