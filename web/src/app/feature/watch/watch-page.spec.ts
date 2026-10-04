import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, convertToParamMap, provideRouter } from '@angular/router';

import { PlaybackData, WatchApi, WatchData, WatchHttpError } from '../../core/watch-api.service';
import { WatchPage } from './watch-page';

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
};
const PLAYBACK: PlaybackData = {
  mp4_url: 'https://store/default.mp4',
  poster_url: 'https://store/poster.jpg',
  expires_in_s: 900,
  duration_ms: 65_000,
};

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
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve));
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

const q = (root: HTMLElement, id: string) => root.querySelector(`[data-testid="${id}"]`);

describe('WatchPage', () => {
  it('plays a ready recording with its poster and offers speeds', async () => {
    const watch = vi.fn().mockResolvedValue(READY);
    const playback = vi.fn().mockResolvedValue(PLAYBACK);
    const root = await open({ watch, playback });

    expect(watch).toHaveBeenCalledWith('abcdefghijkl');
    expect(q(root, 'watch-title')?.textContent).toBe('Sprint demo');
    const video = q(root, 'watch-video') as HTMLVideoElement;
    expect(video.getAttribute('src')).toBe(PLAYBACK.mp4_url);
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
});
