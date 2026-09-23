import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';

import { CapabilityService, CapabilityMatrix } from '../../core/capability.service';
import { DebugPage } from './debug-page';

describe('DebugPage', () => {
  it('renders one row per capability with its detected value', () => {
    const fakeCapabilities: CapabilityMatrix = {
      getDisplayMedia: true,
      mediaRecorderWebm: false,
      opfs: true,
      systemAudio: false,
    };
    const capabilityServiceStub: Partial<CapabilityService> = {
      capabilities: signal(fakeCapabilities),
    };

    TestBed.configureTestingModule({
      imports: [DebugPage],
      providers: [{ provide: CapabilityService, useValue: capabilityServiceStub }],
    });

    const fixture = TestBed.createComponent(DebugPage);
    fixture.detectChanges();
    const element: HTMLElement = fixture.nativeElement;

    expect(element.querySelector('[data-testid="capability-value-getDisplayMedia"]')?.textContent?.trim()).toBe(
      'true',
    );
    expect(
      element.querySelector('[data-testid="capability-value-mediaRecorderWebm"]')?.textContent?.trim(),
    ).toBe('false');
    expect(element.querySelectorAll('[data-testid^="capability-row-"]').length).toBe(4);
  });
});
