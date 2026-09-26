const KEY = 'sintade.recorder.mic';

/**
 * The last microphone the user chose (US-10: "last choice remembered"). A per-device
 * convenience only: storage can be unavailable (private mode, blocked site data), so every
 * access is guarded and a missing value just means "no preference".
 */
export const micPreference = {
  load(): string | null {
    try {
      return localStorage.getItem(KEY);
    } catch {
      return null;
    }
  },
  save(deviceId: string | null): void {
    try {
      if (deviceId) {
        localStorage.setItem(KEY, deviceId);
      } else {
        localStorage.removeItem(KEY);
      }
    } catch {
      // Not remembering is fine.
    }
  },
};
