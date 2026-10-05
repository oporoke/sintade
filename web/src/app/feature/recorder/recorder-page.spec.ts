import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Subject, of } from 'rxjs';

import { BehaviorSubject, ReplaySubject, Subject as RxSubject } from 'rxjs';

import {
  AudioMixer,
  CaptureError,
  ChunkStore,
  MicDevice,
  RecorderState,
  SourceManager,
  TakeMeta,
  TakeSession,
  UploadHttpError,
} from '../../capture';
import { CapabilityService, SystemAudioSupport } from '../../core/capability.service';
import {
  AUDIO_MIXER,
  CHUNK_STORE,
  FREE_PLAN_LIMITS,
  PLAN_LIMITS,
  PlanLimits,
  RECORDINGS_API,
  RecordingsApi,
  SOURCE_MANAGER,
  START_TAKE,
} from '../../core/capture.tokens';
import { LIBRARY_API } from '../../core/library-api.service';
import { SHARE_API, SharePort } from '../../core/share-api.service';
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
    applyQuality: vi.fn().mockResolvedValue(undefined),
    stopAll: vi.fn(),
  };
  return manager as unknown as SourceManager & typeof manager;
}

function fakeMixer(micLevel = 0.4) {
  const mixer = {
    mix: vi.fn().mockResolvedValue({ kind: 'audio' }),
    levels$: () => of({ mic: micLevel, display: 0, mix: micLevel }),
    close: vi.fn().mockResolvedValue(undefined),
    isMuted: vi.fn().mockReturnValue(false),
    setMuted: vi.fn(),
    setVolume: vi.fn(),
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
      { provide: PLAN_LIMITS, useValue: signal(planLimits) },
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

let planLimits: PlanLimits = FREE_PLAN_LIMITS;

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

  it('offers 4K only on a plan that allows it; a free user cannot pick it', async () => {
    const free = await setup(SUPPORTED);
    const four = free
      .q<HTMLSelectElement>('recorder-resolution')!
      .querySelector<HTMLOptionElement>('option[value="2160"]')!;
    expect(four.disabled).toBe(true);
    expect(four.textContent).toContain('not on your plan');
    const select = free.q<HTMLSelectElement>('recorder-resolution')!;
    expect(select.value).toBe('1080');
    TestBed.resetTestingModule();

    planLimits = { maxResolution: 2160, maxDurationMs: 3_600_000 };
    try {
      const paid = await setup(SUPPORTED);
      const option = paid
        .q<HTMLSelectElement>('recorder-resolution')!
        .querySelector<HTMLOptionElement>('option[value="2160"]')!;
      expect(option.disabled).toBe(false);
    } finally {
      planLimits = FREE_PLAN_LIMITS;
    }
  });

  it('drags a crop on the preview and passes it, and the cursor choice, to the take', async () => {
    const { q, sources, settle, fixture } = await setup(SUPPORTED);
    q<HTMLButtonElement>('recorder-choose-screen')!.click();
    await settle();
    const pad = q<HTMLElement>('recorder-crop-pad')!;
    pad.getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 200 }) as DOMRect;
    const fire = (type: string, x: number, y: number) =>
      pad.dispatchEvent(new PointerEvent(type, { clientX: x, clientY: y, pointerId: 1 }));
    fire('pointerdown', 200, 0);
    fire('pointermove', 400, 200);
    fire('pointerup', 400, 200);
    await settle();
    const box = q<HTMLElement>('recorder-crop-box')!;
    expect(box.style.left).toBe('50%');
    expect(box.style.width).toBe('50%');
    expect(box.style.height).toBe('100%');

    q<HTMLButtonElement>('recorder-crop-clear')!.click();
    await settle();
    expect(q('recorder-crop-box')).toBeNull();

    const cursor = q<HTMLInputElement>('recorder-cursor')!;
    cursor.checked = false;
    cursor.dispatchEvent(new Event('change'));
    q<HTMLButtonElement>('recorder-choose-screen')!.click();
    await settle();
    expect(sources.pickDisplay).toHaveBeenLastCalledWith(
      expect.objectContaining({ cursor: 'never' }),
    );
    void fixture;
  });

  it('asks the browser for the chosen frame rate and passes the mic processing toggles', async () => {
    const { q, sources, settle } = await setup(SUPPORTED);
    const fps = q<HTMLSelectElement>('recorder-fps')!;
    fps.value = '60';
    fps.dispatchEvent(new Event('change'));
    await settle();
    q<HTMLButtonElement>('recorder-choose-screen')!.click();
    await settle();
    expect(sources.pickDisplay).toHaveBeenCalledWith(expect.objectContaining({ frameRate: 60 }));

    const noise = q<HTMLInputElement>('recorder-noise-suppression')!;
    expect(noise.checked).toBe(true);
    noise.checked = false;
    noise.dispatchEvent(new Event('change'));
    await settle();
    const mic = q<HTMLSelectElement>('recorder-mic-select')!;
    mic.value = 'mic-b';
    mic.dispatchEvent(new Event('change'));
    await settle();
    expect(sources.openMic).toHaveBeenLastCalledWith(
      'mic-b',
      expect.objectContaining({ noiseSuppression: false, echoCancellation: true }),
    );
  });

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

    expect(sources.pickDisplay).toHaveBeenCalledWith({
      systemAudio: true,
      frameRate: 30,
      height: 1080,
      cursor: 'always',
    });
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

    expect(sources.openMic).toHaveBeenCalledWith('mic-b', expect.any(Object));
    expect(q('recorder-mic-info')?.textContent).toContain('Mic B');
    expect(localStorage.getItem('sintade.recorder.mic')).toBe('mic-b');
  });

  it('keeps a remembered mic choosable when the browser hides device ids', async () => {
    localStorage.setItem('sintade.recorder.mic', 'real-id-from-last-time');
    const sources = fakeSources();
    sources.listMics.mockResolvedValue([{ deviceId: '', label: 'Microphone 1' }]);
    const { q } = await setup(SUPPORTED, sources);
    const select = q<HTMLSelectElement>('recorder-mic-select');
    expect(select?.value).toBe('real-id-from-last-time');
    expect(select?.selectedOptions[0]?.textContent?.trim()).toBe('Last used microphone');
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

    expect(sources.openMic).toHaveBeenCalledWith(undefined, expect.any(Object));
    expect(q('recorder-mic-info')?.textContent).toContain('Fake Mic');
    expect(localStorage.getItem('sintade.recorder.mic')).toBe('real-mic-id');
  });
});

/** A take that records until told otherwise; `endFromBrowser()` mimics "Stop sharing". */
function fakeSession() {
  const state = new BehaviorSubject<RecorderState>('recording');
  const elapsed = new RxSubject<number>();
  const stored = new ReplaySubject<number>();
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
    stored.complete();
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
    stored$: stored.asObservable(),
  };
  return {
    session: session as unknown as TakeSession & typeof session,
    elapsed,
    stored,
    endFromBrowser: end,
  };
}

describe('RecorderPage control bar', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  async function recording(api: RecordingsApi = offlineApi(), share: SharePort = fakeShare()) {
    const take = fakeSession();
    const mixer = fakeMixer();
    const library = { trash: vi.fn().mockResolvedValue(undefined) };
    const startTake = vi.fn().mockResolvedValue(take.session);
    const store = {
      getMeta: vi.fn().mockResolvedValue(null),
      indexes: vi.fn().mockResolvedValue([]),
      get: vi.fn().mockResolvedValue(new Blob(['chunk'])),
      deleteTake: vi.fn().mockResolvedValue(undefined),
    } as unknown as ChunkStore;
    TestBed.configureTestingModule({
      imports: [RecorderPage],
      providers: [
        { provide: CapabilityService, useValue: capabilities(SUPPORTED) },
        { provide: SOURCE_MANAGER, useValue: fakeSources() },
        { provide: AUDIO_MIXER, useValue: mixer },
        { provide: PLAN_LIMITS, useValue: signal(planLimits) },
        { provide: LIBRARY_API, useValue: library },
        { provide: CHUNK_STORE, useValue: Promise.resolve(store) },
        { provide: START_TAKE, useValue: startTake },
        { provide: RECORDINGS_API, useValue: api },
        { provide: SHARE_API, useValue: share },
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
    return { ...take, startTake, store, library, mixer, fixture, q, settle };
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
    expect(q('recorder-done-summary')?.textContent?.trim()).toBe('12 s saved on this device.');
    expect(document.activeElement?.id).toBe('recorder-done-heading');
  });

  describe('uploading', () => {
    beforeEach(() => {
      vi.stubGlobal('MediaRecorder', { isTypeSupported: () => true });
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => new Response(null, { status: 200 })),
      );
    });
    afterEach(() => vi.unstubAllGlobals());

    it('stops by itself just inside the plan limit, and says why', async () => {
      const api = onlineApi();
      const { q, session, elapsed, settle } = await recording(api);
      elapsed.next(598_000);
      await settle();
      expect(session.stop).not.toHaveBeenCalled();
      elapsed.next(599_000);
      await settle();
      expect(session.stop).toHaveBeenCalledTimes(1);
      expect(q('recorder-limit-notice')?.textContent).toContain('up to 10 min');
    });

    it('warns 30 seconds before the automatic stop, in steps', async () => {
      const { q, elapsed, settle } = await recording(onlineApi());
      elapsed.next(560_000);
      await settle();
      expect(q('recorder-limit-warning')).toBeNull();
      elapsed.next(575_000); // 24 s left before the 1 s margin -> 25
      await settle();
      expect(q('recorder-limit-warning')?.textContent).toContain('in 25 s');
      elapsed.next(591_500); // 7.5 s left -> 8
      await settle();
      expect(q('recorder-limit-warning')?.textContent).toContain('in 8 s');
    });

    it('discarding deletes the take on the device, trashes the server copy and finalizes nothing', async () => {
      const api = onlineApi();
      const { q, session, store, library, settle } = await recording(api);
      q<HTMLButtonElement>('recorder-discard')?.click();
      await settle();
      expect(q('recorder-confirm')).not.toBeNull();
      q<HTMLButtonElement>('recorder-confirm-yes')?.click();
      await settle();
      expect(session.stop).toHaveBeenCalled();
      expect(store.deleteTake).toHaveBeenCalledWith(SERVER_TAKE);
      expect(library.trash).toHaveBeenCalledWith('rec-1');
      expect(api.finalize).not.toHaveBeenCalled();
      expect(q('recorder-controls')).toBeNull();
      expect(q('recorder-start')).not.toBeNull();
    });

    it('keeps recording when the discard is not confirmed', async () => {
      const { q, session, settle } = await recording(onlineApi());
      q<HTMLButtonElement>('recorder-discard')?.click();
      await settle();
      q<HTMLButtonElement>('recorder-confirm-no')?.click();
      await settle();
      expect(session.stop).not.toHaveBeenCalled();
      expect(q('recorder-status')?.textContent?.trim()).toBe('Recording');
    });

    it('restart discards the take and starts a new one after the countdown', async () => {
      const api = onlineApi();
      const { q, startTake, store, settle } = await recording(api);
      q<HTMLButtonElement>('recorder-restart')?.click();
      await settle();
      q<HTMLButtonElement>('recorder-confirm-yes')?.click();
      await settle(3000);
      expect(store.deleteTake).toHaveBeenCalledWith(SERVER_TAKE);
      expect(api.createRecording).toHaveBeenCalledTimes(2);
      expect(startTake).toHaveBeenCalledTimes(2);
    });

    it('keyboard shortcuts pause, mute the mic and stop', async () => {
      const { q, session, mixer, settle } = await recording(onlineApi());
      const press = (key: string) =>
        document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
      press('p');
      await settle();
      expect(session.pause).toHaveBeenCalled();
      press('p');
      await settle();
      expect(session.resume).toHaveBeenCalled();
      press('m');
      expect(mixer.setMuted).toHaveBeenCalledWith('mic', true);
      press('r');
      await settle();
      expect(q('recorder-confirm')).not.toBeNull();
      press('Escape');
      await settle();
      expect(q('recorder-confirm')).toBeNull();
      press('s');
      await settle();
      expect(session.stop).toHaveBeenCalled();
    });

    it('ignores shortcuts typed into a form field or with a modifier', async () => {
      const { session, settle } = await recording(onlineApi());
      const input = document.createElement('input');
      document.body.appendChild(input);
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 's', bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 's', ctrlKey: true }));
      await settle();
      expect(session.stop).not.toHaveBeenCalled();
      input.remove();
    });

    it("doesn't record when the plan's recording limit is reached", async () => {
      const api = onlineApi();
      api.createRecording.mockRejectedValue(new UploadHttpError('api', 402, 'limit'));
      const { q, startTake } = await recording(api);
      expect(startTake).not.toHaveBeenCalled();
      expect(q('recorder-limit-notice')?.textContent).toContain("reached your plan's limit");
      expect(q('recorder-start')).not.toBeNull();
    });

    it('records under the server take id, uploads each stored chunk, finalizes and clears the device', async () => {
      const api = onlineApi();
      const { q, startTake, stored, store, settle } = await recording(api);
      // No mic and no system audio: the video-only format.
      expect(api.createRecording).toHaveBeenCalledWith({
        mime_type: 'video/webm;codecs=vp9',
        has_system_audio: false,
        has_mic: false,
        has_camera: false,
      });
      expect(startTake.mock.calls[0][0].takeId).toBe(SERVER_TAKE);

      [0, 1, 2].forEach((idx) => stored.next(idx));
      await vi.waitFor(() => expect(api.ack).toHaveBeenCalledTimes(3));

      q<HTMLButtonElement>('recorder-stop')?.click();
      await vi.waitFor(() => expect(api.finalize).toHaveBeenCalledWith(SERVER_TAKE, 3, 12_000));
      await settle();
      expect(store.deleteTake).toHaveBeenCalledWith(SERVER_TAKE);
      const summary = q('recorder-done-summary');
      expect(summary?.textContent?.trim()).toBe('12 s uploaded. Processing has started.');
      expect(summary?.getAttribute('data-uploaded')).toBe('true');
      expect(summary?.getAttribute('data-recording-id')).toBe('rec-1');
      expect(summary?.getAttribute('data-stop-to-finalize-ms')).toMatch(/^\d+$/);
      expect(q('recorder-download')).toBeNull();
      expect(document.getElementById('recorder-done-heading')?.textContent?.trim()).toBe(
        'Recording uploaded',
      );
    });

    describe('the share link', () => {
      let writeText: ReturnType<typeof vi.fn>;
      beforeEach(() => {
        writeText = vi.fn().mockResolvedValue(undefined);
        Object.defineProperty(navigator, 'clipboard', {
          value: { writeText },
          configurable: true,
        });
      });

      async function stopAndUpload(api: ReturnType<typeof onlineApi>, share: SharePort) {
        const rec = await recording(api, share);
        [0, 1, 2].forEach((idx) => rec.stored.next(idx));
        await vi.waitFor(() => expect(api.ack).toHaveBeenCalledTimes(3));
        rec.q<HTMLButtonElement>('recorder-stop')?.click();
        await vi.waitFor(() => expect(api.finalize).toHaveBeenCalled());
        await rec.settle();
        await rec.settle();
        return rec;
      }

      it('is created when the upload completes and copied to the clipboard', async () => {
        const share = fakeShare();
        const { q } = await stopAndUpload(onlineApi(), share);
        expect(share.create).toHaveBeenCalledWith('rec-1', { visibility: 'link' });
        const expected = `${location.origin}/s/abcdefghijkl`;
        expect(writeText).toHaveBeenCalledWith(expected);
        const line = q('recorder-share-link');
        expect(line?.getAttribute('data-copied')).toBe('true');
        expect(line?.textContent).toContain('Link copied');
        expect(line?.querySelector('a')?.getAttribute('href')).toBe(expected);
        expect(q('recorder-copy-link')).toBeNull();
        expect(q('recorder-share')).not.toBeNull();
      });

      it('offers a Copy button when the browser refuses the clipboard', async () => {
        writeText.mockRejectedValueOnce(new DOMException('denied', 'NotAllowedError'));
        const { q, settle } = await stopAndUpload(onlineApi(), fakeShare());
        expect(q('recorder-share-link')?.getAttribute('data-copied')).toBe('false');
        q<HTMLButtonElement>('recorder-copy-link')?.click();
        await settle();
        expect(writeText).toHaveBeenCalledTimes(2);
        expect(q('recorder-share-link')?.getAttribute('data-copied')).toBe('true');
      });

      it('says so when the link cannot be created, and still offers Share', async () => {
        const share = fakeShare();
        (share.create as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('403'));
        const { q } = await stopAndUpload(onlineApi(), share);
        expect(q('recorder-share-link')).toBeNull();
        expect(q('recorder-share-error')?.textContent).toContain("couldn't create its link");
        expect(q('recorder-share')).not.toBeNull();
      });
    });

    it('keeps the take on the device when the server is unreachable at the start', async () => {
      const api = onlineApi();
      api.createRecording.mockRejectedValue(new Error('offline'));
      const { q, startTake, settle } = await recording(api);
      expect(startTake.mock.calls[0][0].takeId).not.toBe(SERVER_TAKE);
      q<HTMLButtonElement>('recorder-stop')?.click();
      await settle();
      expect(api.presign).not.toHaveBeenCalled();
      expect(q('recorder-done-summary')?.textContent?.trim()).toBe('12 s saved on this device.');
      expect(q('recorder-upload-notice')?.textContent).toContain('kept on this device');
    });

    it('keeps the take on the device when finalize is refused', async () => {
      const api = onlineApi();
      api.finalize.mockRejectedValue(new UploadHttpError('api', 409, 'closed'));
      const { q, stored, store, settle } = await recording(api);
      [0, 1, 2].forEach((idx) => stored.next(idx));
      q<HTMLButtonElement>('recorder-stop')?.click();
      await vi.waitFor(() => expect(api.finalize).toHaveBeenCalled());
      await settle();
      expect(store.deleteTake).not.toHaveBeenCalled();
      expect(q('recorder-done-summary')?.getAttribute('data-uploaded')).toBe('false');
      expect(q('recorder-upload-notice')?.textContent).toContain("couldn't be completed");
    });
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
    expect(sources.openMic).toHaveBeenLastCalledWith('mic-a', expect.any(Object));
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
        { provide: PLAN_LIMITS, useValue: signal(planLimits) },
      ],
    });
    const fixture = TestBed.createComponent(RecorderPage);
    fixture.detectChanges();
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="recorder-mobile-notice"]')).not.toBeNull();
    expect(element.querySelector('[data-testid="recorder-setup"]')).toBeNull();
  });
});

function fakeShare(): SharePort {
  return {
    list: vi.fn().mockResolvedValue([]),
    create: vi.fn().mockResolvedValue({
      id: 'link-1',
      recording_id: 'rec-1',
      slug: 'abcdefghijkl',
      visibility: 'link',
      allow_download: false,
      expires_at: null,
      revoked_at: null,
      created_at: '2026-10-05T08:00:00Z',
    }),
    update: vi.fn(),
    revoke: vi.fn().mockResolvedValue(undefined),
  };
}

const SERVER_TAKE = '01a0e7a3-c969-756c-93d0-000000000001';

/** A server that answers everything. */
function onlineApi() {
  return {
    createRecording: vi.fn().mockResolvedValue({
      recording_id: 'rec-1',
      take_id: SERVER_TAKE,
      max_duration_ms: 600_000,
    }),
    presign: vi.fn(async (_take: string, idx: number, count: number) =>
      Array.from({ length: count }, (_, i) => ({ idx: idx + i, url: `https://store/${idx + i}` })),
    ),
    ack: vi.fn().mockResolvedValue(undefined),
    status: vi.fn().mockResolvedValue({ finalized: false, chunks: [] }),
    finalize: vi.fn().mockResolvedValue({ recording_id: 'rec-1' }),
  };
}

/** Where MediaRecorder is missing (jsdom) the page never calls the server. */
function offlineApi(): RecordingsApi {
  const api = onlineApi();
  api.createRecording.mockRejectedValue(new Error('offline'));
  return api;
}
