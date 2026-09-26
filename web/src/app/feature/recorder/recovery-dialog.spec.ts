import { TestBed } from '@angular/core/testing';

import { ChunkStore, TakeMeta } from '../../capture';
import { CHUNK_STORE } from '../../core/capture.tokens';
import { RecoveryDialog } from './recovery-dialog';

const TAKE = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b';

function fakeStore(takes: Record<string, { meta: TakeMeta | null; chunks: string[] }>) {
  const state = structuredClone(takes);
  const store = {
    backend: 'indexeddb',
    takes: vi.fn(async () => Object.keys(state)),
    indexes: vi.fn(async (id: string) => state[id]?.chunks.map((_, i) => i) ?? []),
    get: vi.fn(async (id: string, index: number) => new Blob([state[id]?.chunks[index] ?? ''])),
    getMeta: vi.fn(async (id: string) => state[id]?.meta ?? null),
    deleteTake: vi.fn(async (id: string) => {
      delete state[id];
    }),
    put: vi.fn(),
    putMeta: vi.fn(),
  };
  return store as unknown as ChunkStore & typeof store;
}

/** The dialog loads through a chain of untracked promises; let them all run, then render. */
async function settle(fixture: { detectChanges(): void }) {
  for (let i = 0; i < 5; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    fixture.detectChanges();
  }
}

async function setup(store: ChunkStore) {
  TestBed.configureTestingModule({
    imports: [RecoveryDialog],
    providers: [{ provide: CHUNK_STORE, useValue: Promise.resolve(store) }],
  });
  const fixture = TestBed.createComponent(RecoveryDialog);
  await settle(fixture);
  const element: HTMLElement = fixture.nativeElement;
  const dialog = element.querySelector<HTMLDialogElement>('[data-testid="recovery-dialog"]');
  return { fixture, element, dialog };
}

describe('RecoveryDialog', () => {
  it('stays closed when nothing was left behind', async () => {
    const { dialog } = await setup(fakeStore({}));
    expect(dialog?.open).toBe(false);
  });

  it('opens with the start time and recorded length of each unfinished take', async () => {
    const startedAt = new Date().setHours(14, 2, 0, 0);
    const store = fakeStore({
      [TAKE]: {
        meta: {
          takeId: TAKE,
          startedAt,
          mimeType: 'video/webm',
          chunkCount: 2,
          durationMs: 380_000,
        },
        chunks: ['a', 'b'],
      },
    });
    const { dialog, element } = await setup(store);

    expect(dialog?.open).toBe(true);
    const summary = element.querySelector('[data-testid="recovery-summary"]')?.textContent ?? '';
    // Locale-dependent clock format ("14:02" or "02:02 PM"); the length is what matters here.
    expect(summary).toMatch(/^Unfinished recording from .*02.*, 6 min 20 s$/);
  });

  it('says so when the journal is missing', async () => {
    const { element } = await setup(fakeStore({ [TAKE]: { meta: null, chunks: ['a'] } }));
    expect(element.querySelector('[data-testid="recovery-summary"]')?.textContent?.trim()).toBe(
      'Unfinished recording (start time and length unknown)',
    );
  });

  it('discard deletes the take and closes the dialog when none are left', async () => {
    const store = fakeStore({ [TAKE]: { meta: null, chunks: ['a'] } });
    const { fixture, element, dialog } = await setup(store);
    element.querySelector<HTMLButtonElement>('[data-testid="recovery-discard"]')?.click();
    await settle(fixture);

    expect(store.deleteTake).toHaveBeenCalledWith(TAKE);
    expect(element.querySelectorAll('[data-testid="recovery-take"]')).toHaveLength(0);
    expect(dialog?.open).toBe(false);
  });

  it('keeps Upload disabled until uploads exist', async () => {
    const { element } = await setup(fakeStore({ [TAKE]: { meta: null, chunks: ['a'] } }));
    const upload = [...element.querySelectorAll('button')].find(
      (button) => button.textContent?.trim() === 'Upload',
    );
    expect(upload?.disabled).toBe(true);
  });
});
