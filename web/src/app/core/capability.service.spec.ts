import { TestBed } from '@angular/core/testing';

import { CapabilityService, systemAudioSupport } from './capability.service';

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

describe('systemAudioSupport', () => {
  const UA = {
    chromeWindows:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
    chromeMac:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
    edgeLinux:
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0',
    firefox: 'Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0',
    safari:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
    opera:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 OPR/120.0.0.0',
  };

  it('is fully supported on Chromium for Windows', () => {
    expect(systemAudioSupport(UA.chromeWindows)).toEqual({ supported: true, note: null });
  });

  it('is supported with a tab-only note on Chromium elsewhere', () => {
    for (const ua of [UA.chromeMac, UA.edgeLinux]) {
      const support = systemAudioSupport(ua);
      expect(support.supported).toBe(true);
      expect(support.supported && support.note).toMatch(/tab's audio/);
    }
  });

  it('is unsupported with a browser-specific reason on Firefox and Safari', () => {
    expect(systemAudioSupport(UA.firefox)).toEqual({
      supported: false,
      reason: "Firefox can't record system or tab audio.",
    });
    expect(systemAudioSupport(UA.safari)).toEqual({
      supported: false,
      reason: "Safari can't record system or tab audio.",
    });
  });

  it('is unsupported with a generic reason elsewhere (e.g. Opera, unknown)', () => {
    expect(systemAudioSupport(UA.opera).supported).toBe(false);
    expect(systemAudioSupport('')).toEqual({
      supported: false,
      reason: "This browser can't record system audio.",
    });
  });
});
