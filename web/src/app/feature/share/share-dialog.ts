import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';

import { SHARE_API, ShareLink, Visibility, shareUrl } from '../../core/share-api.service';

const VISIBILITIES: { value: Visibility; label: string }[] = [
  { value: 'link', label: $localize`Anyone with the link` },
  { value: 'workspace', label: $localize`People in my workspace` },
  { value: 'private', label: $localize`Only me` },
  { value: 'public', label: $localize`Public` },
];

/**
 * Who can watch a recording: choose the visibility, copy the link, allow downloads, or turn the
 * link off. Opens as a modal from a "Share" button (`open()`); changes save as they're made.
 */
@Component({
  selector: 'app-share-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dialog #dialog data-testid="share-dialog" aria-labelledby="share-title">
      <h2 id="share-title" i18n>Share</h2>
      @if (loading()) {
        <p role="status" i18n>Loading…</p>
      } @else if (link(); as current) {
        <label>
          <span i18n>Link</span>
          <input
            type="text"
            readonly
            data-testid="share-url"
            [value]="url(current)"
            (focus)="$any($event.target).select()"
          />
        </label>
        <a [href]="url(current)" target="_blank" rel="noopener" data-testid="share-open" i18n
          >Open</a
        >
        <button type="button" data-testid="share-copy" (click)="copy(current)" i18n>
          Copy link
        </button>
        @if (copied()) {
          <span role="status" data-testid="share-copied" i18n>Link copied</span>
        }
        <label>
          <span i18n>Who can watch</span>
          <select
            data-testid="share-visibility"
            [value]="current.visibility"
            (change)="setVisibility($any($event.target).value)"
          >
            @for (option of visibilities; track option.value) {
              <option [value]="option.value" [selected]="option.value === current.visibility">
                {{ option.label }}
              </option>
            }
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            data-testid="share-download"
            [checked]="current.allow_download"
            (change)="setDownload($any($event.target).checked)"
          />
          <span i18n>Let viewers download the video</span>
        </label>
        <button type="button" data-testid="share-revoke" (click)="revoke(current)" i18n>
          Turn off this link
        </button>
      } @else {
        <p data-testid="share-no-link" i18n>This recording has no active link.</p>
        <button type="button" data-testid="share-create" (click)="create()" i18n>
          Create a link
        </button>
      }
      @if (error(); as message) {
        <p role="alert" data-testid="share-error">{{ message }}</p>
      }
      <button type="button" data-testid="share-close" (click)="close()" i18n>Close</button>
    </dialog>
  `,
})
export class ShareDialog {
  /** The recording to manage; `open(id)` can point one dialog at several recordings. */
  readonly recordingId = input<string | null>(null);

  private readonly api = inject(SHARE_API);
  private target: string | null = null;
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');

  protected readonly visibilities = VISIBILITIES;
  protected readonly loading = signal(false);
  protected readonly link = signal<ShareLink | null>(null);
  protected readonly copied = signal(false);
  protected readonly error = signal<string | null>(null);

  async open(recordingId?: string): Promise<void> {
    this.target = recordingId ?? this.recordingId();
    if (!this.target) {
      return;
    }
    const element = this.dialog().nativeElement;
    if (typeof element.showModal === 'function') {
      element.showModal();
    } else {
      element.setAttribute('open', '');
    }
    this.error.set(null);
    this.copied.set(false);
    this.loading.set(true);
    try {
      const links = await this.api.list(this.target ?? '');
      this.link.set(links.find((candidate) => candidate.revoked_at === null) ?? null);
    } catch {
      this.error.set($localize`Couldn't load the sharing settings.`);
    } finally {
      this.loading.set(false);
    }
  }

  protected close(): void {
    const element = this.dialog().nativeElement;
    if (typeof element.close === 'function') {
      element.close();
    } else {
      element.removeAttribute('open');
    }
  }

  protected url(link: ShareLink): string {
    return shareUrl(link.slug);
  }

  protected async copy(link: ShareLink): Promise<void> {
    try {
      await navigator.clipboard.writeText(this.url(link));
      this.copied.set(true);
    } catch {
      this.error.set($localize`Couldn't copy automatically. Select the link and copy it.`);
    }
  }

  protected create(): Promise<void> {
    return this.run(async () => {
      this.link.set(await this.api.create(this.target ?? '', { visibility: 'link' }));
    });
  }

  protected setVisibility(value: Visibility): Promise<void> {
    return this.change({ visibility: value });
  }

  protected setDownload(allow: boolean): Promise<void> {
    return this.change({ allow_download: allow });
  }

  protected revoke(link: ShareLink): Promise<void> {
    return this.run(async () => {
      await this.api.revoke(this.target ?? '', link.id);
      this.link.set(null);
    });
  }

  private change(body: { visibility?: Visibility; allow_download?: boolean }): Promise<void> {
    const current = this.link();
    if (!current) {
      return Promise.resolve();
    }
    return this.run(async () => {
      this.link.set(await this.api.update(this.target ?? '', current.id, body));
    });
  }

  private async run(action: () => Promise<void>): Promise<void> {
    this.error.set(null);
    this.copied.set(false);
    try {
      await action();
    } catch {
      this.error.set($localize`That didn't work. Try again.`);
    }
  }
}
