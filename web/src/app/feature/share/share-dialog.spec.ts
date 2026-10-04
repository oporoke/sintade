import { TestBed } from '@angular/core/testing';

import { SHARE_API, ShareLink, SharePort } from '../../core/share-api.service';
import { ShareDialog } from './share-dialog';

const LINK: ShareLink = {
  id: 'link-1',
  recording_id: 'rec-1',
  slug: 'abcdefghijkl',
  visibility: 'link',
  allow_download: false,
  expires_at: null,
  revoked_at: null,
  created_at: '2026-10-05T08:00:00Z',
};

function port(links: ShareLink[]): SharePort & Record<keyof SharePort, ReturnType<typeof vi.fn>> {
  return {
    list: vi.fn().mockResolvedValue(links),
    create: vi.fn().mockResolvedValue(LINK),
    update: vi.fn(async (_r: string, _l: string, body: object) => ({ ...LINK, ...body })),
    revoke: vi.fn().mockResolvedValue(undefined),
  };
}

async function open(api: SharePort) {
  TestBed.configureTestingModule({
    imports: [ShareDialog],
    providers: [{ provide: SHARE_API, useValue: api }],
  });
  const fixture = TestBed.createComponent(ShareDialog);
  fixture.componentRef.setInput('recordingId', 'rec-1');
  fixture.detectChanges();
  await fixture.componentInstance.open();
  fixture.detectChanges();
  const root = fixture.nativeElement as HTMLElement;
  const q = <T extends Element>(id: string) => root.querySelector<T>(`[data-testid="${id}"]`);
  const settle = async () => {
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
  };
  return { q, settle };
}

describe('ShareDialog', () => {
  it('shows the live link with its visibility', async () => {
    const api = port([{ ...LINK, id: 'old', revoked_at: '2026-10-01T00:00:00Z' }, LINK].reverse());
    const { q } = await open(api);
    expect(api.list).toHaveBeenCalledWith('rec-1');
    expect(q<HTMLInputElement>('share-url')?.value).toBe(`${location.origin}/s/abcdefghijkl`);
    expect(q<HTMLSelectElement>('share-visibility')?.value).toBe('link');
    expect(q('share-dialog')?.hasAttribute('open')).toBe(true);
  });

  it('saves a visibility change and the download flag as they are made', async () => {
    const api = port([LINK]);
    const { q, settle } = await open(api);
    const select = q<HTMLSelectElement>('share-visibility');
    if (!select) throw new Error('no select');
    select.value = 'private';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect(api.update).toHaveBeenCalledWith('rec-1', 'link-1', { visibility: 'private' });
    expect(q<HTMLSelectElement>('share-visibility')?.value).toBe('private');

    const download = q<HTMLInputElement>('share-download');
    if (!download) throw new Error('no checkbox');
    download.checked = true;
    download.dispatchEvent(new Event('change'));
    await settle();
    expect(api.update).toHaveBeenCalledWith('rec-1', 'link-1', { allow_download: true });
  });

  it('copies the link', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const { q, settle } = await open(port([LINK]));
    q<HTMLButtonElement>('share-copy')?.click();
    await settle();
    expect(writeText).toHaveBeenCalledWith(`${location.origin}/s/abcdefghijkl`);
    expect(q('share-copied')).not.toBeNull();
  });

  it('turns the link off, then can create a new one', async () => {
    const api = port([LINK]);
    const { q, settle } = await open(api);
    q<HTMLButtonElement>('share-revoke')?.click();
    await settle();
    expect(api.revoke).toHaveBeenCalledWith('rec-1', 'link-1');
    expect(q('share-no-link')).not.toBeNull();
    q<HTMLButtonElement>('share-create')?.click();
    await settle();
    expect(api.create).toHaveBeenCalledWith('rec-1', { visibility: 'link' });
    expect(q('share-url')).not.toBeNull();
  });

  it('reports failures without losing the dialog', async () => {
    const api = port([LINK]);
    api.update.mockRejectedValue(new Error('403'));
    const { q, settle } = await open(api);
    const select = q<HTMLSelectElement>('share-visibility');
    if (!select) throw new Error('no select');
    select.value = 'public';
    select.dispatchEvent(new Event('change'));
    await settle();
    expect(q('share-error')?.textContent).toContain("didn't work");
    expect(q('share-url')).not.toBeNull();
  });
});
