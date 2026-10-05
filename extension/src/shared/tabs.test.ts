import { describe, expect, it } from 'vitest';

import { unrecordableReason } from './tabs';

describe('unrecordableReason', () => {
  it('allows ordinary web pages', () => {
    for (const url of ['https://example.com/a?b=1', 'http://localhost:4200/home']) {
      expect(unrecordableReason(url)).toBeNull();
    }
  });

  it('refuses browser pages, files and extensions', () => {
    for (const url of [
      'chrome://extensions',
      'edge://settings',
      'about:blank',
      'file:///home/me/a.html',
      'chrome-extension://abc/popup.html',
      'view-source:https://example.com',
    ]) {
      expect(unrecordableReason(url), url).toMatch(/extensions|record/);
    }
  });

  it('refuses the extension stores', () => {
    expect(unrecordableReason('https://chromewebstore.google.com/detail/x')).toMatch(/store/);
    expect(unrecordableReason('https://microsoftedge.microsoft.com/addons/x')).toMatch(/store/);
  });

  it('refuses a tab it cannot see or parse', () => {
    expect(unrecordableReason(undefined)).toMatch(/can't see/);
    expect(unrecordableReason('not a url')).toMatch(/can't record/);
  });
});
