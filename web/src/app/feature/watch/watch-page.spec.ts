import { ComponentFixture, TestBed } from '@angular/core/testing';
import { ActivatedRoute, convertToParamMap, provideRouter } from '@angular/router';

import { Subject } from 'rxjs';

import { RecordingStatus, STATUS_STREAM } from '../../core/status-stream.service';
import { PlaybackData, WatchApi, WatchData, WatchHttpError } from '../../core/watch-api.service';
import { vi } from 'vitest';

const hlsFake = vi.hoisted(() => {
  const instances: {
    handlers: Map<string, (event: string, data: unknown) => void>;
    nextLevel: number;
  }[] = [];
  class FakeHls {
    static Events = {
      MANIFEST_PARSED: 'manifest',
      LEVEL_SWITCHED: 'switched',
      ERROR: 'error',
      FRAG_LOADED: 'frag',
    };
    static ErrorTypes = { NETWORK_ERROR: 'network', MEDIA_ERROR: 'media' };
    static isSupported = () => true;
    handlers = new Map<string, (event: string, data: unknown) => void>();
    levels = [{ height: 360 }, { height: 720 }];
    nextLevel = -1;
    loadSource = vi.fn();
    attachMedia = vi.fn();
    destroy = vi.fn();
    constructor() {
      instances.push(this);
    }
    on(event: string, handler: (event: string, data: unknown) => void) {
      this.handlers.set(event, handler);
    }
  }
  return { instances, FakeHls };
});
vi.mock('hls.js', () => ({ default: hlsFake.FakeHls }));

import { STATUS_POLL_MAX_MS, STATUS_POLL_MS, WatchPage, pollDelay } from './watch-page';

const READY: WatchData = {
  requirement: 'none',
  title: 'Sprint demo',
  state: 'ready',
  duration_ms: 65_000,
  width: 1280,
  height: 720,
  created_at: '2026-10-05T08:00:00Z',
  allow_download: false,
  can_download: false,
  poster_url: 'https://store/poster.jpg',
  chapters: [],
};
const PLAYBACK: PlaybackData = {
  kind: 'mp4',
  url: 'https://store/default.mp4',
  content_type: 'video/mp4',
  poster_url: 'https://store/poster.jpg',
  expires_in_s: 900,
  duration_ms: 65_000,
};

let lastFixture: ComponentFixture<WatchPage>;

async function open(api: Partial<WatchApi>) {
  TestBed.configureTestingModule({
    imports: [WatchPage],
    providers: [
      provideRouter([]),
      { provide: WatchApi, useValue: api },
      {
        provide: ActivatedRoute,
        useValue: { snapshot: { paramMap: convertToParamMap({ slug: 'abcdefghijkl' }) } },
      },
    ],
  });
  const fixture = TestBed.createComponent(WatchPage);
  lastFixture = fixture;
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve));
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

const q = (root: HTMLElement, id: string) => root.querySelector(`[data-testid="${id}"]`);

describe('WatchPage', () => {
  it('plays the ladder when there is one and lets the viewer pick a quality', async () => {
    const watch = vi.fn().mockResolvedValue(READY);
    const playback = vi
      .fn()
      .mockResolvedValue({ ...PLAYBACK, hls_url: '/api/v1/s/abcdefghijkl/hls/master.m3u8' });
    hlsFake.instances.length = 0;
    const root = await open({ watch, playback });
    // hls.js is loaded lazily: let the import settle.
    await vi.waitFor(() => expect(hlsFake.instances).toHaveLength(1));
    const [instance] = hlsFake.instances;
    expect(q(root, 'watch-quality')).toBeNull();
    expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBeNull();

    instance.handlers.get('manifest')?.('manifest', {
      levels: [{ height: 360 }, { height: 720 }],
    });
    instance.handlers.get('switched')?.('switched', { level: 1 });
    lastFixture.detectChanges();
    const select = q(root, 'watch-quality') as HTMLSelectElement;
    expect(
      Array.from(select.options).map((o) => o.textContent?.replace(/\s+/g, ' ').trim()),
    ).toEqual(['Auto (720p)', '360p', '720p']);

    select.value = '0';
    select.dispatchEvent(new Event('change'));
    expect(instance.nextLevel).toBe(0);
    select.value = '-1';
    select.dispatchEvent(new Event('change'));
    expect(instance.nextLevel).toBe(-1);
  });

  it('resumes where this viewer left off and remembers the position as they watch', async () => {
    localStorage.clear();
    localStorage.setItem('sintade.resume.abcdefghijkl', '30');
    const watch = vi.fn().mockResolvedValue(READY);
    const playback = vi.fn().mockResolvedValue(PLAYBACK);
    const root = await open({ watch, playback });
    const video = q(root, 'watch-video') as HTMLVideoElement;
    Object.defineProperty(video, 'duration', { value: 65, configurable: true });
    video.dispatchEvent(new Event('loadedmetadata'));
    expect(video.currentTime).toBe(30);

    video.currentTime = 41;
    video.dispatchEvent(new Event('pause'));
    expect(localStorage.getItem('sintade.resume.abcdefghijkl')).toBe('41');
    // Watched to the end: next time it starts over.
    video.currentTime = 64;
    video.dispatchEvent(new Event('ended'));
    expect(localStorage.getItem('sintade.resume.abcdefghijkl')).toBeNull();
  });

  it('lists the chapters, marks them on the scrub bar and seeks when one is clicked', async () => {
    const watch = vi.fn().mockResolvedValue({
      ...READY,
      chapters: [
        { start_ms: 0, title: 'Intro' },
        { start_ms: 20_000, title: 'Demo' },
      ],
    });
    const playback = vi.fn().mockResolvedValue(PLAYBACK);
    const root = await open({ watch, playback });
    const video = q(root, 'watch-video') as HTMLVideoElement;
    Object.defineProperty(video, 'duration', { value: 65, configurable: true });
    video.dispatchEvent(new Event('loadedmetadata'));
    lastFixture.detectChanges();

    const buttons = Array.from(root.querySelectorAll('[data-testid="watch-chapter"]'));
    expect(buttons.map((b) => b.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      '0 s Intro',
      '20 s Demo',
    ]);
    // The mark for 0:00 is skipped; 20 s of 65 s sits at ~31 %.
    const marks = root.querySelectorAll<HTMLElement>('[data-testid="watch-chapter-mark"]');
    expect(marks).toHaveLength(1);
    expect(parseFloat(marks[0].style.left)).toBeCloseTo(30.8, 0);

    (buttons[1] as HTMLButtonElement).click();
    lastFixture.detectChanges();
    expect(video.currentTime).toBe(20);
    expect(buttons[1].getAttribute('aria-current')).toBe('true');
    expect(buttons[0].getAttribute('aria-current')).toBeNull();
  });

  it('shows no chapter list when the recording has none', async () => {
    const root = await open({
      watch: vi.fn().mockResolvedValue(READY),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
    });
    expect(q(root, 'watch-chapters')).toBeNull();
  });

  it('plays a ready recording with its poster and offers speeds', async () => {
    const watch = vi.fn().mockResolvedValue(READY);
    const playback = vi.fn().mockResolvedValue(PLAYBACK);
    const root = await open({ watch, playback });

    expect(watch).toHaveBeenCalledWith('abcdefghijkl');
    expect(q(root, 'watch-title')?.textContent).toBe('Sprint demo');
    const video = q(root, 'watch-video') as HTMLVideoElement;
    expect(video.getAttribute('src')).toBe(PLAYBACK.url);
    expect(video.getAttribute('poster')).toBe(PLAYBACK.poster_url);
    const speeds = Array.from(q(root, 'watch-speed')?.querySelectorAll('option') ?? []).map(
      (option) => option.textContent?.trim(),
    );
    expect(speeds).toEqual(['0.5×', '0.75×', '1×', '1.25×', '1.5×', '2×']);
    expect(q(root, 'watch-fullscreen')).not.toBeNull();
  });

  it('changes the playback rate from the speed menu', async () => {
    const root = await open({
      watch: vi.fn().mockResolvedValue(READY),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
    });
    const select = q(root, 'watch-speed') as HTMLSelectElement;
    select.value = '1.5';
    select.dispatchEvent(new Event('change'));
    expect((q(root, 'watch-video') as HTMLVideoElement).playbackRate).toBe(1.5);
  });

  it('seeks five seconds with the arrow keys and ignores keys from the menu', async () => {
    const root = await open({
      watch: vi.fn().mockResolvedValue(READY),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
    });
    const video = q(root, 'watch-video') as HTMLVideoElement;
    const times: number[] = [];
    Object.defineProperty(video, 'duration', { value: 60, configurable: true });
    let current = 20;
    Object.defineProperty(video, 'currentTime', {
      get: () => current,
      set: (value: number) => {
        current = value;
        times.push(value);
      },
      configurable: true,
    });
    const player = q(root, 'watch-player') as HTMLElement;
    player.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    player.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(times).toEqual([25, 20]);

    const select = q(root, 'watch-speed') as HTMLElement;
    select.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(times).toEqual([25, 20]);
  });

  it('shows processing and failed recordings without a player', async () => {
    const processing = await open({
      watch: vi.fn().mockResolvedValue({ ...READY, state: 'processing' }),
      playback: vi.fn(),
    });
    expect(q(processing, 'watch-processing')).not.toBeNull();
    expect(q(processing, 'watch-video')).toBeNull();
    TestBed.resetTestingModule();

    const failed = await open({
      watch: vi.fn().mockResolvedValue({ ...READY, state: 'failed' }),
      playback: vi.fn(),
    });
    expect(q(failed, 'watch-failed')).not.toBeNull();
  });

  it('asks a workspace link to sign in first', async () => {
    const root = await open({
      watch: vi.fn().mockResolvedValue({ requirement: 'login' }),
      playback: vi.fn(),
    });
    expect(q(root, 'watch-login')).not.toBeNull();
    expect(q(root, 'watch-title')).toBeNull();
  });

  it('treats 404 as a link that does not work, and other failures as retryable', async () => {
    const gone = await open({
      watch: vi.fn().mockRejectedValue(new WatchHttpError(404)),
      playback: vi.fn(),
    });
    expect(q(gone, 'watch-not-found')).not.toBeNull();
    TestBed.resetTestingModule();

    const down = await open({
      watch: vi.fn().mockRejectedValue(new WatchHttpError(0)),
      playback: vi.fn(),
    });
    expect(q(down, 'watch-error')).not.toBeNull();
  });

  it('offers Download only to viewers who may, and starts it from the signed URL', async () => {
    const download = vi.fn().mockResolvedValue({
      url: 'https://store/default.mp4?sig=1',
      filename: 'Sprint demo.mp4',
      expires_in_s: 900,
    });
    const hidden = await open({
      watch: vi.fn().mockResolvedValue(READY),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
      download,
    });
    expect(q(hidden, 'watch-download')).toBeNull();
    TestBed.resetTestingModule();

    const root = await open({
      watch: vi.fn().mockResolvedValue({ ...READY, can_download: true }),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
      download,
    });
    const clicked: HTMLAnchorElement[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push(this);
    });
    (q(root, 'watch-download') as HTMLButtonElement).click();
    await new Promise((resolve) => setTimeout(resolve));
    click.mockRestore();
    expect(download).toHaveBeenCalledWith('abcdefghijkl');
    expect(clicked).toHaveLength(1);
    expect(clicked[0].href).toBe('https://store/default.mp4?sig=1');
    expect(clicked[0].download).toBe('Sprint demo.mp4');
  });

  it('says when the download cannot start', async () => {
    const root = await open({
      watch: vi.fn().mockResolvedValue({ ...READY, can_download: true }),
      playback: vi.fn().mockResolvedValue(PLAYBACK),
      download: vi.fn().mockRejectedValue(new WatchHttpError(403)),
    });
    (q(root, 'watch-download') as HTMLButtonElement).click();
    await new Promise((resolve) => setTimeout(resolve));
    expect(q(root, 'watch-download-error')).not.toBeNull();
  });

  describe('while the recording is still processing', () => {
    const PREVIEW: PlaybackData = {
      kind: 'preview',
      url: 'https://store/source.webm',
      content_type: 'video/webm',
      poster_url: null,
      expires_in_s: 900,
      duration_ms: null,
    };
    const PROCESSING: WatchData = { ...READY, state: 'processing', poster_url: null };

    beforeEach(() => {
      vi.useFakeTimers();
      vi.spyOn(HTMLMediaElement.prototype, 'canPlayType').mockImplementation((type) =>
        type.startsWith('video/webm') ? 'maybe' : '',
      );
    });
    afterEach(() => {
      vi.useRealTimers();
      vi.restoreAllMocks();
    });

    async function openFake(api: Partial<WatchApi>, stream?: Subject<RecordingStatus>) {
      TestBed.configureTestingModule({
        imports: [WatchPage],
        providers: [
          provideRouter([]),
          ...(stream ? [{ provide: STATUS_STREAM, useValue: { follow: () => stream } }] : []),
          { provide: WatchApi, useValue: api },
          {
            provide: ActivatedRoute,
            useValue: { snapshot: { paramMap: convertToParamMap({ slug: 'abcdefghijkl' }) } },
          },
        ],
      });
      const fixture = TestBed.createComponent(WatchPage);
      const root = fixture.nativeElement as HTMLElement;
      const settle = async (ms = 0) => {
        await vi.advanceTimersByTimeAsync(ms);
        fixture.detectChanges();
        await vi.advanceTimersByTimeAsync(0);
        fixture.detectChanges();
      };
      fixture.detectChanges();
      await settle();
      return { root, settle };
    }

    it('shows a notice, checks again every second, and plays the MP4 once it exists', async () => {
      const watch = vi
        .fn()
        .mockResolvedValueOnce(PROCESSING)
        .mockResolvedValueOnce(PROCESSING)
        .mockResolvedValue(READY);
      const playback = vi
        .fn()
        .mockRejectedValueOnce(new WatchHttpError(409))
        .mockRejectedValueOnce(new WatchHttpError(409))
        .mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback });
      expect(q(root, 'watch-processing')).not.toBeNull();
      expect(q(root, 'watch-video')).toBeNull();

      await settle(STATUS_POLL_MS);
      expect(watch).toHaveBeenCalledTimes(2);
      expect(q(root, 'watch-processing')).not.toBeNull();

      await settle(pollDelay(1));
      expect(q(root, 'watch-processing')).toBeNull();
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
      expect(q(root, 'watch-preview')).toBeNull();

      // Done polling.
      await settle(STATUS_POLL_MS * 3);
      expect(watch).toHaveBeenCalledTimes(3);
    });

    it('plays the original as a preview straight away, then switches to the MP4', async () => {
      const watch = vi.fn().mockResolvedValueOnce(PROCESSING).mockResolvedValue(READY);
      const playback = vi.fn().mockResolvedValueOnce(PREVIEW).mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback });
      const video = q(root, 'watch-video') as HTMLVideoElement;
      expect(video.getAttribute('src')).toBe(PREVIEW.url);
      expect(q(root, 'watch-preview')).not.toBeNull();
      expect(q(root, 'watch-processing')).toBeNull();

      // Nobody is watching yet (paused), so the MP4 takes over when it appears.
      await settle(STATUS_POLL_MS);
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
      expect(q(root, 'watch-preview')).toBeNull();
    });

    it('waits for a pause before swapping while the preview is playing', async () => {
      const watch = vi.fn().mockResolvedValueOnce(PROCESSING).mockResolvedValue(READY);
      const playback = vi.fn().mockResolvedValueOnce(PREVIEW).mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback });
      const video = q(root, 'watch-video') as HTMLVideoElement;
      Object.defineProperty(video, 'paused', { value: false, configurable: true });

      await settle(STATUS_POLL_MS);
      expect(video.getAttribute('src')).toBe(PREVIEW.url);
      expect(q(root, 'watch-preview')).not.toBeNull();

      Object.defineProperty(video, 'paused', { value: true, configurable: true });
      video.dispatchEvent(new Event('pause'));
      await settle();
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
    });

    it('keeps the processing notice when this browser cannot play the original', async () => {
      vi.spyOn(HTMLMediaElement.prototype, 'canPlayType').mockReturnValue('');
      const { root } = await openFake({
        watch: vi.fn().mockResolvedValue(PROCESSING),
        playback: vi.fn().mockResolvedValue(PREVIEW),
      });
      expect(q(root, 'watch-processing')).not.toBeNull();
      expect(q(root, 'watch-video')).toBeNull();
    });

    it('stops polling when the page goes away', async () => {
      const watch = vi.fn().mockResolvedValue(PROCESSING);
      const { settle } = await openFake({
        watch,
        playback: vi.fn().mockRejectedValue(new WatchHttpError(409)),
      });
      TestBed.resetTestingModule();
      await settle(STATUS_POLL_MS * 3);
      expect(watch).toHaveBeenCalledTimes(1);
    });

    const status = (state: string): RecordingStatus => ({
      state,
      hls: false,
      sprite: false,
      preview: false,
    });

    /** Day 82's Check: the page moves on when the server says so, without a timer asking. */
    it('waits on the live status stream and plays the MP4 the moment it is ready', async () => {
      const stream = new Subject<RecordingStatus>();
      const watch = vi.fn().mockResolvedValueOnce(PROCESSING).mockResolvedValue(READY);
      const playback = vi
        .fn()
        .mockRejectedValueOnce(new WatchHttpError(409))
        .mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback }, stream);
      expect(q(root, 'watch-processing')).not.toBeNull();

      stream.next(status('processing')); // what the page already knows
      await settle(STATUS_POLL_MAX_MS * 3); // no timer is asking
      expect(watch).toHaveBeenCalledTimes(1);
      expect(q(root, 'watch-processing')).not.toBeNull();

      stream.next(status('ready'));
      await settle();
      expect(watch).toHaveBeenCalledTimes(2);
      expect(q(root, 'watch-processing')).toBeNull();
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
      expect(stream.observed).toBe(false); // closed once it has what it needed
    });

    it('asks now and then instead when the status stream cannot be had', async () => {
      const stream = new Subject<RecordingStatus>();
      const watch = vi.fn().mockResolvedValueOnce(PROCESSING).mockResolvedValue(READY);
      const playback = vi
        .fn()
        .mockRejectedValueOnce(new WatchHttpError(409))
        .mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback }, stream);

      stream.error(new Error('refused'));
      await settle(pollDelay(0));
      expect(watch).toHaveBeenCalledTimes(2);
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
    });

    it('closes the status stream when the page goes away', async () => {
      const stream = new Subject<RecordingStatus>();
      await openFake(
        {
          watch: vi.fn().mockResolvedValue(PROCESSING),
          playback: vi.fn().mockRejectedValue(new WatchHttpError(409)),
        },
        stream,
      );
      expect(stream.observed).toBe(true);
      TestBed.resetTestingModule();
      expect(stream.observed).toBe(false);
    });

    it('slows its checks down the longer a recording takes', () => {
      expect(pollDelay(0)).toBe(STATUS_POLL_MS);
      const delays = Array.from({ length: 12 }, (_, attempt) => pollDelay(attempt));
      expect(delays).toEqual([...delays].sort((a, b) => a - b));
      expect(delays.at(-1)).toBe(STATUS_POLL_MAX_MS);
      // A tab left open for a long recording stays far below 120 requests a minute (ADR-0022).
      const perMinuteAtTheSlowest = (60_000 / STATUS_POLL_MAX_MS) * 2;
      expect(perMinuteAtTheSlowest).toBeLessThan(20);
    });

    it('keeps what is on screen and waits when the server says to slow down', async () => {
      const watch = vi
        .fn()
        .mockResolvedValueOnce(PROCESSING)
        .mockRejectedValueOnce(new WatchHttpError(429))
        .mockResolvedValue(READY);
      const playback = vi.fn().mockResolvedValueOnce(PREVIEW).mockResolvedValue(PLAYBACK);
      const { root, settle } = await openFake({ watch, playback });
      expect(q(root, 'watch-preview')).not.toBeNull();

      await settle(pollDelay(0)); // 429: nothing changes, no error page
      expect(q(root, 'watch-error')).toBeNull();
      expect(q(root, 'watch-preview')).not.toBeNull();

      await settle(pollDelay(1)); // asked again: the MP4 is there
      expect((q(root, 'watch-video') as HTMLVideoElement).getAttribute('src')).toBe(PLAYBACK.url);
    });
  });
});
