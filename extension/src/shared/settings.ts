/** What the user chose to show on top of the recorded tab. */
export interface Settings {
  /** Record the tab's own sound. */
  tabAudio: boolean;
  /** Mix in the microphone (needs the browser's permission, asked for once on its own page). */
  microphone: boolean;
  /** A ring where the mouse is pressed. */
  highlights: boolean;
  /** The keys pressed (never from password fields). Off by default: it can reveal what is typed. */
  keystrokes: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  tabAudio: true,
  microphone: false,
  highlights: true,
  keystrokes: false,
};

export const SETTINGS_KEY = 'settings';

/** Fills in anything missing or malformed in what was stored. */
export function normalizeSettings(stored: unknown): Settings {
  const value = (typeof stored === 'object' && stored !== null ? stored : {}) as Record<
    string,
    unknown
  >;
  const flag = (key: keyof Settings) =>
    typeof value[key] === 'boolean' ? value[key] : DEFAULT_SETTINGS[key];
  return {
    tabAudio: flag('tabAudio'),
    microphone: flag('microphone'),
    highlights: flag('highlights'),
    keystrokes: flag('keystrokes'),
  };
}
