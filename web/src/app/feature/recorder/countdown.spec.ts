import { TestBed } from '@angular/core/testing';

import { Countdown } from './countdown';

function setup() {
  const fixture = TestBed.createComponent(Countdown);
  fixture.detectChanges();
  const element: HTMLElement = fixture.nativeElement;
  const number = () =>
    element.querySelector('[data-testid="countdown-number"]')?.textContent?.trim() ?? null;
  return { fixture, countdown: fixture.componentInstance, number };
}

describe('Countdown', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('counts 3, 2, 1 a second apart and then finishes', async () => {
    const { fixture, countdown, number } = setup();
    const outcome = countdown.run();
    const seen: (string | null)[] = [];
    for (let i = 0; i < 3; i += 1) {
      fixture.detectChanges();
      seen.push(number());
      await vi.advanceTimersByTimeAsync(1000);
    }
    fixture.detectChanges();

    expect(seen).toEqual(['3', '2', '1']);
    await expect(outcome).resolves.toBe('finished');
    expect(number()).toBeNull();
  });

  it('Esc skips straight to the start', async () => {
    const { fixture, countdown, number } = setup();
    const outcome = countdown.run();
    fixture.detectChanges();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    fixture.detectChanges();

    await expect(outcome).resolves.toBe('skipped');
    expect(number()).toBeNull();
  });

  it('ignores other keys', async () => {
    const { countdown } = setup();
    const outcome = countdown.run();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await vi.advanceTimersByTimeAsync(3000);
    await expect(outcome).resolves.toBe('finished');
  });

  it('cancel() and destroy end it as cancelled and stop listening', async () => {
    const { fixture, countdown } = setup();
    const first = countdown.run();
    countdown.cancel();
    await expect(first).resolves.toBe('cancelled');

    const second = countdown.run();
    fixture.destroy();
    await expect(second).resolves.toBe('cancelled');
    expect(countdown.running).toBe(false);
  });
});
