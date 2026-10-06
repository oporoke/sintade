import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  Injector,
  OnInit,
  afterNextRender,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { RouterLink } from '@angular/router';
import { Subscription } from 'rxjs';

import { LIBRARY_API, RecordingSummary } from '../../core/library-api.service';
import { STATUS_STREAM } from '../../core/status-stream.service';
import { formatDuration } from '../recorder/format';
import { ChaptersDialog } from '../chapters/chapters-dialog';
import { ShareDialog } from '../share/share-dialog';

/**
 * The library (docs/design.md §4.12): the workspace's recordings, newest first, 24 at a time,
 * each with a thumbnail, length, date and state. Share opens the Share dialog for that
 * recording; Download saves the MP4.
 */
@Component({
  selector: 'app-library-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink, ShareDialog, ChaptersDialog],
  styles: `
    :host {
      display: block;
      max-width: 1100px;
      margin: 0 auto;
      padding: var(--space-3);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
      gap: var(--space-3);
      list-style: none;
      padding: 0;
    }
    .card {
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      overflow: hidden;
    }
    .thumb {
      aspect-ratio: 16 / 9;
      width: 100%;
      object-fit: cover;
      display: block;
      background: var(--color-surface-raised);
    }
    .placeholder {
      aspect-ratio: 16 / 9;
      display: grid;
      place-items: center;
      background: var(--color-surface-raised);
      color: var(--color-text-muted);
    }
    .body {
      padding: var(--space-2) var(--space-3) var(--space-3);
    }
    .meta {
      color: var(--color-text-muted);
      margin: 0;
    }
    .title {
      all: unset;
      cursor: text;
    }
    .actions {
      display: flex;
      gap: var(--space-2);
      margin-top: var(--space-2);
    }
  `,
  template: `
    <h1 i18n>Your recordings</h1>
    @if (error()) {
      <div role="alert" data-testid="library-error">
        <p i18n>We couldn't load your recordings.</p>
        <button type="button" (click)="loadMore()" i18n>Try again</button>
      </div>
    }
    @if (loaded() && items().length === 0 && !error()) {
      <div data-testid="library-empty">
        <p i18n>You haven't recorded anything yet.</p>
        <p>
          <a routerLink="/record" data-testid="library-record" i18n>Make your first recording</a>
        </p>
      </div>
    }
    <ul class="grid" data-testid="library-grid">
      @for (item of items(); track item.id; let index = $index) {
        <li class="card" data-testid="library-item" [attr.data-recording-id]="item.id">
          @if (item.poster_url) {
            <img
              class="thumb"
              [src]="item.poster_url"
              alt=""
              [attr.loading]="index < 6 ? 'eager' : 'lazy'"
            />
          } @else {
            <div class="placeholder" aria-hidden="true">{{ stateLabel(item) }}</div>
          }
          <div class="body">
            @if (editing() === item.id) {
              <form (submit)="saveTitle($event, item, titleInput.value)">
                <input
                  #titleInput
                  data-testid="library-title-input"
                  [value]="item.title"
                  maxlength="200"
                  aria-label="Title"
                  i18n-aria-label
                  (keydown.escape)="cancelRename()"
                  (blur)="cancelRename()"
                />
              </form>
            } @else {
              <h2 data-testid="library-title">
                <button
                  type="button"
                  class="title"
                  data-testid="library-rename"
                  title="Rename"
                  i18n-title
                  (click)="startRename(item)"
                >
                  {{ item.title }}
                </button>
              </h2>
            }
            <p class="meta">
              @if (item.duration_ms) {
                <span data-testid="library-duration">{{ duration(item.duration_ms) }}</span> ·
              }
              <time [attr.datetime]="item.created_at" data-testid="library-date">{{
                date(item.created_at)
              }}</time>
              @if (item.state !== 'ready') {
                · <span data-testid="library-state">{{ stateLabel(item) }}</span>
              }
            </p>
            <div class="actions">
              <button type="button" data-testid="library-share" (click)="share(item)" i18n>
                Share
              </button>
              @if (item.state === 'ready') {
                <button type="button" data-testid="library-chapters" (click)="chapters(item)" i18n>
                  Chapters
                </button>
                <button type="button" data-testid="library-download" (click)="download(item)" i18n>
                  Download
                </button>
              }
              @if (confirming() === item.id) {
                <span role="group" aria-label="Confirm" i18n-aria-label>
                  <span i18n>Move to trash?</span>
                  <button type="button" data-testid="library-trash-yes" (click)="trash(item)" i18n>
                    Yes
                  </button>
                  <button
                    type="button"
                    data-testid="library-trash-no"
                    (click)="confirming.set(null)"
                    i18n
                  >
                    No
                  </button>
                </span>
              } @else {
                <button
                  type="button"
                  data-testid="library-trash"
                  (click)="confirming.set(item.id)"
                  i18n
                >
                  Delete
                </button>
              }
            </div>
          </div>
        </li>
      }
    </ul>
    @if (nextCursor()) {
      <button
        type="button"
        data-testid="library-more"
        [disabled]="loading()"
        (click)="loadMore()"
        i18n
      >
        Load more
      </button>
    }
    @if (notice(); as message) {
      <p role="status" data-testid="library-notice">{{ message }}</p>
    }
    <app-share-dialog />
    <app-chapters-dialog />
  `,
})
export class LibraryPage implements OnInit {
  private readonly api = inject(LIBRARY_API);
  private readonly shareDialog = viewChild.required(ShareDialog);
  private readonly chaptersDialog = viewChild.required(ChaptersDialog);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly injector = inject(Injector);
  private readonly statusStream = inject(STATUS_STREAM);
  /** Open status streams, by recording: one per recording that is still being made ready. */
  private readonly following = new Map<string, Subscription>();

  protected readonly items = signal<RecordingSummary[]>([]);
  protected readonly nextCursor = signal<string | null>(null);
  protected readonly loaded = signal(false);
  protected readonly loading = signal(false);
  protected readonly error = signal(false);
  protected readonly editing = signal<string | null>(null);
  protected readonly confirming = signal<string | null>(null);
  protected readonly notice = signal<string | null>(null);

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      for (const subscription of this.following.values()) {
        subscription.unsubscribe();
      }
      this.following.clear();
    });
  }

  ngOnInit(): void {
    void this.loadMore();
  }

  /**
   * Recordings that aren't ready yet follow their live status (docs/design.md §9): the card
   * changes from "Processing" to its thumbnail by itself, with no reload. A recording whose
   * stream can't be opened simply stays as it was until the page is opened again.
   */
  private followUnfinished(): void {
    for (const item of this.items()) {
      if (
        (item.state !== 'processing' && item.state !== 'uploading') ||
        this.following.has(item.id)
      ) {
        continue;
      }
      const id = item.id;
      this.following.set(
        id,
        this.statusStream.follow(`/recordings/${encodeURIComponent(id)}/events`).subscribe({
          next: (status) => {
            this.items.update((all) =>
              all.map((each) => (each.id === id ? { ...each, state: status.state } : each)),
            );
            if (status.state === 'ready' || status.state === 'failed') {
              this.unfollow(id);
              if (status.state === 'ready') {
                void this.refreshItem(id);
              }
            }
          },
          error: () => this.unfollow(id),
          complete: () => this.unfollow(id),
        }),
      );
    }
  }

  private unfollow(id: string): void {
    this.following.get(id)?.unsubscribe();
    this.following.delete(id);
  }

  /** Takes what finishing added (thumbnail, length) from a fresh first page. */
  private async refreshItem(id: string): Promise<void> {
    try {
      const page = await this.api.list(null);
      const fresh = page.items.find((each) => each.id === id);
      if (fresh) {
        this.items.update((all) => all.map((each) => (each.id === id ? fresh : each)));
      }
    } catch {
      // The state is already right; the thumbnail arrives with the next load.
    }
  }

  protected async loadMore(): Promise<void> {
    if (this.loading()) {
      return;
    }
    this.loading.set(true);
    this.error.set(false);
    try {
      const page = await this.api.list(this.nextCursor());
      this.items.update((current) => [...current, ...page.items]);
      this.nextCursor.set(page.next_cursor ?? null);
      this.loaded.set(true);
      this.followUnfinished();
    } catch {
      this.error.set(true);
    } finally {
      this.loading.set(false);
    }
  }

  protected duration(ms: number): string {
    return formatDuration(ms);
  }

  protected date(iso: string): string {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(new Date(iso));
  }

  protected stateLabel(item: RecordingSummary): string {
    switch (item.state) {
      case 'ready':
        return $localize`Ready`;
      case 'failed':
        return $localize`Failed`;
      case 'processing':
        return $localize`Processing`;
      default:
        return $localize`Uploading`;
    }
  }

  protected startRename(item: RecordingSummary): void {
    this.notice.set(null);
    this.editing.set(item.id);
    afterNextRender(
      () => {
        const input = this.host.nativeElement.querySelector<HTMLInputElement>(
          '[data-testid="library-title-input"]',
        );
        input?.focus();
        input?.select();
      },
      { injector: this.injector },
    );
  }

  protected cancelRename(): void {
    this.editing.set(null);
  }

  protected async saveTitle(event: Event, item: RecordingSummary, value: string): Promise<void> {
    event.preventDefault();
    if (value.trim() === item.title) {
      this.editing.set(null);
      return;
    }
    try {
      const renamed = await this.api.rename(item.id, value);
      this.items.update((all) =>
        all.map((each) => (each.id === item.id ? { ...each, title: renamed.title } : each)),
      );
      this.editing.set(null);
    } catch {
      this.notice.set($localize`The title couldn't be saved. Try again.`);
    }
  }

  protected async trash(item: RecordingSummary): Promise<void> {
    this.confirming.set(null);
    try {
      await this.api.trash(item.id);
      this.items.update((all) => all.filter((each) => each.id !== item.id));
      this.notice.set(
        $localize`Moved to trash. Its links stopped working. It will be deleted for good in 30 days.`,
      );
    } catch {
      this.notice.set($localize`Couldn't move it to the trash. Try again.`);
    }
  }

  protected chapters(item: RecordingSummary): void {
    void this.chaptersDialog().open(item.id);
  }

  protected share(item: RecordingSummary): void {
    void this.shareDialog().open(item.id);
  }

  protected async download(item: RecordingSummary): Promise<void> {
    try {
      const grant = await this.api.download(item.id);
      const link = document.createElement('a');
      link.href = grant.url;
      link.download = grant.filename;
      link.rel = 'noopener';
      document.body.appendChild(link);
      link.click();
      link.remove();
    } catch {
      this.error.set(true);
    }
  }
}
