/** What the user chose to show on top of the recorded tab. */
export interface Settings {
  /** A ring where the mouse is pressed. */
  highlights: boolean;
  /** The keys pressed (never from password fields). Off by default: it can reveal what is typed. */
  keystrokes: boolean;
}

export const DEFAULT_SETTINGS: Settings = { highlights: true, keystrokes: false };

export const SETTINGS_KEY = 'settings';

/** Fills in anything missing or malformed in what was stored. */
export function normalizeSettings(stored: unknown): Settings {
  const value = (typeof stored === 'object' && stored !== null ? stored : {}) as Partial<Settings>;
  return {
    highlights:
      typeof value.highlights === 'boolean' ? value.highlights : DEFAULT_SETTINGS.highlights,
    keystrokes:
      typeof value.keystrokes === 'boolean' ? value.keystrokes : DEFAULT_SETTINGS.keystrokes,
  };
}
