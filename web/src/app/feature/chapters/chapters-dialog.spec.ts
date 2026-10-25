import { TestBed } from '@angular/core/testing';

import { ChaptersApi } from '../../core/chapters-api.service';
import { ChaptersDialog } from './chapters-dialog';

const q = (root: HTMLElement, id: string) => root.querySelector(`[data-testid="${id}"]`);

async function open(api: Partial<ChaptersApi>) {
  TestBed.configureTestingModule({
    imports: [ChaptersDialog],
    providers: [{ provide: ChaptersApi, useValue: api }],
  });
  const fixture = TestBed.createComponent(ChaptersDialog);
  fixture.detectChanges();
  await fixture.componentInstance.open('rec-1');
  fixture.detectChanges();
  return { fixture, root: fixture.nativeElement as HTMLElement };
}

function type(root: HTMLElement, text: string) {
  const area = q(root, 'chapters-text') as HTMLTextAreaElement;
  area.value = text;
  area.dispatchEvent(new Event('input'));
}

describe('ChaptersDialog', () => {
  it('loads the chapters as lines and saves what was typed', async () => {
    const put = vi.fn().mockImplementation(async (_id: string, chapters: unknown) => chapters);
    const get = vi.fn().mockResolvedValue([{ start_ms: 65_000, title: 'Demo' }]);
    const { fixture, root } = await open({ get, put });
    expect((q(root, 'chapters-text') as HTMLTextAreaElement).value).toBe('1:05 Demo');

    type(root, '0:00 Intro\n1:05 Demo');
    (q(root, 'chapters-save') as HTMLButtonElement).click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(put).toHaveBeenCalledWith('rec-1', [
      { start_ms: 0, title: 'Intro' },
      { start_ms: 65_000, title: 'Demo' },
    ]);
    expect(q(root, 'chapters-saved')).not.toBeNull();
  });

  it('offers nothing to type into until the chapters have loaded', async () => {
    // `showModal()` shows the dialog before Angular renders "Loading…": a textarea already in
    // the DOM could be typed into, then reset by the load, and an empty list saved.
    let resolve: (chapters: never[]) => void = () => undefined;
    const get = vi.fn().mockImplementation(() => new Promise((done) => (resolve = done)));
    TestBed.configureTestingModule({
      imports: [ChaptersDialog],
      providers: [{ provide: ChaptersApi, useValue: { get, put: vi.fn() } }],
    });
    const fixture = TestBed.createComponent(ChaptersDialog);
    fixture.detectChanges();
    const root = fixture.nativeElement as HTMLElement;
    expect(q(root, 'chapters-text')).toBeNull();

    const opened = fixture.componentInstance.open('rec-1');
    resolve([]);
    await opened;
    fixture.detectChanges();
    expect(q(root, 'chapters-text')).not.toBeNull();

    (q(root, 'chapters-close') as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(q(root, 'chapters-text')).toBeNull();
  });

  it('names a bad line and does not call the API', async () => {
    const put = vi.fn();
    const { fixture, root } = await open({ get: vi.fn().mockResolvedValue([]), put });
    type(root, '0:00 Intro\nsoon Demo');
    (q(root, 'chapters-save') as HTMLButtonElement).click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(put).not.toHaveBeenCalled();
    expect(q(root, 'chapters-error')?.textContent).toContain('Line 2');
  });
});
