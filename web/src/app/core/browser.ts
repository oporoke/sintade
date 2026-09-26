export type Engine = 'chromium' | 'firefox' | 'safari' | 'other';
export type Os = 'windows' | 'mac' | 'linux' | 'chromeos' | 'android' | 'ios' | 'other';

export interface BrowserInfo {
  engine: Engine;
  os: Os;
  /** Phones and tablets: view only, since mobile browsers can't record (README §4.1). */
  mobile: boolean;
}

/**
 * Which browser and OS this is, for help text and the mobile notice. UA-based on purpose: the
 * help has to name the browser's own menus. iPadOS reports a desktop Mac UA, so a Mac UA with
 * a touch screen is treated as an iPad.
 */
export function detectBrowser(ua: string, maxTouchPoints = 0): BrowserInfo {
  const iPadAsMac = /Macintosh/.test(ua) && maxTouchPoints > 1;
  const os: Os =
    /iPhone|iPad|iPod/.test(ua) || iPadAsMac
      ? 'ios'
      : /Android/.test(ua)
        ? 'android'
        : /CrOS/.test(ua)
          ? 'chromeos'
          : /Windows/.test(ua)
            ? 'windows'
            : /Macintosh|Mac OS X/.test(ua)
              ? 'mac'
              : /Linux/.test(ua)
                ? 'linux'
                : 'other';
  const engine: Engine = /Firefox\/|FxiOS\//.test(ua)
    ? 'firefox'
    : /Chrome\/|Chromium\/|Edg\/|CriOS\//.test(ua)
      ? 'chromium'
      : /Safari\//.test(ua)
        ? 'safari'
        : 'other';
  const mobile = os === 'ios' || os === 'android' || /Mobi/.test(ua);
  return { engine, os, mobile };
}

export function currentBrowser(): BrowserInfo {
  if (typeof navigator === 'undefined') {
    return { engine: 'other', os: 'other', mobile: false };
  }
  return detectBrowser(navigator.userAgent, navigator.maxTouchPoints ?? 0);
}
