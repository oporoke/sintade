import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, convertToParamMap, provideRouter } from '@angular/router';

import { EditorApi, EditorHttpError, EditorSource } from '../../core/editor-api.service';
import { EditorPage, WAVEFORM_RETRY_MS } from './editor-page';
import { Waveform } from './waveform';

const q = (root: HTMLElement, id: string) => root.querySelector(`[data-testid="${id}"]`);

/** A 12 s original with one sprite cue a second. */
function source(): EditorSource {
  const cues = Array.from({ length: 12 }, (_, i) => {
    const clock = (s: number) => `00:00:${String(s).padStart(2, '0')}.000`;
    return `${clock(i)} --> ${clock(i + 1)}\nhttps://s/sprite_0.jpg?sig#xywh=${(i % 10) * 160},${Math.floor(i / 10) * 90},160,90`;
  });
  return {
    video_url: 'https://s/mp4/default.mp4?sig',
    duration_ms: 12_000,
    sprite_vtt: `WEBVTT\n\n${cues.join('\n\n')}\n`,
    expires_in_s: 900,
  };
}

const peaks: Waveform = {
  version: 1,
  duration_ms: 12_000,
  bucket_ms: 50,
  has_audio: true,
  peaks: Array.from({ length: 240 }, (_, i) => i % 256),
};

function configure(api: Partial<EditorApi>) {
  TestBed.configureTestingModule({
    imports: [EditorPage],
    providers: [
      provideRouter([]),
      { provide: EditorApi, useValue: api },
      {
        provide: ActivatedRoute,
        useValue: { snapshot: { paramMap: convertToParamMap({ id: 'rec-1' }) } },
      },
    ],
  });
  return TestBed.createComponent(EditorPage);
}

async function open(api: Partial<EditorApi>) {
  const fixture = configure(api);
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve));
  fixture.detectChanges();
  return { fixture, root: fixture.nativeElement as HTMLElement };
}

/** Like `open`, with fake timers: `settle(ms)` moves the clock on and renders. */
async function openFake(api: Partial<EditorApi>) {
  const fixture = configure(api);
  const settle = async (ms = 0) => {
    await vi.advanceTimersByTimeAsync(ms);
    fixture.detectChanges();
    await vi.advanceTimersByTimeAsync(0);
    fixture.detectChanges();
  };
  fixture.detectChanges();
  await settle();
  return { root: fixture.nativeElement as HTMLElement, settle };
}

describe('EditorPage', () => {
  afterEach(() => vi.useRealTimers());

  // Day 93 Check: the timeline spans exactly the original's length.
  it('shows the original under a timeline as long as it, thumbnails edge to edge', async () => {
    const sourceCall = vi.fn().mockResolvedValue(source());
    const { root } = await open({ source: sourceCall, waveform: vi.fn().mockResolvedValue(peaks) });
    expect(sourceCall).toHaveBeenCalledWith('rec-1');
    expect((q(root, 'editor-video') as HTMLVideoElement).src).toBe(source().video_url);
    const timeline = q(root, 'editor-timeline') as HTMLElement;
    expect(timeline.dataset['durationMs']).toBe('12000');
    expect(timeline.getAttribute('aria-valuemax')).toBe('12000');
    expect(q(root, 'editor-duration')?.textContent?.trim()).toBe('0:12');
    expect(q(root, 'editor-time')?.textContent?.trim()).toBe('0:00');

    const thumbs = Array.from(root.querySelectorAll<HTMLElement>('[data-testid="editor-thumb"]'));
    // 800 px (no layout here) of 96 px thumbnails.
    expect(thumbs).toHaveLength(8);
    expect(thumbs[0].dataset['startMs']).toBe('0');
    expect(thumbs[thumbs.length - 1].dataset['endMs']).toBe('12000');
    expect((thumbs[0].firstElementChild as HTMLElement).style.backgroundImage).toContain(
      'https://s/sprite_0.jpg?sig',
    );
    expect(q(root, 'editor-waveform')?.getAttribute('data-state')).toBe('ready');
  });

  it('seeks to where the timeline is clicked, its right edge being the end', async () => {
    const { fixture, root } = await open({
      source: vi.fn().mockResolvedValue(source()),
      waveform: vi.fn().mockResolvedValue(peaks),
    });
    const timeline = q(root, 'editor-timeline') as HTMLElement;
    timeline.getBoundingClientRect = () => ({ left: 100, width: 400 }) as DOMRect;
    const video = q(root, 'editor-video') as HTMLVideoElement;

    timeline.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, pointerId: 1 }));
    fixture.detectChanges();
    expect(video.currentTime).toBe(12);
    expect(q(root, 'editor-time')?.textContent?.trim()).toBe('0:12');
    expect((q(root, 'editor-playhead') as HTMLElement).style.left).toBe('100%');

    timeline.dispatchEvent(new PointerEvent('pointerdown', { clientX: 200, pointerId: 1 }));
    fixture.detectChanges();
    expect(video.currentTime).toBe(3);
    expect((q(root, 'editor-playhead') as HTMLElement).style.left).toBe('25%');

    timeline.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home' }));
    fixture.detectChanges();
    expect(video.currentTime).toBe(0);
    timeline.dispatchEvent(new KeyboardEvent('keydown', { key: 'End' }));
    fixture.detectChanges();
    expect(video.currentTime).toBe(12);
  });

  it('asks for the peaks again while they are being made', async () => {
    vi.useFakeTimers();
    const waveform = vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce(peaks);
    const { root, settle } = await openFake({
      source: vi.fn().mockResolvedValue(source()),
      waveform,
    });
    expect(q(root, 'editor-waveform-pending')).not.toBeNull();
    await settle(WAVEFORM_RETRY_MS);
    expect(waveform).toHaveBeenCalledTimes(2);
    expect(q(root, 'editor-waveform-pending')).toBeNull();
    expect(q(root, 'editor-waveform')?.getAttribute('data-state')).toBe('ready');
  });

  it('works without thumbnails when the recording has no sprite', async () => {
    const { root } = await open({
      source: vi.fn().mockResolvedValue({ ...source(), sprite_vtt: null }),
      waveform: vi.fn().mockResolvedValue(peaks),
    });
    expect(q(root, 'editor-timeline')).not.toBeNull();
    expect(q(root, 'editor-thumbs')).toBeNull();
  });

  it('says so when the recording is missing or still processing', async () => {
    const missing = await open({
      source: vi.fn().mockRejectedValue(new EditorHttpError(404)),
      waveform: vi.fn(),
    });
    expect(q(missing.root, 'editor-not-found')).not.toBeNull();
    TestBed.resetTestingModule();
    const processing = await open({
      source: vi.fn().mockRejectedValue(new EditorHttpError(409)),
      waveform: vi.fn(),
    });
    expect(q(processing.root, 'editor-processing')).not.toBeNull();
    expect(q(processing.root, 'editor-timeline')).toBeNull();
  });
});
