import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';

import { ChunkStore, OrphanTake, assembleTake, listOrphans } from '../../capture';
import { CHUNK_STORE } from '../../core/capture.tokens';
import { formatDuration, formatStart } from './format';

/**
 * On app load, offers every unfinished recording left behind by a crashed or closed tab
 * (§10 Recover, US-12): "Unfinished recording from 14:02, 6 min 20 s". Discard deletes it; Save
 * a copy downloads the reassembled file. Upload arrives with streaming upload (Days 37–39),
 * so it is shown but disabled until then.
 */
@Component({
  selector: 'app-recovery-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dialog #dialog data-testid="recovery-dialog" aria-labelledby="recovery-title">
      <h2 id="recovery-title" i18n>Unfinished recordings</h2>
      <p i18n>
        These were still being recorded when the tab closed. They're stored on this device.
      </p>
      @for (orphan of orphans(); track orphan.takeId) {
        <section
          data-testid="recovery-take"
          [attr.data-take-id]="orphan.takeId"
          [attr.data-duration-ms]="orphan.durationMs"
        >
          <p data-testid="recovery-summary">{{ summary(orphan) }}</p>
          <button type="button" disabled aria-describedby="recovery-upload-note" i18n>
            Upload
          </button>
          <button type="button" (click)="save(orphan)" data-testid="recovery-save" i18n>
            Save a copy
          </button>
          <button type="button" (click)="discard(orphan)" data-testid="recovery-discard" i18n>
            Discard
          </button>
        </section>
      }
      <p id="recovery-upload-note" i18n>
        Uploading a recovered recording will be available once uploads are enabled.
      </p>
      @if (error()) {
        <p role="alert" data-testid="recovery-error">{{ error() }}</p>
      }
      <button type="button" (click)="close()" data-testid="recovery-later" i18n>
        Decide later
      </button>
    </dialog>
  `,
})
export class RecoveryDialog {
  private readonly storePromise = inject(CHUNK_STORE);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  private store: ChunkStore | null = null;

  protected readonly orphans = signal<OrphanTake[]>([]);
  protected readonly error = signal<string | null>(null);

  constructor() {
    effect(() => {
      const element = this.dialog().nativeElement;
      if (this.orphans().length > 0) {
        if (!element.open) {
          open(element);
        }
      } else if (element.open) {
        shut(element);
      }
    });
    void this.load();
  }

  protected summary(orphan: OrphanTake): string {
    if (orphan.startedAt === null || orphan.durationMs === null) {
      return $localize`Unfinished recording (start time and length unknown)`;
    }
    return $localize`Unfinished recording from ${formatStart(orphan.startedAt)}:start:, ${formatDuration(orphan.durationMs)}:duration:`;
  }

  async save(orphan: OrphanTake): Promise<void> {
    await this.act(async (store) => {
      const { blob } = await assembleTake(store, orphan.takeId);
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `sintade-${orphan.takeId}.${blob.type.includes('mp4') ? 'mp4' : 'webm'}`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    });
  }

  async discard(orphan: OrphanTake): Promise<void> {
    await this.act(async (store) => {
      await store.deleteTake(orphan.takeId);
      this.orphans.update((orphans) => orphans.filter((o) => o.takeId !== orphan.takeId));
    });
  }

  close(): void {
    shut(this.dialog().nativeElement);
  }

  private async load(): Promise<void> {
    try {
      this.store = await this.storePromise;
      this.orphans.set(await listOrphans(this.store));
    } catch {
      // No local storage (or it failed to open): nothing can have been left behind to recover.
    }
  }

  private async act(action: (store: ChunkStore) => Promise<void>): Promise<void> {
    if (!this.store) {
      return;
    }
    this.error.set(null);
    try {
      await action(this.store);
    } catch (error) {
      this.error.set(error instanceof Error ? error.message : String(error));
    }
  }
}

/** jsdom and very old engines lack showModal()/close(); the open attribute is the fallback. */
function open(dialog: HTMLDialogElement): void {
  if (typeof dialog.showModal === 'function') {
    dialog.showModal();
  } else {
    dialog.setAttribute('open', '');
  }
}

function shut(dialog: HTMLDialogElement): void {
  if (typeof dialog.close === 'function') {
    dialog.close();
  } else {
    dialog.removeAttribute('open');
  }
}
