import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { RouterLink } from '@angular/router';

import { LIBRARY_API, RecordingSummary } from '../../core/library-api.service';
import { formatDuration } from '../recorder/format';
import { ShareDialog } from '../share/share-dialog';

/**
 * The library (docs/design.md §4.12): the workspace's recordings, newest first, 24 at a time,
 * each with a thumbnail, length, date and state. Share opens the Share dialog for that
 * recording; Download saves the MP4.
 */
@Component({
  selector: 'app-library-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink, ShareDialog],
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
            <h2 data-testid="library-title">{{ item.title }}</h2>
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
                <button type="button" data-testid="library-download" (click)="download(item)" i18n>
                  Download
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
    <app-share-dialog />
  `,
})
export class LibraryPage implements OnInit {
  private readonly api = inject(LIBRARY_API);
  private readonly shareDialog = viewChild.required(ShareDialog);

  protected readonly items = signal<RecordingSummary[]>([]);
  protected readonly nextCursor = signal<string | null>(null);
  protected readonly loaded = signal(false);
  protected readonly loading = signal(false);
  protected readonly error = signal(false);

  ngOnInit(): void {
    void this.loadMore();
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
