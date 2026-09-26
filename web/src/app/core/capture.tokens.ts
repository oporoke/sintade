import { InjectionToken } from '@angular/core';

import { AudioMixer, SourceManager } from '../capture';

/*
 * Angular's handles on the framework-free capture engine. The capture package itself knows
 * nothing about DI; pages inject these so tests can substitute fakes.
 */

export const SOURCE_MANAGER = new InjectionToken<SourceManager>('SourceManager', {
  factory: () => new SourceManager(),
});

/** A fresh mixer per injector: each recorder/page owns its own Web Audio graph. */
export const AUDIO_MIXER = new InjectionToken<AudioMixer>('AudioMixer', {
  factory: () => new AudioMixer(),
});
