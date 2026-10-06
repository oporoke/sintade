import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { Subject } from 'rxjs';

import { LIBRARY_API, LibraryPort, RecordingSummary } from '../../core/library-api.service';
import { RecordingStatus, STATUS_STREAM } from '../../core/status-stream.service';
import { SHARE_API } from '../../core/share-api.service';
import { LibraryPage } from './library-page';

function item(n: number, patch: Partial<RecordingSummary> = {}): RecordingSummary {
  return {
    id: `rec-${n}`,
    title: `Recording ${n}`,
    state: 'ready',
    duration_ms: 65_000,
    created_at: '2026-10-05T08:00:00Z',
    poster_url: `https://store/poster-${n}.jpg`,
    ...patch,
  };
}

async function open(
  api: Partial<LibraryPort>,
  streams: Record<string, Subject<RecordingStatus>> = {},
) {
  TestBed.configureTestingModule({
    imports: [LibraryPage],
    providers: [
      provideRouter([]),
      {
        provide: STATUS_STREAM,
        useValue: {
          follow: (path: string) => {
            const id = path.split('/')[2];
            return streams[id] ?? new Subject<RecordingStatus>();
          },
        },
      },
      { provide: LIBRARY_API, useValue: api },
      {
        provide: SHARE_API,
        useValue: { list: vi.fn().mockResolvedValue([]), create: vi.fn() },
      },
    ],
  });
  const fixture = TestBed.createComponent(LibraryPage);
  fixture.detectChanges();
  const settle = async () => {
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
  };
  await settle();
  const root = fixture.nativeElement as HTMLElement;
  const all = (id: string) => Array.from(root.querySelectorAll(`[data-testid="${id}"]`));
  return { root, all, settle, q: (id: string) => root.querySelector(`[data-testid="${id}"]`) };
}

describe('LibraryPage', () => {
  it('lists recordings with thumbnail, length, date and state', async () => {
    const list = vi.fn().mockResolvedValue({
      items: [item(1), item(2, { state: 'processing', poster_url: null, duration_ms: null })],
      next_cursor: null,
    });
    const { all, q } = await open({ list });
    expect(list).toHaveBeenCalledWith(null);
    const cards = all('library-item');
    expect(cards).toHaveLength(2);
    expect(cards[0].querySelector('img')?.getAttribute('src')).toBe('https://store/poster-1.jpg');
    expect(cards[0].querySelector('[data-testid="library-title"]')?.textContent?.trim()).toBe(
      'Recording 1',
    );
    expect(cards[0].querySelector('[data-testid="library-duration"]')?.textContent).toBe(
      '1 min 5 s',
    );
    expect(cards[0].querySelector('time')?.getAttribute('datetime')).toBe('2026-10-05T08:00:00Z');
    expect(cards[0].querySelector('[data-testid="library-state"]')).toBeNull();
    expect(cards[0].querySelector('[data-testid="library-download"]')).not.toBeNull();
    // A recording still being processed: placeholder, state label, no download.
    expect(cards[1].querySelector('img')).toBeNull();
    expect(cards[1].querySelector('[data-testid="library-state"]')?.textContent).toBe('Processing');
    expect(cards[1].querySelector('[data-testid="library-download"]')).toBeNull();
    expect(q('library-more')).toBeNull();
    expect(q('library-empty')).toBeNull();
  });

  it('shows an empty state that points at the recorder', async () => {
    const { q } = await open({ list: vi.fn().mockResolvedValue({ items: [], next_cursor: null }) });
    expect(q('library-empty')).not.toBeNull();
    expect(q('library-record')?.getAttribute('href')).toBe('/record');
  });

  it('loads the next page with the cursor and appends it', async () => {
    const list = vi
      .fn()
      .mockResolvedValueOnce({ items: [item(1)], next_cursor: 'c1' })
      .mockResolvedValueOnce({ items: [item(2)], next_cursor: null });
    const { all, q, settle } = await open({ list });
    expect(all('library-item')).toHaveLength(1);
    (q('library-more') as HTMLButtonElement).click();
    await settle();
    expect(list).toHaveBeenLastCalledWith('c1');
    expect(all('library-item').map((c) => c.getAttribute('data-recording-id'))).toEqual([
      'rec-1',
      'rec-2',
    ]);
    expect(q('library-more')).toBeNull();
  });

  it('reports a failed load and retries', async () => {
    const list = vi
      .fn()
      .mockRejectedValueOnce(new Error('down'))
      .mockResolvedValueOnce({ items: [item(1)], next_cursor: null });
    const { all, q, settle } = await open({ list });
    expect(q('library-error')).not.toBeNull();
    expect(q('library-empty')).toBeNull();
    (q('library-error')?.querySelector('button') as HTMLButtonElement).click();
    await settle();
    expect(q('library-error')).toBeNull();
    expect(all('library-item')).toHaveLength(1);
  });

  it('opens the Share dialog for the chosen recording', async () => {
    const list = vi.fn().mockResolvedValue({ items: [item(1), item(2)], next_cursor: null });
    const shareList = vi.fn().mockResolvedValue([]);
    TestBed.configureTestingModule({
      imports: [LibraryPage],
      providers: [
        provideRouter([]),
        { provide: LIBRARY_API, useValue: { list } },
        { provide: SHARE_API, useValue: { list: shareList, create: vi.fn() } },
      ],
    });
    const fixture = TestBed.createComponent(LibraryPage);
    fixture.detectChanges();
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    const buttons = fixture.nativeElement.querySelectorAll('[data-testid="library-share"]');
    (buttons[1] as HTMLButtonElement).click();
    await new Promise((resolve) => setTimeout(resolve));
    expect(shareList).toHaveBeenCalledWith('rec-2');
  });

  it('starts a download from the signed URL', async () => {
    const download = vi.fn().mockResolvedValue({
      url: 'https://store/x.mp4?sig',
      filename: 'Recording 1.mp4',
      expires_in_s: 900,
    });
    const { all, settle } = await open({
      list: vi.fn().mockResolvedValue({ items: [item(1)], next_cursor: null }),
      download,
    });
    const clicked: HTMLAnchorElement[] = [];
    const spy = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push(this);
    });
    (all('library-download')[0] as HTMLButtonElement).click();
    await settle();
    spy.mockRestore();
    expect(download).toHaveBeenCalledWith('rec-1');
    expect(clicked[0].download).toBe('Recording 1.mp4');
  });

  it('renames a title in place: Enter saves, Escape cancels', async () => {
    const rename = vi.fn().mockResolvedValue({ id: 'rec-1', title: 'Better name' });
    const { all, q, settle } = await open({
      list: vi.fn().mockResolvedValue({ items: [item(1)], next_cursor: null }),
      rename,
    });
    (q('library-rename') as HTMLButtonElement).click();
    await settle();
    let input = q('library-title-input') as HTMLInputElement;
    expect(input.value).toBe('Recording 1');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await settle();
    expect(q('library-title-input')).toBeNull();
    expect(rename).not.toHaveBeenCalled();

    (q('library-rename') as HTMLButtonElement).click();
    await settle();
    input = q('library-title-input') as HTMLInputElement;
    input.value = 'Better name';
    input.form?.dispatchEvent(new Event('submit', { cancelable: true }));
    await settle();
    expect(rename).toHaveBeenCalledWith('rec-1', 'Better name');
    expect(all('library-title')[0].textContent?.trim()).toBe('Better name');
    expect(q('library-title-input')).toBeNull();
  });

  it('keeps the editor open and says so when the rename fails', async () => {
    const { q, settle } = await open({
      list: vi.fn().mockResolvedValue({ items: [item(1)], next_cursor: null }),
      rename: vi.fn().mockRejectedValue(new Error('422')),
    });
    (q('library-rename') as HTMLButtonElement).click();
    await settle();
    const input = q('library-title-input') as HTMLInputElement;
    input.value = 'x';
    input.form?.dispatchEvent(new Event('submit', { cancelable: true }));
    await settle();
    expect(q('library-notice')?.textContent).toContain("couldn't be saved");
    expect(q('library-title-input')).not.toBeNull();
  });

  it('asks before moving to the trash, then removes the card', async () => {
    const trash = vi.fn().mockResolvedValue(undefined);
    const { all, q, settle } = await open({
      list: vi.fn().mockResolvedValue({ items: [item(1), item(2)], next_cursor: null }),
      trash,
    });
    (all('library-trash')[0] as HTMLButtonElement).click();
    await settle();
    expect(trash).not.toHaveBeenCalled();
    (q('library-trash-no') as HTMLButtonElement).click();
    await settle();
    expect(trash).not.toHaveBeenCalled();

    (all('library-trash')[0] as HTMLButtonElement).click();
    await settle();
    (q('library-trash-yes') as HTMLButtonElement).click();
    await settle();
    expect(trash).toHaveBeenCalledWith('rec-1');
    expect(all('library-item').map((c) => c.getAttribute('data-recording-id'))).toEqual(['rec-2']);
    expect(q('library-notice')?.textContent).toContain('30 days');
  });

  describe('live status', () => {
    const status = (state: string): RecordingStatus => ({
      state,
      hls: false,
      sprite: false,
      preview: false,
    });

    /** Day 82's Check: a card that is processing becomes ready without a reload. */
    it('turns a processing card into a ready one when the stream says so', async () => {
      const processing = item(2, { state: 'processing', poster_url: null, duration_ms: null });
      const list = vi
        .fn()
        .mockResolvedValueOnce({ items: [item(1), processing], next_cursor: null })
        .mockResolvedValue({
          items: [item(1), item(2, { poster_url: 'https://store/poster-2.jpg' })],
          next_cursor: null,
        });
      const stream = new Subject<RecordingStatus>();
      const { all, settle } = await open({ list }, { 'rec-2': stream });
      const card = () => all('library-item')[1];
      expect(card().querySelector('[data-testid="library-state"]')?.textContent).toBe('Processing');
      expect(card().querySelector('img')).toBeNull();

      stream.next(status('ready'));
      await settle();
      await settle();
      expect(card().querySelector('[data-testid="library-state"]')).toBeNull();
      expect(card().querySelector('img')?.getAttribute('src')).toBe('https://store/poster-2.jpg');
      expect(card().querySelector('[data-testid="library-download"]')).not.toBeNull();
      expect(stream.observed).toBe(false);
    });

    it('shows a recording that failed as failed, and follows only unfinished ones', async () => {
      const follow = vi.fn((path: string) => {
        void path;
        return new Subject<RecordingStatus>();
      });
      TestBed.configureTestingModule({
        imports: [LibraryPage],
        providers: [
          provideRouter([]),
          { provide: STATUS_STREAM, useValue: { follow } },
          {
            provide: LIBRARY_API,
            useValue: {
              list: vi.fn().mockResolvedValue({
                items: [item(1), item(2, { state: 'processing' }), item(3, { state: 'failed' })],
                next_cursor: null,
              }),
            },
          },
          { provide: SHARE_API, useValue: { list: vi.fn(), create: vi.fn() } },
        ],
      });
      const fixture = TestBed.createComponent(LibraryPage);
      fixture.detectChanges();
      await new Promise((resolve) => setTimeout(resolve));
      expect(follow).toHaveBeenCalledTimes(1);
      expect(follow).toHaveBeenCalledWith('/recordings/rec-2/events');
    });

    it('closes its streams when the page goes away', async () => {
      const stream = new Subject<RecordingStatus>();
      const list = vi.fn().mockResolvedValue({
        items: [item(2, { state: 'processing' })],
        next_cursor: null,
      });
      await open({ list }, { 'rec-2': stream });
      expect(stream.observed).toBe(true);
      TestBed.resetTestingModule();
      expect(stream.observed).toBe(false);
    });
  });
});
