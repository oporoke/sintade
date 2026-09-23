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

@Injectable({ providedIn: 'root' })
export class CapabilityService {
  readonly capabilities = signal<CapabilityMatrix>(detectCapabilities());
}

function detectCapabilities(): CapabilityMatrix {
  return {
    getDisplayMedia: typeof navigator !== 'undefined' && !!navigator.mediaDevices?.getDisplayMedia,
    mediaRecorderWebm:
      typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported('video/webm'),
    opfs: typeof navigator !== 'undefined' && typeof navigator.storage?.getDirectory === 'function',
    systemAudio: detectSystemAudioHeuristic(),
  };
}

function detectSystemAudioHeuristic(): boolean {
  if (typeof navigator === 'undefined') {
    return false;
  }
  const ua = navigator.userAgent;
  const isChromiumFamily = /Chrome|Chromium|Edg\//.test(ua);
  const isOpera = /OPR\//.test(ua);
  return isChromiumFamily && !isOpera;
}
