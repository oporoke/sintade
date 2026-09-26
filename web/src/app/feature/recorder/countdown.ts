import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  input,
  signal,
} from '@angular/core';

export type CountdownOutcome = 'finished' | 'skipped' | 'cancelled';

/**
 * The 3-2-1 before recording starts (US-11: "3-2-1 countdown, skippable with Esc"). `run()`
 * resolves when it reaches zero, when the user presses Esc (skipped), or when `cancel()` is
 * called. Numbers are announced to screen readers.
 */
@Component({
  selector: 'app-countdown',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (remaining() > 0) {
      <div class="countdown" role="timer" aria-live="assertive" data-testid="countdown">
        <span class="countdown__number" data-testid="countdown-number">{{ remaining() }}</span>
        <span class="countdown__hint" i18n>Press Esc to start now</span>
      </div>
    }
  `,
  styles: `
    .countdown {
      display: grid;
      place-items: center;
      gap: 0.5rem;
      padding: 2rem;
    }
    .countdown__number {
      font-size: 6rem;
      font-weight: 700;
      line-height: 1;
    }
  `,
})
export class Countdown {
  readonly seconds = input(3);
  protected readonly remaining = signal(0);
  private finish: ((outcome: CountdownOutcome) => void) | null = null;
  private timer: ReturnType<typeof setInterval> | undefined;
  private readonly onKeydown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      this.end('skipped');
    }
  };

  constructor() {
    inject(DestroyRef).onDestroy(() => this.end('cancelled'));
  }

  get running(): boolean {
    return this.finish !== null;
  }

  run(): Promise<CountdownOutcome> {
    this.end('cancelled');
    return new Promise((resolve) => {
      this.finish = resolve;
      this.remaining.set(this.seconds());
      document.addEventListener('keydown', this.onKeydown);
      this.timer = setInterval(() => {
        const next = this.remaining() - 1;
        if (next <= 0) {
          this.end('finished');
        } else {
          this.remaining.set(next);
        }
      }, 1000);
    });
  }

  cancel(): void {
    this.end('cancelled');
  }

  private end(outcome: CountdownOutcome): void {
    clearInterval(this.timer);
    this.timer = undefined;
    document.removeEventListener('keydown', this.onKeydown);
    this.remaining.set(0);
    const finish = this.finish;
    this.finish = null;
    finish?.(outcome);
  }
}
