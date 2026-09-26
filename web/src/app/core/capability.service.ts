import { Injectable, signal } from '@angular/core';

export interface CapabilityMatrix {
  getDisplayMedia: boolean;
  mediaRecorderWebm: boolean;
  opfs: boolean;
  /**
   * Whether the browser can capture system/tab audio via `getDisplayMedia`. There is no
   * synchronous feature-detection API for this, so it's a UA-based heuristic
   * (Chromium-family only, as of this writing) — TODO: Verify against real Safari/Firefox
   * releases if this ever needs to be precise rather than indicative.
   */
  systemAudio: boolean;
}

/**
 * Whether system/tab audio can be recorded here, and the one-line reason or caveat to show
 * (US-10; README §4.1 support table). There's no feature-detection API for this, so it's
 * UA-based.
 */
export type SystemAudioSupport =
  { supported: true; note: string | null } | { supported: false; reason: string };

@Injectable({ providedIn: 'root' })
export class CapabilityService {
  readonly capabilities = signal<CapabilityMatrix>(detectCapabilities());
  readonly systemAudio = signal<SystemAudioSupport>(systemAudioSupport(userAgent()));
}

export function systemAudioSupport(ua: string): SystemAudioSupport {
  const chromium = /Chrome\/|Chromium\/|Edg\//.test(ua) && !/OPR\//.test(ua);
  if (chromium) {
    return /Windows/.test(ua)
      ? { supported: true, note: null }
      : {
          supported: true,
          note: $localize`On this system only a browser tab's audio can be captured, not the whole system's.`,
        };
  }
  if (/Firefox\//.test(ua)) {
    return { supported: false, reason: $localize`Firefox can't record system or tab audio.` };
  }
  if (/Safari\//.test(ua)) {
    return { supported: false, reason: $localize`Safari can't record system or tab audio.` };
  }
  return { supported: false, reason: $localize`This browser can't record system audio.` };
}

function userAgent(): string {
  return typeof navigator === 'undefined' ? '' : navigator.userAgent;
}

function detectCapabilities(): CapabilityMatrix {
  return {
    getDisplayMedia: typeof navigator !== 'undefined' && !!navigator.mediaDevices?.getDisplayMedia,
    mediaRecorderWebm:
      typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported('video/webm'),
    opfs: typeof navigator !== 'undefined' && typeof navigator.storage?.getDirectory === 'function',
    systemAudio: systemAudioSupport(userAgent()).supported,
  };
}
