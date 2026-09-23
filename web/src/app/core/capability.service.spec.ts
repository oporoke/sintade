import { TestBed } from '@angular/core/testing';

import { CapabilityService } from './capability.service';

describe('CapabilityService', () => {
  it('reports a boolean for every capability', () => {
    const service = TestBed.inject(CapabilityService);
    const capabilities = service.capabilities();

    expect(typeof capabilities.getDisplayMedia).toBe('boolean');
    expect(typeof capabilities.mediaRecorderWebm).toBe('boolean');
    expect(typeof capabilities.opfs).toBe('boolean');
    expect(typeof capabilities.systemAudio).toBe('boolean');
  });
});
