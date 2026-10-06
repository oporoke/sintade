import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { HttpErrorResponse } from '@angular/common/http';

import { ChaptersApi } from '../../core/chapters-api.service';
import { formatChapterLines, parseChapterLines } from './chapters-text';

/**
 * Edits a recording's chapters as text, one per line (`1:05 Demo`). They show on the watch page
 * as a table of contents and as marks on the scrub bar. Opens as a modal (`open(id)`).
 */
@Component({
  selector: 'app-chapters-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dialog #dialog data-testid="chapters-dialog" aria-labelledby="chapters-title">
      <h2 id="chapters-title" i18n>Chapters</h2>
      @if (loading()) {
        <p role="status" i18n>Loading…</p>
      } @else {
        <label>
          <span i18n>One per line: time, then title (for example 1:05 Demo)</span>
          <textarea
            data-testid="chapters-text"
            rows="8"
            [value]="text()"
            (input)="text.set($any($event.target).value)"
          ></textarea>
        </label>
        <button type="button" data-testid="chapters-save" (click)="save()" i18n>Save</button>
        @if (saved()) {
          <span role="status" data-testid="chapters-saved" i18n>Saved</span>
        }
      }
      @if (error(); as message) {
        <p role="alert" data-testid="chapters-error">{{ message }}</p>
      }
      <button type="button" data-testid="chapters-close" (click)="close()" i18n>Close</button>
    </dialog>
  `,
})
export class ChaptersDialog {
  private readonly api = inject(ChaptersApi);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  private target: string | null = null;

  protected readonly loading = signal(false);
  protected readonly text = signal('');
  protected readonly saved = signal(false);
  protected readonly error = signal<string | null>(null);

  async open(recordingId: string): Promise<void> {
    this.target = recordingId;
    const element = this.dialog().nativeElement;
    if (typeof element.showModal === 'function') {
      element.showModal();
    } else {
      element.setAttribute('open', '');
    }
    this.error.set(null);
    this.saved.set(false);
    this.loading.set(true);
    try {
      this.text.set(formatChapterLines(await this.api.get(recordingId)));
    } catch {
      this.error.set($localize`Couldn't load the chapters.`);
    } finally {
      this.loading.set(false);
    }
  }

  protected async save(): Promise<void> {
    this.saved.set(false);
    this.error.set(null);
    const parsed = parseChapterLines(this.text());
    if (!parsed.ok) {
      this.error.set($localize`Line ${parsed.line} should start with a time, like 1:05.`);
      return;
    }
    if (!this.target) {
      return;
    }
    try {
      const stored = await this.api.put(this.target, parsed.chapters);
      this.text.set(formatChapterLines(stored));
      this.saved.set(true);
    } catch (error) {
      const invalid = error instanceof HttpErrorResponse && error.status === 422;
      this.error.set(
        invalid
          ? $localize`Those chapters aren't valid: titles are 1–120 characters, times are different and inside the recording.`
          : $localize`Couldn't save the chapters. Try again.`,
      );
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
}
