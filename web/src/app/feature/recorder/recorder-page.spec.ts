import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Subject, of } from 'rxjs';

import { BehaviorSubject, Subject as RxSubject } from 'rxjs';

import {
  AudioMixer,
  CaptureError,
  ChunkStore,
  MicDevice,
  RecorderState,
  SourceManager,
  TakeMeta,
  TakeSession,
} from '../../capture';
import { CapabilityService, SystemAudioSupport } from '../../core/capability.service';
import { AUDIO_MIXER, CHUNK_STORE, SOURCE_MANAGER, START_TAKE } from '../../core/capture.tokens';
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

function fakeMixer(micLevel = 0.4) {
  const mixer = {
    mix: vi.fn().mockResolvedValue({ kind: 'audio' }),
    levels$: () => of({ mic: micLevel, display: 0, mix: micLevel }),
    close: vi.fn().mockResolvedValue(undefined),
  };
  return mixer as unknown as AudioMixer & typeof mixer;
}

async function setup(support: SystemAudioSupport, sources = fakeSources(), mixer = fakeMixer()) {
  TestBed.configureTestingModule({
    imports: [RecorderPage],
    providers: [
      { provide: CapabilityService, useValue: capabilities(support) },
      { provide: SOURCE_MANAGER, useValue: sources },
      { provide: AUDIO_MIXER, useValue: mixer },
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
  return { fixture, element, sources, mixer, q, settle };
}

const SUPPORTED: SystemAudioSupport = { supported: true, note: null };

function capabilities(support: SystemAudioSupport, getDisplayMedia = true) {
  return {
    systemAudio: signal(support),
    capabilities: signal({
      getDisplayMedia,
      mediaRecorderWebm: true,
      opfs: true,
      systemAudio: true,
    }),
  };
}

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
    expect(q('recorder-problem-title')?.textContent?.trim()).toBe(
      'Screen sharing was cancelled or blocked',
    );
    expect(q('recorder-help-steps')?.querySelectorAll('li').length).toBeGreaterThan(0);

    // Try again re-opens the screen picker.
    sources.pickDisplay.mockClear();
    q<HTMLButtonElement>('recorder-retry')?.click();
    await settle();
    expect(sources.pickDisplay).toHaveBeenCalledTimes(1);
  });

  it('meters the opened mic and stops metering when the mic is released', async () => {
    const { q, mixer, settle } = await setup(SUPPORTED);
    const select = q<HTMLSelectElement>('recorder-mic-select');
    if (select) {
      select.value = 'mic-a';
      select.dispatchEvent(new Event('change'));
    }
    await settle();
    expect(mixer.mix).toHaveBeenCalledWith({ mic: expect.anything() });
    expect(q('recorder-mic-level')?.textContent?.trim()).toBe('0.400');

    if (select) {
      select.value = '';
      select.dispatchEvent(new Event('change'));
    }
    await settle();
    expect(mixer.close).toHaveBeenCalled();
    expect(q('recorder-mic-meter')).toBeNull();
  });
});

describe('RecorderPage before mic permission (Firefox)', () => {
  beforeEach(() => localStorage.clear());

  it('opens the browser default for an unaddressable mic, then pins the real device', async () => {
    const sources = fakeSources();
    sources.listMics.mockResolvedValue([{ deviceId: '', label: 'Microphone 1' }]);
    const realTrack = {
      kind: 'audio',
      label: 'Fake Mic',
      getSettings: () => ({ deviceId: 'real-mic-id' }),
    } as unknown as MediaStreamTrack;
    sources.openMic.mockResolvedValue({
      getAudioTracks: () => [realTrack],
    } as unknown as MediaStream);
    const { q, settle } = await setup(SUPPORTED, sources);

    const select = q<HTMLSelectElement>('recorder-mic-select');
    if (select) {
      select.value = 'any';
      select.dispatchEvent(new Event('change'));
    }
    await settle();

    expect(sources.openMic).toHaveBeenCalledWith(undefined);
    expect(q('recorder-mic-info')?.textContent).toContain('Fake Mic');
    expect(localStorage.getItem('sintade.recorder.mic')).toBe('real-mic-id');
  });
});

/** A take that records until told otherwise; `endFromBrowser()` mimics "Stop sharing". */
function fakeSession() {
  const state = new BehaviorSubject<RecorderState>('recording');
  const elapsed = new RxSubject<number>();
  let finish: (meta: TakeMeta) => void = () => undefined;
  const ended = new Promise<TakeMeta>((resolve) => (finish = resolve));
  const meta: TakeMeta = {
    takeId: '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b',
    startedAt: 0,
    mimeType: 'video/webm',
    chunkCount: 3,
    durationMs: 12_000,
  };
  const end = () => {
    state.next('stopping');
    state.next('idle');
    finish(meta);
  };
  const session = {
    state$: state.asObservable(),
    elapsed$: () => elapsed.asObservable(),
    pause: vi.fn(() => state.next('paused')),
    resume: vi.fn(() => state.next('recording')),
    stop: vi.fn(async () => {
      end();
      return meta;
    }),
    ended,
  };
  return {
    session: session as unknown as TakeSession & typeof session,
    elapsed,
    endFromBrowser: end,
  };
}

describe('RecorderPage control bar', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  async function recording() {
    const take = fakeSession();
    const startTake = vi.fn().mockResolvedValue(take.session);
    const store = {
      getMeta: vi.fn().mockResolvedValue(null),
      indexes: vi.fn().mockResolvedValue([]),
      get: vi.fn(),
    } as unknown as ChunkStore;
    TestBed.configureTestingModule({
      imports: [RecorderPage],
      providers: [
        { provide: CapabilityService, useValue: capabilities(SUPPORTED) },
        { provide: SOURCE_MANAGER, useValue: fakeSources() },
        { provide: AUDIO_MIXER, useValue: fakeMixer() },
        { provide: CHUNK_STORE, useValue: Promise.resolve(store) },
        { provide: START_TAKE, useValue: startTake },
      ],
    });
    const fixture = TestBed.createComponent(RecorderPage);
    document.body.appendChild(fixture.nativeElement);
    const element: HTMLElement = fixture.nativeElement;
    const q = <T extends Element>(testId: string) =>
      element.querySelector<T>(`[data-testid="${testId}"]`);
    const settle = async (ms = 0) => {
      await vi.advanceTimersByTimeAsync(ms);
      fixture.detectChanges();
      await vi.advanceTimersByTimeAsync(0);
      fixture.detectChanges();
    };
    fixture.detectChanges();
    await settle();
    q<HTMLButtonElement>('recorder-choose-screen')?.click();
    await settle();
    q<HTMLButtonElement>('recorder-start')?.click();
    await settle(3000); // the 3-2-1
    return { ...take, startTake, fixture, q, settle };
  }

  it('starts the take after the countdown and moves focus to Pause', async () => {
    const { q, startTake } = await recording();
    expect(startTake).toHaveBeenCalledTimes(1);
    expect(q('recorder-status')?.textContent?.trim()).toBe('Recording');
    expect(document.activeElement).toBe(q('recorder-pause'));
    expect(q<HTMLFieldSetElement>('recorder-setup')?.disabled).toBe(true);
  });

  it('shows the pause-excluding timer as m:ss', async () => {
    const { q, elapsed, settle } = await recording();
    elapsed.next(65_400);
    await settle();
    expect(q('recorder-timer')?.textContent?.trim()).toBe('1:05');
  });

  it('pauses and resumes from the same button, relabelled', async () => {
    const { q, session, settle } = await recording();
    q<HTMLButtonElement>('recorder-pause')?.click();
    await settle();
    expect(session.pause).toHaveBeenCalled();
    expect(q('recorder-status')?.textContent?.trim()).toBe('Paused');
    expect(q('recorder-pause')?.textContent?.trim()).toBe('Resume');

    q<HTMLButtonElement>('recorder-pause')?.click();
    await settle();
    expect(session.resume).toHaveBeenCalled();
    expect(q('recorder-pause')?.textContent?.trim()).toBe('Pause');
  });

  it('Stop saves the take, shows the result and moves focus to it', async () => {
    const { q, session, settle } = await recording();
    q<HTMLButtonElement>('recorder-stop')?.click();
    await settle();
    expect(session.stop).toHaveBeenCalled();
    expect(q('recorder-controls')).toBeNull();
    expect(q('recorder-done-summary')?.textContent?.trim()).toBe(
      '12 s saved on this device. Uploading arrives soon.',
    );
    expect(document.activeElement?.id).toBe('recorder-done-heading');
  });

  it('ends the same way when the browser stops the share', async () => {
    const { q, endFromBrowser, settle } = await recording();
    endFromBrowser();
    await settle();
    expect(q('recorder-done')).not.toBeNull();
  });
});

describe('RecorderPage problems and view-only', () => {
  beforeEach(() => localStorage.clear());

  it('explains a blocked microphone with steps, and Try again reopens it', async () => {
    const sources = fakeSources();
    sources.openMic.mockRejectedValueOnce(new CaptureError('permission-denied', 'NotAllowedError'));
    const { q, settle } = await setup(SUPPORTED, sources);
    const select = q<HTMLSelectElement>('recorder-mic-select');
    if (select) {
      select.value = 'mic-a';
      select.dispatchEvent(new Event('change'));
    }
    await settle();

    expect(q('recorder-problem-title')?.textContent?.trim()).toBe('Microphone access is blocked');
    expect(q('recorder-help-steps')?.textContent).toMatch(/Microphone|microphone/);

    q<HTMLButtonElement>('recorder-retry')?.click();
    await settle();
    expect(sources.openMic).toHaveBeenLastCalledWith('mic-a');
    expect(q('recorder-error')).toBeNull();
    expect(q('recorder-mic-info')).not.toBeNull();
  });

  it('shows the view-only notice instead of the recorder without screen capture', async () => {
    TestBed.configureTestingModule({
      imports: [RecorderPage],
      providers: [
        { provide: CapabilityService, useValue: capabilities(SUPPORTED, false) },
        { provide: SOURCE_MANAGER, useValue: fakeSources() },
        { provide: AUDIO_MIXER, useValue: fakeMixer() },
      ],
    });
    const fixture = TestBed.createComponent(RecorderPage);
    fixture.detectChanges();
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="recorder-mobile-notice"]')).not.toBeNull();
    expect(element.querySelector('[data-testid="recorder-setup"]')).toBeNull();
  });
});
