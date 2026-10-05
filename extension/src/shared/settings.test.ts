import { describe, expect, it } from 'vitest';

import { DEFAULT_SETTINGS, normalizeSettings } from './settings';

describe('normalizeSettings', () => {
  it('defaults to highlights on and keystrokes off', () => {
    expect(DEFAULT_SETTINGS).toEqual({
      tabAudio: true,
      microphone: false,
      highlights: true,
      keystrokes: false,
    });
    expect(normalizeSettings(undefined)).toEqual(DEFAULT_SETTINGS);
    expect(normalizeSettings(null)).toEqual(DEFAULT_SETTINGS);
  });

  it('keeps valid choices and replaces malformed ones', () => {
    expect(
      normalizeSettings({ tabAudio: false, microphone: true, highlights: false, keystrokes: true }),
    ).toEqual({ tabAudio: false, microphone: true, highlights: false, keystrokes: true });
    expect(normalizeSettings({ highlights: 'yes', keystrokes: 1 })).toEqual(DEFAULT_SETTINGS);
    expect(normalizeSettings({ keystrokes: true })).toEqual({
      ...DEFAULT_SETTINGS,
      keystrokes: true,
    });
  });
});
